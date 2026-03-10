use std::path::Path;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use tokio::io::AsyncReadExt;

use super::resolve::{
    resolve_change_target, resolve_single_file_target, ResolveFileError, ResolvedTarget,
    RouteTargetRequest,
};
use crate::renderer::render_markdown;
use crate::server::messages::{ApiError, BroadcastMessage};
use crate::server::state::AppState;
use crate::template::{error_message_json, UpdateMessage};
use crate::toc::generate_toc;

/// ファイルサイズ上限: OOM防止
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
const FILE_SIZE_LIMIT_ERROR_MESSAGE: &str = "ファイルサイズが上限（10MB）を超えています";

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
    read_and_render_file(target.file_path())
        .await
        .map(|update| target.update(update))
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

/// WebSocket接続時の初期コンテンツを取得する。
///
/// 単一ファイルモード: ファイルを読み込みSome(UpdateMessage)を返す。
/// ディレクトリモード: Noneを返す（初期コンテンツなし）。
pub(in crate::server) async fn load_initial_socket_update(
    state: &AppState,
) -> Result<Option<UpdateMessage>, SocketInitError> {
    let Some(target) =
        resolve_single_file_target(state, "WebSocket初期ターゲットの相対パス算出失敗")
            .map_err(map_socket_validation_error)?
    else {
        return Ok(None);
    };

    let update = read_and_render_file(target.file_path())
        .await
        .map_err(|error| {
            tracing::warn!("[markdown-view] WebSocket初期読み込みエラー: {}", error);
            SocketInitError::new(error.close_code(), error.user_message())
        })?;
    Ok(Some(target.update(update)))
}

/// WebSocketクライアント遅延時の回復メッセージを生成する。
///
/// 単一ファイルモード: ファイルを再読み込みしてUpdateを返す。
/// ディレクトリモード: Refreshを返す（クライアント側で再取得させる）。
pub(in crate::server) async fn build_lagged_recovery_message(state: &AppState) -> BroadcastMessage {
    let target =
        match resolve_single_file_target(state, "WebSocket再送信ターゲットの相対パス算出失敗")
        {
            Ok(Some(target)) => target,
            Ok(None) => return BroadcastMessage::Refresh,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] WebSocket再送信時のファイル検証失敗: {}",
                    error
                );
                return BroadcastMessage::Error(format!("ファイル検証エラー: {}", error));
            }
        };

    match read_and_render_file(target.file_path()).await {
        Ok(update) => BroadcastMessage::Update(target.update(update)),
        Err(error) => {
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
/// Noneを返した場合、ブロードキャストをスキップすべきことを示す
/// （ディレクトリモードで相対パスが算出できない場合）。
pub(in crate::server) async fn build_change_broadcast_message(
    state: &AppState,
    changed_file: &Path,
) -> Option<BroadcastMessage> {
    let target = match resolve_change_target(state, changed_file) {
        Ok(Some(target)) => target,
        Ok(None) => return None,
        Err(error) => {
            let file_label = state
                .mode()
                .single_file()
                .map(display_name)
                .unwrap_or_else(|| changed_file.display().to_string());
            tracing::warn!(
                "[markdown-view] 更新時ファイル検証失敗 ({}): {}",
                file_label,
                error
            );
            return Some(BroadcastMessage::Error(format!(
                "ファイル検証エラー ({}): {}",
                file_label, error
            )));
        }
    };

    Some(match read_and_render_file(target.file_path()).await {
        Ok(update) => BroadcastMessage::Update(target.update(update)),
        Err(error) => {
            tracing::warn!(
                "[markdown-view] 更新時読み込みエラー ({}): {}",
                target.file_label(),
                error
            );
            BroadcastMessage::Error(format!(
                "ファイル読み込みエラー ({}): {}",
                target.file_label(),
                error.user_message()
            ))
        }
    })
}

fn map_socket_validation_error(error: ResolveFileError) -> SocketInitError {
    tracing::warn!("[markdown-view] WebSocket初期ファイル検証失敗: {}", error);
    SocketInitError::new(1008, format!("ファイル検証に失敗しました: {}", error))
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
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
async fn read_markdown_with_limit(file_path: &Path) -> Result<String, ReadMarkdownError> {
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

/// ファイルを読み込み、Markdown→HTML変換とTOC生成を行いUpdateMessageとして返す
async fn read_and_render_file(file_path: &Path) -> Result<UpdateMessage, ReadMarkdownError> {
    let markdown = read_markdown_with_limit(file_path).await?;
    Ok(UpdateMessage::new(
        render_markdown(&markdown),
        generate_toc(&markdown),
        None,
    ))
}
