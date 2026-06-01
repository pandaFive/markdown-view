use std::path::Path;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use tokio::io::AsyncReadExt;

use super::resolve::{
    file_display_name, resolve_change_target_blocking, resolve_single_file_target_blocking,
    ResolveFileError, ResolvedTarget, RouteTargetRequest,
};
use crate::renderer::render_document;
use crate::server::log_path::sanitize_path_for_logging;
use crate::server::messages::{ApiError, BroadcastMessage, LaggedRecoveryMessage};
use crate::server::state::AppState;
use crate::template::{error_message_json, UpdateMessage};

/// ファイルサイズ上限: OOM防止
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
const FILE_SIZE_LIMIT_ERROR_MESSAGE: &str = "ファイルサイズが上限（10MB）を超えています";

#[cfg(test)]
type ContentBeforeReadHook = std::sync::Arc<dyn Fn(&Path) + Send + Sync + 'static>;

#[cfg(test)]
static CONTENT_BEFORE_READ_HOOK: std::sync::OnceLock<
    std::sync::Mutex<Option<ContentBeforeReadHook>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
static CONTENT_TEST_OVERRIDE_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
    std::sync::OnceLock::new();

#[cfg(test)]
type ContentTestOverrideLockGuard = std::sync::MutexGuard<'static, ()>;

#[cfg(test)]
pub(in crate::server) struct ContentBeforeReadHookGuard {
    _lock: ContentTestOverrideLockGuard,
}

#[cfg(test)]
fn lock_content_test_override() -> ContentTestOverrideLockGuard {
    CONTENT_TEST_OVERRIDE_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
impl Drop for ContentBeforeReadHookGuard {
    fn drop(&mut self) {
        let hook = CONTENT_BEFORE_READ_HOOK.get_or_init(|| std::sync::Mutex::new(None));
        *hook.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

#[cfg(test)]
pub(in crate::server) fn set_content_before_read_hook_for_test(
    hook: ContentBeforeReadHook,
) -> ContentBeforeReadHookGuard {
    let lock = lock_content_test_override();
    let slot = CONTENT_BEFORE_READ_HOOK.get_or_init(|| std::sync::Mutex::new(None));
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    ContentBeforeReadHookGuard { _lock: lock }
}

#[cfg(test)]
fn notify_content_before_read_for_test(file_path: &Path) {
    let hook = CONTENT_BEFORE_READ_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(hook) = hook {
        hook(file_path);
    }
}

#[cfg(not(test))]
fn notify_content_before_read_for_test(_file_path: &Path) {}

#[derive(Debug, Clone)]
/// WebSocket初期化時のエラー。closeフレームのコードと理由を保持する。
pub(in crate::server) struct SocketInitError {
    close_code: u16,
    reason: String,
}

impl SocketInitError {
    fn new(close_code: u16, reason: impl Into<String>) -> Self {
        Self {
            close_code,
            reason: reason.into(),
        }
    }

    pub(in crate::server) fn close_code(&self) -> u16 {
        self.close_code
    }

    pub(in crate::server) fn reason(&self) -> &str {
        &self.reason
    }
}

/// Markdownの読み込みと描画を行い、失敗時はAPI応答用のエラーへ変換する。
pub(in crate::server) async fn load_route_update(
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<UpdateMessage, ApiError> {
    read_and_render_target(target)
        .await
        .map(|update| target.attach_file_info(update))
        .map_err(|error| {
            tracing::warn!(
                "[markdown-view] {}読み込みエラー ({}): {}",
                request.read_error_log_label(),
                target.file_label(),
                error
            );
            crate::server::guards::json_error(error.status_code(), error.user_message())
        })
}

/// ターゲット解決＋読込＋描画の統一結果。
///
/// 3関数（初期化・遅延回復・変更通知）でエラーマッピングと成功型だけが異なり、
/// 制御フローは共通であるため、中間型に畳み込んでラッパー側で分岐する。
enum ValidateRenderOutcome {
    /// ディレクトリモード等で対象が決まらない。
    NoTarget,
    /// 解決・読込・描画すべてが成功した。`UpdateMessage` は file stamp 済み。
    Rendered(ResolvedTarget, UpdateMessage),
    /// 解決時にエラーが発生した（読込は未実行）。
    ResolveFailed(ResolveFileError),
    /// 解決は成功したが読込・描画に失敗した。
    ReadFailed(ResolvedTarget, ReadMarkdownError),
}

/// 解決済みターゲットを受け取って読込＋描画＋file stamp を行い、統一結果を返す。
///
/// validate 部分は呼び出し側の resolver（`resolve_single_file_target` 等）が担い、
/// ヘルパーは後段の read/render と stamp、outcome 分岐のみを担う。
async fn validate_and_render(
    resolve_result: Result<Option<ResolvedTarget>, ResolveFileError>,
) -> ValidateRenderOutcome {
    let target = match resolve_result {
        Ok(Some(target)) => target,
        Ok(None) => return ValidateRenderOutcome::NoTarget,
        Err(error) => return ValidateRenderOutcome::ResolveFailed(error),
    };
    match read_and_render_target(&target).await {
        Ok(update) => {
            let stamped = target.attach_file_info(update);
            ValidateRenderOutcome::Rendered(target, stamped)
        }
        Err(error) => ValidateRenderOutcome::ReadFailed(target, error),
    }
}

/// WebSocket接続時の初期コンテンツを取得する。
///
/// 単一ファイルモード: ファイルを読み込みSome(UpdateMessage)を返す。
/// ディレクトリモード: Noneを返す（初期コンテンツなし）。
pub(in crate::server) async fn load_initial_socket_update(
    state: &AppState,
) -> Result<Option<UpdateMessage>, SocketInitError> {
    let resolve_result =
        resolve_single_file_target_blocking(state, "WebSocket初期ターゲットの相対パス算出失敗")
            .await;
    match validate_and_render(resolve_result).await {
        ValidateRenderOutcome::NoTarget => Ok(None),
        ValidateRenderOutcome::Rendered(_, update) => Ok(Some(update)),
        ValidateRenderOutcome::ResolveFailed(error) => Err(map_socket_validation_error(error)),
        ValidateRenderOutcome::ReadFailed(_, error) => {
            tracing::warn!("[markdown-view] WebSocket初期読み込みエラー: {}", error);
            Err(SocketInitError::new(
                error.close_code(),
                error.user_message(),
            ))
        }
    }
}

/// WebSocketクライアント遅延時の回復メッセージを生成する。
///
/// 単一ファイルモード: ファイルを再読み込みして本文更新とメモ再取得通知を返す。
/// ディレクトリモード: Refreshを返す（クライアント側で再取得させる）。
pub(in crate::server) async fn build_lagged_recovery_message(state: &AppState) -> BroadcastMessage {
    let resolve_result =
        resolve_single_file_target_blocking(state, "WebSocket再送信ターゲットの相対パス算出失敗")
            .await;
    match validate_and_render(resolve_result).await {
        ValidateRenderOutcome::NoTarget => BroadcastMessage::Refresh,
        ValidateRenderOutcome::Rendered(target, update) => {
            let relative = target.relative_path().map(ToOwned::to_owned);
            BroadcastMessage::LaggedRecovery(LaggedRecoveryMessage::new(update, relative))
        }
        ValidateRenderOutcome::ResolveFailed(error) => {
            tracing::warn!(
                "[markdown-view] WebSocket再送信時のファイル検証失敗: {}",
                error
            );
            BroadcastMessage::Error(format!("ファイル検証エラー: {}", error.user_message()))
        }
        ValidateRenderOutcome::ReadFailed(target, error) => {
            tracing::warn!(
                "[markdown-view] WebSocket再送信読み込みエラー ({}): {}",
                target.file_label(),
                error
            );
            BroadcastMessage::Error(format!(
                "ファイル読み込みエラー ({}): {}",
                target.file_label(),
                error.user_message()
            ))
        }
    }
}

/// ファイル変更イベントからブロードキャスト用メッセージを生成する。
///
/// Noneを返した場合、ブロードキャストをスキップすべきことを示す。
/// これは、対象なし、ディレクトリモードの削除・rename中の一時不在、
/// またはwatcher由来の無効なpathをブラウザへ通知しない場合に発生する。
///
/// 単一ファイルモードの `NotFound` と、`NotFile` / `Traversal` / `Hidden` /
/// `NotMarkdown` / `Io` / `InternalState` などの検証失敗は、セキュリティ境界の拒否、
/// 一時不在ではない異常、または内部不整合としてError broadcastにする。
pub(in crate::server) async fn build_change_broadcast_message(
    state: &AppState,
    changed_file: &Path,
) -> Option<BroadcastMessage> {
    let resolve_result = resolve_change_target_blocking(state, changed_file).await;
    match validate_and_render(resolve_result).await {
        ValidateRenderOutcome::NoTarget => None,
        ValidateRenderOutcome::Rendered(_, update) => Some(BroadcastMessage::Update(update)),
        ValidateRenderOutcome::ResolveFailed(ResolveFileError::NotFound)
            if should_skip_not_found_change(state) =>
        {
            let file_label = change_error_file_label(state, changed_file);
            tracing::debug!(
                "[markdown-view] 更新対象が削除または一時不在のためbroadcastをスキップ: {}",
                file_label
            );
            None
        }
        ValidateRenderOutcome::ResolveFailed(ResolveFileError::NotFound) => {
            let file_label = change_error_file_label(state, changed_file);
            let error = ResolveFileError::NotFound;
            tracing::warn!(
                "[markdown-view] 更新時ファイル検証失敗 ({}): {}",
                file_label,
                error
            );
            Some(BroadcastMessage::Error(format!(
                "ファイル検証エラー ({}): {}",
                file_label,
                error.user_message()
            )))
        }
        ValidateRenderOutcome::ResolveFailed(ResolveFileError::InvalidPath) => {
            let file_label = change_error_file_label(state, changed_file);
            tracing::error!(
                "[markdown-view] watcher由来の無効な更新pathを検出したためbroadcastをスキップ: {}",
                file_label
            );
            None
        }
        ValidateRenderOutcome::ResolveFailed(ResolveFileError::InternalState) => {
            let file_label = change_error_file_label(state, changed_file);
            let error = ResolveFileError::InternalState;
            tracing::error!(
                "[markdown-view] 更新時ファイル検証失敗 ({}): {}",
                file_label,
                error
            );
            Some(BroadcastMessage::Error(format!(
                "ファイル検証エラー ({}): {}",
                file_label,
                error.user_message()
            )))
        }
        ValidateRenderOutcome::ResolveFailed(
            error @ (ResolveFileError::EmptyPath
            | ResolveFileError::NotFile
            | ResolveFileError::Traversal
            | ResolveFileError::NotMarkdown
            | ResolveFileError::Hidden
            | ResolveFileError::Io(_)),
        ) => {
            let file_label = change_error_file_label(state, changed_file);
            tracing::warn!(
                "[markdown-view] 更新時ファイル検証失敗 ({}): {}",
                file_label,
                error
            );
            Some(BroadcastMessage::Error(format!(
                "ファイル検証エラー ({}): {}",
                file_label,
                error.user_message()
            )))
        }
        ValidateRenderOutcome::ReadFailed(target, error) => {
            tracing::warn!(
                "[markdown-view] 更新時読み込みエラー ({}): {}",
                target.file_label(),
                error
            );
            Some(BroadcastMessage::Error(format!(
                "ファイル読み込みエラー ({}): {}",
                target.file_label(),
                error.user_message()
            )))
        }
    }
}

fn should_skip_not_found_change(state: &AppState) -> bool {
    state.mode().is_directory()
}

/// 変更通知の検証失敗ログに使うファイル表示名を返す。
fn change_error_file_label(state: &AppState, changed_file: &Path) -> String {
    state
        .mode()
        .single_file()
        .map(file_display_name)
        .unwrap_or_else(|| {
            sanitize_path_for_logging(changed_file, state.mode().base_dir()).into_owned()
        })
}

/// 本文読込や描画の前に、存在・サイズ・open可否だけを確認する。
///
/// 解決済みhandleがある場合はcurrent pathを再openせず、そのhandleのmetadataだけを
/// 検証する。handleがないディレクトリモードのフォールバック経路だけpathをopenする。
pub(super) async fn check_readable_before_render(
    target: &ResolvedTarget,
) -> Result<(), ReadMarkdownError> {
    if let Some(file) = target.read_file().map_err(ReadMarkdownError::Io)? {
        let file = tokio::fs::File::from_std(file);
        let metadata = file.metadata().await.map_err(ReadMarkdownError::Io)?;
        return validate_readable_metadata(metadata);
    }

    let file_path = target.file_path();
    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(ReadMarkdownError::Io)?;
    validate_readable_metadata(metadata)?;

    // 読み込み本体は避けつつ、権限やロックなどでopenできない状態を検出する。
    let _file = tokio::fs::File::open(file_path)
        .await
        .map_err(ReadMarkdownError::Io)?;
    Ok(())
}

fn validate_readable_metadata(metadata: std::fs::Metadata) -> Result<(), ReadMarkdownError> {
    if !metadata.is_file() {
        return Err(ReadMarkdownError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "通常ファイルではありません",
        )));
    }
    if metadata.len() > MAX_FILE_SIZE {
        return Err(ReadMarkdownError::TooLarge);
    }
    Ok(())
}

/// WebSocket受信者がいない変更イベントで、ローカルログに残すエラー文言を組み立てる。
pub(in crate::server) async fn build_change_error_log_message_without_receivers(
    state: &AppState,
    changed_file: &Path,
) -> Option<String> {
    let resolve_result = resolve_change_target_blocking(state, changed_file).await;
    let target = match resolve_result {
        Ok(Some(target)) => target,
        Ok(None) => return None,
        Err(ResolveFileError::NotFound) if should_skip_not_found_change(state) => return None,
        Err(error) => {
            let file_label = change_error_file_label(state, changed_file);
            return Some(format!(
                "更新時ファイル検証失敗 ({}): {}",
                file_label, error
            ));
        }
    };

    match check_readable_before_render(&target).await {
        Ok(()) => None,
        Err(ReadMarkdownError::NotUtf8) => None,
        Err(error) => Some(format!(
            "更新時読み込みエラー ({}): {}",
            target.file_label(),
            error
        )),
    }
}

pub(super) fn map_socket_validation_error(error: ResolveFileError) -> SocketInitError {
    if matches!(error, ResolveFileError::InternalState) {
        tracing::error!(
            "[markdown-view] WebSocket初期ファイル検証で内部状態不整合を検出: {}",
            error
        );
        return SocketInitError::new(1011, "内部エラーが発生しました");
    }

    tracing::warn!("[markdown-view] WebSocket初期ファイル検証失敗: {}", error);
    SocketInitError::new(
        1008,
        format!("ファイル検証に失敗しました: {}", error.user_message()),
    )
}

#[derive(Debug)]
pub(super) enum ReadMarkdownError {
    Io(std::io::Error),
    TooLarge,
    NotUtf8,
}

impl std::fmt::Display for ReadMarkdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadMarkdownError::Io(error) => write!(f, "I/Oエラー: {}", error),
            ReadMarkdownError::TooLarge => write!(f, "{}", FILE_SIZE_LIMIT_ERROR_MESSAGE),
            ReadMarkdownError::NotUtf8 => write!(f, "ファイルがUTF-8テキストではありません"),
        }
    }
}

impl std::error::Error for ReadMarkdownError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ReadMarkdownError::Io(error) => Some(error),
            ReadMarkdownError::TooLarge | ReadMarkdownError::NotUtf8 => None,
        }
    }
}

impl ReadMarkdownError {
    pub(super) fn status_code(&self) -> StatusCode {
        match self {
            ReadMarkdownError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ReadMarkdownError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ReadMarkdownError::NotUtf8 => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    pub(super) fn close_code(&self) -> u16 {
        match self {
            ReadMarkdownError::Io(_) => 1011,
            ReadMarkdownError::TooLarge => 1009,
            ReadMarkdownError::NotUtf8 => 1003,
        }
    }

    pub(super) fn user_message(&self) -> String {
        match self {
            ReadMarkdownError::Io(_) => "ファイルの読み込みに失敗しました".to_string(),
            ReadMarkdownError::TooLarge => FILE_SIZE_LIMIT_ERROR_MESSAGE.to_string(),
            ReadMarkdownError::NotUtf8 => "このファイルはUTF-8テキストではありません".to_string(),
        }
    }
}

impl IntoResponse for ReadMarkdownError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status_code();
        let body = Json(error_message_json(self.user_message()));
        (status, body).into_response()
    }
}

/// Markdownファイルを読み込む（TOCTOU対策として二段階サイズチェック）
///
/// 1. `metadata().len()` で事前チェック（競合状態の大部分を防止）
/// 2. `AsyncReadExt::take()` で実読み取り量を制限（TOCTOU回避の最終防衛）
pub(super) async fn read_markdown_with_limit(
    file_path: &Path,
) -> Result<String, ReadMarkdownError> {
    notify_content_before_read_for_test(file_path);
    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(ReadMarkdownError::Io)?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err(ReadMarkdownError::TooLarge);
    }

    let file = tokio::fs::File::open(file_path)
        .await
        .map_err(ReadMarkdownError::Io)?;
    let buffer = read_bytes_with_limit(file).await?;

    markdown_from_utf8(buffer)
}

async fn read_markdown_from_open_file(
    file_path: &Path,
    file: std::fs::File,
) -> Result<String, ReadMarkdownError> {
    notify_content_before_read_for_test(file_path);
    let file = tokio::fs::File::from_std(file);
    let metadata = file.metadata().await.map_err(ReadMarkdownError::Io)?;
    validate_readable_metadata(metadata)?;
    let buffer = read_bytes_with_limit(file).await?;
    markdown_from_utf8(buffer)
}

fn markdown_from_utf8(buffer: Vec<u8>) -> Result<String, ReadMarkdownError> {
    String::from_utf8(buffer).map_err(|error| {
        tracing::warn!(
            "[markdown-view] UTF-8デコード失敗: バイトオフセット {} で無効なバイト列",
            error.utf8_error().valid_up_to()
        );
        ReadMarkdownError::NotUtf8
    })
}

pub(super) async fn read_bytes_with_limit(
    file: tokio::fs::File,
) -> Result<Vec<u8>, ReadMarkdownError> {
    let mut limited_reader = file.take(MAX_FILE_SIZE + 1);
    let mut buffer = Vec::new();
    limited_reader
        .read_to_end(&mut buffer)
        .await
        .map_err(ReadMarkdownError::Io)?;
    if buffer.len() as u64 > MAX_FILE_SIZE {
        return Err(ReadMarkdownError::TooLarge);
    }
    Ok(buffer)
}

async fn read_and_render_target(
    target: &ResolvedTarget,
) -> Result<UpdateMessage, ReadMarkdownError> {
    let markdown = match target.read_file().map_err(ReadMarkdownError::Io)? {
        Some(file) => read_markdown_from_open_file(target.file_path(), file).await?,
        None => read_markdown_with_limit(target.file_path()).await?,
    };
    render_markdown_update(&markdown)
}

fn render_markdown_update(markdown: &str) -> Result<UpdateMessage, ReadMarkdownError> {
    let document = render_document(markdown);
    Ok(UpdateMessage::new(document.content, document.toc, None))
}
