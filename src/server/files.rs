//! Markdownファイルの探索、検証、読み込み、描画を管理する。

use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use tokio::io::AsyncReadExt;

use super::guards::json_error;
use super::messages::{ApiError, BroadcastMessage};
use super::state::AppState;
use crate::renderer::render_markdown;
use crate::template::{error_message_json, UpdateMessage};
use crate::toc::generate_toc;

/// ファイルサイズ上限: OOM防止
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
const FILE_SIZE_LIMIT_ERROR_MESSAGE: &str = "ファイルサイズが上限（10MB）を超えています";

/// ファイル一覧の最大件数
const MAX_FILE_LIST: usize = 1000;

/// ディレクトリ走査の最大深度（スタックオーバーフロー防止）
const MAX_DIR_DEPTH: usize = 32;

#[derive(Debug, Clone)]
/// ファイル解決結果。ターゲットファイルのパス、ファイル一覧、相対パス、表示用ラベルを保持する。
pub(super) struct ResolvedTarget {
    file_path: PathBuf,
    file_list: Option<Vec<String>>,
    relative_path: Option<String>,
    file_label: String,
}

impl ResolvedTarget {
    fn new(
        file_path: PathBuf,
        file_list: Option<Vec<String>>,
        relative_path: Option<String>,
    ) -> Self {
        let file_label = relative_path
            .clone()
            .or_else(|| {
                file_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| file_path.display().to_string());
        Self {
            file_path,
            file_list,
            relative_path,
            file_label,
        }
    }

    pub(super) fn file_path(&self) -> &Path {
        &self.file_path
    }

    pub(super) fn file_list(&self) -> Option<&[String]> {
        self.file_list.as_deref()
    }

    pub(super) fn relative_path(&self) -> Option<&str> {
        self.relative_path.as_deref()
    }

    fn update(&self, update: UpdateMessage) -> UpdateMessage {
        update.with_file(self.relative_path.clone())
    }
}

#[derive(Debug, Clone)]
/// WebSocket初期化時のエラー。closeフレームのコードと理由を保持する。
pub(super) struct SocketInitError {
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

    pub(super) fn close_code(&self) -> u16 {
        self.close_code
    }

    pub(super) fn reason(&self) -> &str {
        &self.reason
    }
}

/// 対象ファイル解決エラーをエンドポイント文脈に応じたAPIエラーへ変換する。
pub(super) fn resolve_target_file_or_error(
    state: &AppState,
    query_file: Option<&str>,
    include_file_list: bool,
    not_found_message: &'static str,
) -> Result<ResolvedTarget, ApiError> {
    let (file_path, file_list) =
        resolve_target_file(state, query_file, include_file_list).map_err(|status| {
        let msg = match status {
            StatusCode::NOT_FOUND => not_found_message,
            StatusCode::INTERNAL_SERVER_ERROR => "ファイル一覧の取得に失敗しました",
            other => {
                tracing::warn!(
                    "[markdown-view] 予期しないファイル解決ステータスを検出: not_found_message={}, status={}",
                    not_found_message,
                    other
                );
                "ファイル解決に失敗しました"
            }
        };
        json_error(status, msg)
    })?;
    Ok(build_resolved_target(
        state,
        file_path,
        file_list,
        "ターゲットファイルの相対パス算出失敗",
    ))
}

/// Markdownの読み込みと描画を行い、失敗時はAPI応答用のエラーへ変換する。
pub(super) async fn read_rendered_update_or_error(
    target: &ResolvedTarget,
    read_error_log_label: &'static str,
) -> Result<UpdateMessage, ApiError> {
    read_and_render_file(target.file_path())
        .await
        .map(|update| target.update(update))
        .map_err(|e| {
            tracing::warn!(
                "[markdown-view] {}読み込みエラー ({}): {}",
                read_error_log_label,
                target.file_label,
                e
            );
            json_error(e.status_code(), e.user_message())
        })
}

/// WebSocket接続時の初期コンテンツを取得する。
///
/// 単一ファイルモード: ファイルを読み込みSome(UpdateMessage)を返す。
/// ディレクトリモード: Noneを返す（初期コンテンツなし）。
pub(super) async fn initial_socket_update(
    state: &AppState,
) -> Result<Option<UpdateMessage>, SocketInitError> {
    let Some(file_path) = state.mode().single_file() else {
        return Ok(None);
    };

    let validated_path = revalidate_single_file_target(file_path).map_err(|e| {
        tracing::warn!("[markdown-view] WebSocket初期ファイル検証失敗: {}", e);
        SocketInitError::new(1008, format!("ファイル検証に失敗しました: {}", e))
    })?;
    let target = build_resolved_target(
        state,
        validated_path,
        None,
        "WebSocket初期ターゲットの相対パス算出失敗",
    );
    let update = read_and_render_file(target.file_path())
        .await
        .map_err(|e| {
            tracing::warn!("[markdown-view] WebSocket初期読み込みエラー: {}", e);
            SocketInitError::new(e.close_code(), e.user_message())
        })?;
    Ok(Some(target.update(update)))
}

/// WebSocketクライアント遅延時の回復メッセージを生成する。
///
/// 単一ファイルモード: ファイルを再読み込みしてUpdateを返す。
/// ディレクトリモード: Refreshを返す（クライアント側で再取得させる）。
pub(super) async fn lagged_recovery_broadcast_message(state: &AppState) -> BroadcastMessage {
    let Some(file_path) = state.mode().single_file() else {
        return BroadcastMessage::Refresh;
    };

    let validated_path = match revalidate_single_file_target(file_path) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!("[markdown-view] WebSocket再送信時のファイル検証失敗: {}", e);
            return BroadcastMessage::Error(format!("ファイル検証エラー: {}", e));
        }
    };
    let target = build_resolved_target(
        state,
        validated_path,
        None,
        "WebSocket再送信ターゲットの相対パス算出失敗",
    );
    match read_and_render_file(target.file_path()).await {
        Ok(update) => BroadcastMessage::Update(target.update(update)),
        Err(e) => {
            tracing::warn!(
                "[markdown-view] WebSocket再送信読み込みエラー ({}): {}",
                target.file_label,
                e
            );
            BroadcastMessage::Error(format!(
                "ファイル読み込みエラー ({}): {}",
                target.file_label,
                e.user_message()
            ))
        }
    }
}

/// ファイル変更イベントからブロードキャスト用メッセージを生成する。
///
/// Noneを返した場合、ブロードキャストをスキップすべきことを示す
/// （ディレクトリモードで相対パスが算出できない場合）。
pub(super) async fn update_broadcast_message(
    state: &AppState,
    changed_file: &Path,
) -> Option<BroadcastMessage> {
    let target = if let Some(expected) = state.mode().single_file() {
        if let Err(e) = revalidate_single_file_target(expected) {
            let file_label = expected
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| expected.display().to_string());
            tracing::warn!(
                "[markdown-view] 更新時ファイル検証失敗 ({}): {}",
                file_label,
                e
            );
            return Some(BroadcastMessage::Error(format!(
                "ファイル検証エラー ({}): {}",
                file_label, e
            )));
        }
        build_resolved_target(
            state,
            changed_file.to_path_buf(),
            None,
            "更新対象の相対パス算出失敗",
        )
    } else {
        match build_update_target(state, changed_file) {
            Some(target) => target,
            None => return None,
        }
    };

    Some(match read_and_render_file(target.file_path()).await {
        Ok(update) => BroadcastMessage::Update(target.update(update)),
        Err(e) => {
            tracing::warn!(
                "[markdown-view] 更新時読み込みエラー ({}): {}",
                target.file_label,
                e
            );
            BroadcastMessage::Error(format!(
                "ファイル読み込みエラー ({}): {}",
                target.file_label,
                e.user_message()
            ))
        }
    })
}

/// モードとクエリパラメータからターゲットファイルを解決する
///
/// ディレクトリモード: クエリ指定があればresolve_file、なければデフォルトファイル
/// 単一ファイルモード: クエリ無視でファイルを返す
pub(super) fn resolve_target_file(
    state: &AppState,
    query_file: Option<&str>,
    include_file_list: bool,
) -> Result<(PathBuf, Option<Vec<String>>), StatusCode> {
    if let Some(path) = state.mode().single_file() {
        let canonical = revalidate_single_file_target(path).map_err(|e| {
            tracing::warn!("[markdown-view] 単一ファイル解決エラー: {}", e);
            e.status_code()
        })?;
        Ok((canonical, None))
    } else if let Some(base) = state.mode().directory() {
        let mut precomputed_files: Option<Vec<String>> = None;
        let file_path = if let Some(rel) = query_file {
            resolve_file(base, rel).map_err(|e| {
                tracing::warn!("[markdown-view] ファイル解決エラー: {}", e);
                e.status_code()
            })?
        } else {
            let files = list_markdown_files(base).map_err(|e| {
                tracing::warn!("[markdown-view] ファイル一覧取得エラー: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            precomputed_files = Some(files.clone());
            let default_file = files
                .iter()
                .find(|f| f.eq_ignore_ascii_case("readme.md"))
                .or_else(|| files.first());

            match default_file {
                Some(rel) => resolve_file(base, rel).map_err(|e| {
                    tracing::warn!("[markdown-view] デフォルトファイル解決エラー: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?,
                None => {
                    return Err(StatusCode::NOT_FOUND);
                }
            }
        };

        let file_list = if include_file_list {
            match precomputed_files {
                Some(files) => Some(files),
                None => Some(list_markdown_files(base).map_err(|e| {
                    tracing::warn!("[markdown-view] ファイル一覧取得エラー: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?),
            }
        } else {
            None
        };

        Ok((file_path, file_list))
    } else {
        tracing::error!("[markdown-view] 未知のAppModeです");
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

fn build_resolved_target(
    state: &AppState,
    file_path: PathBuf,
    file_list: Option<Vec<String>>,
    warn_label: &'static str,
) -> ResolvedTarget {
    let relative_path = state.mode().relative_path_of(&file_path);
    if state.mode().is_directory() && relative_path.is_none() {
        tracing::warn!(
            "[markdown-view] {}: {} はベース {} の配下ではありません",
            warn_label,
            file_path.display(),
            state.mode().base_dir().display()
        );
        // これは意図的な graceful degradation であり、relative_path が None でもページ描画自体は継続できる。
        // HTTP経路(resolve_target_file_or_error経由)ではサイドバーのファイルハイライトだけが効かなくなり、
        // コンテンツ表示そのものには影響しない。
        // 一方で WebSocket経路(build_update_target経由)では relative_path が None の場合に None を返し、
        // 当該更新のブロードキャストをスキップする対策を既に入れている。
    }
    ResolvedTarget::new(file_path, file_list, relative_path)
}

fn build_update_target(state: &AppState, changed_file: &Path) -> Option<ResolvedTarget> {
    let target = build_resolved_target(
        state,
        changed_file.to_path_buf(),
        None,
        "相対パス算出失敗のためブロードキャストをスキップ",
    );
    if state.mode().is_directory() && target.relative_path.is_none() {
        return None;
    }
    Some(target)
}

/// ディレクトリ内の.mdファイルを再帰的に列挙する
pub fn list_markdown_files(base_dir: &Path) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();
    let mut visited_dirs = std::collections::HashSet::new();
    let canonical_base = base_dir.canonicalize()?;
    visited_dirs.insert(canonical_base);
    list_markdown_files_recursive(base_dir, base_dir, &mut files, &mut visited_dirs, 0)?;
    files.sort();
    files.truncate(MAX_FILE_LIST);
    Ok(files)
}

fn list_markdown_files_recursive(
    base_dir: &Path,
    current_dir: &Path,
    files: &mut Vec<String>,
    visited_dirs: &mut std::collections::HashSet<PathBuf>,
    depth: usize,
) -> std::io::Result<()> {
    if depth >= MAX_DIR_DEPTH {
        tracing::warn!(
            "[markdown-view] ディレクトリ深度上限に到達（スキップ）: {}",
            current_dir.display()
        );
        return Ok(());
    }
    let entries = std::fs::read_dir(current_dir)?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    "[markdown-view] ディレクトリエントリ読み取りエラー（スキップ）: {} ({})",
                    current_dir.display(),
                    e
                );
                continue;
            }
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                tracing::warn!(
                    "[markdown-view] ファイルタイプ取得エラー（スキップ）: {} ({})",
                    path.display(),
                    e
                );
                continue;
            }
        };

        if file_type.is_dir() || (file_type.is_symlink() && path.is_dir()) {
            if files.len() >= MAX_FILE_LIST {
                return Ok(());
            }
            if file_type.is_symlink() {
                let Some(resolved) = canonicalize_dir_for_cycle(&path, "シンボリックリンク")
                else {
                    continue;
                };
                let canonical_base = match base_dir.canonicalize() {
                    Ok(cb) => cb,
                    Err(e) => {
                        tracing::warn!(
                            "[markdown-view] ベースディレクトリの正規化に失敗（スキップ）: {} ({})",
                            base_dir.display(),
                            e
                        );
                        continue;
                    }
                };
                if !resolved.starts_with(&canonical_base) {
                    tracing::warn!(
                        "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
                        path.display(),
                        resolved.display()
                    );
                    continue;
                }
                if !visited_dirs.insert(resolved) {
                    tracing::warn!(
                        "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                        path.display()
                    );
                    continue;
                }
            } else {
                let Some(canonical) = canonicalize_dir_for_cycle(&path, "通常ディレクトリ")
                else {
                    continue;
                };
                if !visited_dirs.insert(canonical) {
                    continue;
                }
            }
            list_markdown_files_recursive(base_dir, &path, files, visited_dirs, depth + 1)?;
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            match path.strip_prefix(base_dir) {
                Ok(relative) => {
                    let relative_str = relative
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    files.push(relative_str);
                    if files.len() >= MAX_FILE_LIST {
                        return Ok(());
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        "[markdown-view] 相対パス算出不可（スキップ）: {} (ベース: {})",
                        path.display(),
                        base_dir.display()
                    );
                }
            }
        }
    }
    Ok(())
}

pub(super) fn canonicalize_dir_for_cycle(path: &Path, label: &str) -> Option<PathBuf> {
    match path.canonicalize() {
        Ok(canonical) => Some(canonical),
        Err(e) => {
            tracing::warn!(
                "[markdown-view] {}の正規化に失敗（スキップ）: {} ({})",
                label,
                path.display(),
                e
            );
            None
        }
    }
}

/// 相対パスを安全に解決する（ディレクトリトラバーサル防止）
pub fn resolve_file(base_dir: &Path, relative: &str) -> Result<PathBuf, ResolveFileError> {
    if relative.is_empty() {
        return Err(ResolveFileError::EmptyPath);
    }

    if relative.contains('\0') {
        return Err(ResolveFileError::InvalidPath);
    }

    let rel_path = Path::new(relative);
    if rel_path.is_absolute() {
        return Err(ResolveFileError::InvalidPath);
    }

    let candidate = base_dir.join(rel_path);
    let canonical = candidate.canonicalize().map_err(|e| {
        tracing::warn!(
            "[markdown-view] ファイルパス正規化失敗: {} ({})",
            candidate.display(),
            e
        );
        ResolveFileError::NotFound
    })?;

    let canonical_base = base_dir.canonicalize().map_err(|e| {
        tracing::warn!(
            "[markdown-view] ベースディレクトリ正規化失敗: {} ({})",
            base_dir.display(),
            e
        );
        ResolveFileError::NotFound
    })?;
    if !canonical.starts_with(&canonical_base) {
        return Err(ResolveFileError::Traversal);
    }

    if let Ok(resolved_relative) = canonical.strip_prefix(&canonical_base) {
        if resolved_relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        {
            return Err(ResolveFileError::Hidden);
        }
    }

    if !canonical.is_file() {
        return Err(ResolveFileError::NotFound);
    }

    match canonical.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("md") => {}
        _ => return Err(ResolveFileError::NotMarkdown),
    }

    Ok(canonical)
}

/// 単一ファイルモードの対象ファイルを安全に再検証する
///
/// 起動時に正規化したパスと現在のパスを比較し、シンボリックリンク差し替え等の
/// 攻撃を検出する。正規化後のパスが起動時と異なる場合はトラバーサルとして拒否する。
pub(super) fn revalidate_single_file_target(
    expected_path: &Path,
) -> Result<PathBuf, ResolveFileError> {
    let canonical = expected_path.canonicalize().map_err(|e| {
        tracing::warn!(
            "[markdown-view] 単一ファイルパス正規化失敗: {} ({})",
            expected_path.display(),
            e
        );
        ResolveFileError::NotFound
    })?;

    if canonical != expected_path {
        return Err(ResolveFileError::Traversal);
    }

    if !canonical.is_file() {
        return Err(ResolveFileError::NotFound);
    }

    match canonical.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("md") => Ok(canonical),
        _ => Err(ResolveFileError::NotMarkdown),
    }
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
            ReadMarkdownError::Io(e) => write!(f, "I/Oエラー: {}", e),
            ReadMarkdownError::TooLarge => write!(f, "{}", FILE_SIZE_LIMIT_ERROR_MESSAGE),
            ReadMarkdownError::NotUtf8 => write!(f, "ファイルがUTF-8テキストではありません"),
        }
    }
}

impl std::error::Error for ReadMarkdownError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ReadMarkdownError::Io(e) => Some(e),
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

#[derive(Debug, PartialEq)]
pub enum ResolveFileError {
    /// 空パス
    EmptyPath,
    /// 無効なパス（絶対パス、NULバイト等）
    InvalidPath,
    /// ファイルが見つからない
    NotFound,
    /// ディレクトリトラバーサル検出
    Traversal,
    /// Markdownファイルではない
    NotMarkdown,
    /// 隠しファイルへのアクセス
    Hidden,
}

impl std::fmt::Display for ResolveFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveFileError::EmptyPath => write!(f, "ファイルパスが空です"),
            ResolveFileError::InvalidPath => write!(f, "無効なパスです"),
            ResolveFileError::NotFound => write!(f, "ファイルが見つかりません"),
            ResolveFileError::Traversal => {
                write!(f, "ディレクトリ外へのアクセスは禁止されています")
            }
            ResolveFileError::NotMarkdown => write!(f, ".mdファイルのみアクセス可能です"),
            ResolveFileError::Hidden => {
                write!(f, "隠しファイルへのアクセスは禁止されています")
            }
        }
    }
}

impl std::error::Error for ResolveFileError {}

impl ResolveFileError {
    /// エラー種別に関わらず404を返す
    ///
    /// エラー種別で応答を分けるとファイル存在有無の推測材料になるため、
    /// すべて404に統一してセキュリティを確保する。
    pub fn status_code(&self) -> StatusCode {
        StatusCode::NOT_FOUND
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

    String::from_utf8(buffer).map_err(|e| {
        tracing::warn!(
            "[markdown-view] UTF-8デコード失敗: バイトオフセット {} で無効なバイト列",
            e.utf8_error().valid_up_to()
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
pub(super) async fn read_and_render_file(
    file_path: &Path,
) -> Result<UpdateMessage, ReadMarkdownError> {
    let markdown = read_markdown_with_limit(file_path).await?;
    Ok(UpdateMessage::new(
        render_markdown(&markdown),
        generate_toc(&markdown),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use tokio::sync::broadcast;

    use super::*;
    use crate::server::{AppMode, AppState};

    #[test]
    fn test_close_code_ioエラーは1011を返す() {
        let err = ReadMarkdownError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, ""));
        assert_eq!(err.close_code(), 1011);
    }

    #[test]
    fn test_close_code_too_largeは1009を返す() {
        let err = ReadMarkdownError::TooLarge;
        assert_eq!(err.close_code(), 1009);
    }

    #[test]
    fn test_close_code_not_utf8は1003を返す() {
        let err = ReadMarkdownError::NotUtf8;
        assert_eq!(err.close_code(), 1003);
    }

    #[test]
    fn test_resolve_file_正常なパス() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "README.md");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("README.md"));
    }

    #[test]
    fn test_resolve_file_サブディレクトリのパス() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "docs/api.md");
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_file_トラバーサル拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "../../../etc/passwd");
        assert!(matches!(
            result,
            Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
        ));
    }

    #[test]
    fn test_resolve_file_バックスラッシュ型トラバーサルを拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "..\\..\\..\\etc\\passwd");
        assert!(matches!(
            result,
            Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
        ));
    }

    #[test]
    fn test_resolve_file_urlエンコード型トラバーサルを拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "docs/%2e%2e/%2e%2e/etc/passwd.md");
        assert!(matches!(
            result,
            Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
        ));
    }

    #[test]
    fn test_resolve_file_絶対パス拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "/etc/passwd");
        assert_eq!(result, Err(ResolveFileError::InvalidPath));
    }

    #[test]
    fn test_resolve_file_存在しないファイル() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "nonexistent.md");
        assert_eq!(result, Err(ResolveFileError::NotFound));
    }

    #[test]
    fn test_resolve_file_非md拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "notes.txt");
        assert_eq!(result, Err(ResolveFileError::NotMarkdown));
    }

    #[test]
    fn test_resolve_file_隠しファイル拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), ".hidden/secret.md");
        assert_eq!(result, Err(ResolveFileError::Hidden));
    }

    #[test]
    fn test_resolve_file_隠しドットファイル拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), ".dotfile.md");
        assert_eq!(result, Err(ResolveFileError::Hidden));
    }

    #[test]
    fn test_resolve_file_nulバイト拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "README\0.md");
        assert_eq!(result, Err(ResolveFileError::InvalidPath));
    }

    #[test]
    fn test_resolve_file_ディレクトリパス拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "docs");
        assert_eq!(result, Err(ResolveFileError::NotFound));
    }

    #[test]
    fn test_resolve_file_空パス拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "");
        assert_eq!(result, Err(ResolveFileError::EmptyPath));
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_file_シンボリックリンクによるトラバーサル拒否() {
        let dir = create_test_dir();
        let outside_dir = tempfile::tempdir().unwrap();
        std::fs::write(outside_dir.path().join("secret.md"), "# Secret").unwrap();

        std::os::unix::fs::symlink(
            outside_dir.path().join("secret.md"),
            dir.path().join("link.md"),
        )
        .unwrap();

        let result = resolve_file(dir.path(), "link.md");
        assert_eq!(result, Err(ResolveFileError::Traversal));
    }

    #[test]
    fn test_list_markdown_files_基本動作() {
        let dir = create_test_dir();
        let files = list_markdown_files(dir.path()).unwrap();
        assert!(files.contains(&"README.md".to_string()));
        assert!(files.contains(&"guide.md".to_string()));
        assert!(files.contains(&"docs/api.md".to_string()));
    }

    #[test]
    fn test_list_markdown_files_非md除外() {
        let dir = create_test_dir();
        let files = list_markdown_files(dir.path()).unwrap();
        assert!(!files.iter().any(|f| f.ends_with(".txt")));
    }

    #[test]
    fn test_list_markdown_files_隠しファイル除外() {
        let dir = create_test_dir();
        let files = list_markdown_files(dir.path()).unwrap();
        assert!(!files.iter().any(|f| f.contains(".hidden")));
        assert!(!files.iter().any(|f| f.starts_with('.')));
    }

    #[test]
    fn test_list_markdown_files_空ディレクトリ() {
        let dir = tempfile::tempdir().unwrap();
        let files = list_markdown_files(dir.path()).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_list_markdown_files_ソート済み() {
        let dir = create_test_dir();
        let files = list_markdown_files(dir.path()).unwrap();
        let mut sorted = files.clone();
        sorted.sort();
        assert_eq!(files, sorted);
    }

    #[test]
    fn test_list_markdown_files_最大1000件で打ち切る() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(MAX_FILE_LIST + 200) {
            let path = dir.path().join(format!("doc-{i:04}.md"));
            std::fs::write(path, "# x").unwrap();
        }

        let files = list_markdown_files(dir.path()).unwrap();
        assert_eq!(files.len(), MAX_FILE_LIST);
    }

    #[test]
    fn test_list_markdown_files_ベースディレクトリ正規化失敗はエラーを返す() {
        let missing = PathBuf::from("/path/that/does/not/exist");
        let result = list_markdown_files(&missing);
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_list_markdown_files_シンボリックリンクサイクルでハングしない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# README").unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/doc.md"), "# Doc").unwrap();

        std::os::unix::fs::symlink(dir.path(), dir.path().join("sub/loop")).unwrap();

        let files = list_markdown_files(dir.path()).unwrap();
        assert!(files.contains(&"README.md".to_string()));
        assert!(files.contains(&"sub/doc.md".to_string()));
        assert!(
            !files.iter().any(|f| f.contains("loop/")),
            "サイクル経由のエントリが含まれてはいけない: {:?}",
            files
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_list_markdown_files_自己参照シンボリックリンクでハングしない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# README").unwrap();

        std::os::unix::fs::symlink(".", dir.path().join("loop")).unwrap();

        let files = list_markdown_files(dir.path()).unwrap();
        assert!(files.contains(&"README.md".to_string()));
        assert!(
            !files.iter().any(|f| f.contains("loop/")),
            "サイクル経由のエントリが含まれてはいけない: {:?}",
            files
        );
    }

    #[test]
    fn test_list_markdown_files_深度上限を超えるパスは除外される() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("root.md"), "# root").unwrap();

        let mut current = dir.path().to_path_buf();
        for i in 0..=MAX_DIR_DEPTH {
            current = current.join(format!("d{}", i));
            std::fs::create_dir_all(&current).unwrap();
        }
        std::fs::write(current.join("deep.md"), "# deep").unwrap();

        let files = list_markdown_files(dir.path()).unwrap();
        assert!(files.contains(&"root.md".to_string()));
        assert!(!files.iter().any(|f| f.ends_with("deep.md")));
    }

    #[test]
    fn test_list_markdown_files_recursive_通常ディレクトリcanonicalize失敗時はスキップ扱い() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing-dir");
        assert!(canonicalize_dir_for_cycle(&missing, "通常ディレクトリ").is_none());
    }

    #[tokio::test]
    async fn test_read_bytes_with_limit_takeによる第2段階チェックで超過を検出する() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("large.md");
        tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
            .await
            .unwrap();

        let file = tokio::fs::File::open(&file_path).await.unwrap();
        let result = read_bytes_with_limit(file).await;
        assert!(matches!(result, Err(ReadMarkdownError::TooLarge)));
    }

    #[tokio::test]
    async fn test_read_markdown_error_into_response_too_largeのjson形式() {
        let response = ReadMarkdownError::TooLarge.into_response();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "error": FILE_SIZE_LIMIT_ERROR_MESSAGE
            })
        );
    }

    #[tokio::test]
    async fn test_read_markdown_error_into_response_ioのjson形式() {
        let io_error = std::io::Error::other("disk failure");
        let response = ReadMarkdownError::Io(io_error).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "error": "ファイルの読み込みに失敗しました"
            })
        );
    }

    #[tokio::test]
    async fn test_read_markdown_error_into_response_not_utf8のjson形式() {
        let response = ReadMarkdownError::NotUtf8.into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "error": "このファイルはUTF-8テキストではありません"
            })
        );
    }

    #[test]
    fn test_resolve_target_file_or_error_ディレクトリモードでrelative_pathとfile_listを返す() {
        let dir = create_test_dir();
        let state = create_directory_state(dir.path());

        let target = resolve_target_file_or_error(
            &state,
            Some("docs/api.md"),
            true,
            "表示可能なMarkdownファイルが見つかりません",
        )
        .unwrap();

        assert_eq!(target.relative_path(), Some("docs/api.md"));
        assert!(target.file_list().is_some());
        assert!(target.file_path().ends_with("docs/api.md"));
    }

    #[tokio::test]
    async fn test_initial_socket_update_単一ファイルモードでupdateを返す() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
        let state = create_single_file_state(&file_path);

        let update = initial_socket_update(&state).await.unwrap().unwrap();
        assert!(update.content().as_str().contains("title"));
    }

    #[tokio::test]
    async fn test_update_broadcast_message_ディレクトリモードでfileを含むupdateを返す() {
        let dir = create_test_dir();
        let state = create_directory_state(dir.path());
        let target = dir.path().join("docs/api.md");

        let message = update_broadcast_message(&state, &target).await.unwrap();
        match message {
            BroadcastMessage::Update(update) => {
                assert_eq!(update.file(), Some("docs/api.md"));
            }
            other => panic!("Updateを期待したが {:?} を受信", other),
        }
    }

    #[test]
    fn test_revalidate_single_file_target_正常なファイルを許可する() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let canonical = file_path.canonicalize().unwrap();
        let result = revalidate_single_file_target(&canonical);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), canonical);
    }

    #[test]
    fn test_revalidate_single_file_target_存在しないファイルはnotfoundを返す() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("nonexistent.md");
        let result = revalidate_single_file_target(&file_path);
        assert_eq!(result, Err(ResolveFileError::NotFound));
    }

    #[test]
    fn test_revalidate_single_file_target_ディレクトリはnotfoundを返す() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let result = revalidate_single_file_target(&canonical);
        assert_eq!(result, Err(ResolveFileError::NotFound));
    }

    #[test]
    fn test_revalidate_single_file_target_非mdファイルはnotmarkdownを返す() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap();
        let canonical = file_path.canonicalize().unwrap();
        let result = revalidate_single_file_target(&canonical);
        assert_eq!(result, Err(ResolveFileError::NotMarkdown));
    }

    #[cfg(unix)]
    #[test]
    fn test_revalidate_single_file_target_シンボリックリンクはtraversalを返す() {
        let dir = tempfile::tempdir().unwrap();
        let real_file = dir.path().join("real.md");
        std::fs::write(&real_file, "# real").unwrap();
        let link_path = dir.path().join("link.md");
        std::os::unix::fs::symlink(&real_file, &link_path).unwrap();
        let result = revalidate_single_file_target(&link_path);
        assert_eq!(result, Err(ResolveFileError::Traversal));
    }

    fn create_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# README").unwrap();
        std::fs::write(dir.path().join("guide.md"), "# Guide").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "text file").unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("docs/api.md"), "# API").unwrap();
        std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join(".hidden/secret.md"), "# Secret").unwrap();
        std::fs::write(dir.path().join(".dotfile.md"), "# Dot").unwrap();
        dir
    }

    fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    fn create_single_file_state(file_path: &std::path::Path) -> AppState {
        let mode = AppMode::new_single_file(file_path).unwrap();
        let (tx, _rx) = broadcast::channel(4);
        AppState::new(mode, false, None, tx)
    }

    fn create_directory_state(dir_path: &std::path::Path) -> AppState {
        let mode = AppMode::new_directory(dir_path).unwrap();
        let (tx, _rx) = broadcast::channel(4);
        AppState::new(mode, false, None, tx)
    }
}
