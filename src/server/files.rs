use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use tokio::io::AsyncReadExt;

use super::guards::json_error;
use super::{
    file_size_limit_error_message, ApiError, AppState, MAX_DIR_DEPTH, MAX_FILE_LIST, MAX_FILE_SIZE,
};
use crate::renderer::render_markdown;
use crate::template::{error_message_json, UpdateMessage};
use crate::toc::generate_toc;

#[derive(Debug, Copy, Clone)]
pub(super) enum TargetResolveContext {
    Index,
    ApiContent,
}

impl TargetResolveContext {
    fn not_found_message(self) -> &'static str {
        match self {
            Self::Index => "表示可能なMarkdownファイルが見つかりません",
            Self::ApiContent => "指定したファイルが見つかりません",
        }
    }

    fn read_error_log_label(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::ApiContent => "api/content",
        }
    }
}

/// 対象ファイル解決エラーをエンドポイント文脈に応じたAPIエラーへ変換する。
pub(super) fn resolve_target_file_or_error(
    state: &AppState,
    query_file: Option<&str>,
    include_file_list: bool,
    context: TargetResolveContext,
) -> Result<(PathBuf, Option<Vec<String>>), ApiError> {
    super::resolve_target_file(state, query_file, include_file_list).map_err(|status| {
        let msg = match status {
            StatusCode::NOT_FOUND => context.not_found_message(),
            StatusCode::INTERNAL_SERVER_ERROR => "ファイル一覧の取得に失敗しました",
            other => {
                tracing::warn!(
                    "[markdown-view] 予期しないファイル解決ステータスを検出: context={:?}, status={}",
                    context,
                    other
                );
                "ファイル解決に失敗しました"
            }
        };
        json_error(status, msg)
    })
}

/// Markdownの読み込みと描画を行い、失敗時はAPI応答用のエラーへ変換する。
pub(super) async fn read_rendered_update_or_error(
    file_path: &Path,
    context: TargetResolveContext,
) -> Result<UpdateMessage, ApiError> {
    read_and_render_file(file_path).await.map_err(|e| {
        tracing::warn!(
            "[markdown-view] {}読み込みエラー: {}",
            context.read_error_log_label(),
            e
        );
        json_error(e.status_code(), e.user_message())
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
            ReadMarkdownError::TooLarge => write!(f, "{}", file_size_limit_error_message()),
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
            ReadMarkdownError::TooLarge => file_size_limit_error_message(),
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
    EmptyPath,
    InvalidPath,
    NotFound,
    Traversal,
    NotMarkdown,
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
    pub fn status_code(&self) -> StatusCode {
        StatusCode::NOT_FOUND
    }
}

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

/// ファイルを読み込んでレンダリングする
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
