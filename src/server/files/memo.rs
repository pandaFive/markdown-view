use std::path::{Path, PathBuf};

use axum::http::StatusCode;

use super::content::{read_bytes_with_limit, ReadMarkdownError, MAX_FILE_SIZE};
use super::resolve::ResolvedTarget;
use super::RouteTargetRequest;
use crate::renderer::render_markdown;
use crate::server::guards::json_error;
use crate::server::messages::ApiError;
use crate::server::state::AppState;
use crate::template::MemoResponse;

const MEMO_DIR_NAME: &str = ".markdown-view";
const MEMO_SUBDIR_NAME: &str = "memos";

/// メモを読み込み、プレビューHTML付き応答へ変換する。
pub(in crate::server) async fn load_route_memo(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<MemoResponse, ApiError> {
    let memo_path = memo_path_for_target(state, target);
    ensure_safe_memo_path(&memo_path, state, target, request)?;
    if !tokio::fs::try_exists(&memo_path)
        .await
        .map_err(|error| io_api_error(target, request, "存在確認", error))?
    {
        return Ok(MemoResponse::empty(
            target.relative_path().map(ToOwned::to_owned),
        ));
    }

    let raw = read_memo_file(&memo_path, target, request).await?;
    Ok(MemoResponse::new(
        raw.clone(),
        render_markdown(&raw),
        target.relative_path().map(ToOwned::to_owned),
    ))
}

/// メモを保存し、保存後のプレビューHTML付き応答を返す。
pub(in crate::server) async fn save_route_memo(
    state: &AppState,
    target: &ResolvedTarget,
    raw: String,
    request: RouteTargetRequest<'_>,
) -> Result<MemoResponse, ApiError> {
    let trimmed = raw.trim();
    let memo_path = memo_path_for_target(state, target);
    if trimmed.is_empty() {
        ensure_safe_memo_path(&memo_path, state, target, request)?;
        delete_memo_file_if_exists(&memo_path, target, request).await?;
        return Ok(MemoResponse::empty(
            target.relative_path().map(ToOwned::to_owned),
        ));
    }

    if raw.len() as u64 > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }

    ensure_safe_memo_path(&memo_path, state, target, request)?;
    if let Some(parent) = memo_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| io_api_error(target, request, "ディレクトリ作成", error))?;
    }
    ensure_safe_memo_path(&memo_path, state, target, request)?;
    tokio::fs::write(&memo_path, raw.as_bytes())
        .await
        .map_err(|error| io_api_error(target, request, "保存", error))?;

    Ok(MemoResponse::new(
        raw.clone(),
        render_markdown(&raw),
        target.relative_path().map(ToOwned::to_owned),
    ))
}

fn memo_root(base_dir: &Path) -> PathBuf {
    base_dir.join(MEMO_DIR_NAME).join(MEMO_SUBDIR_NAME)
}

fn memo_path_for_target(state: &AppState, target: &ResolvedTarget) -> PathBuf {
    let root = memo_root(state.mode().base_dir());
    if let Some(relative_path) = target.relative_path() {
        root.join(relative_path)
    } else {
        root.join(
            target
                .file_path()
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("memo.md")),
        )
    }
}

fn ensure_safe_memo_path(
    memo_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), ApiError> {
    let base_dir = state.mode().base_dir();
    let memo_root = memo_root(base_dir);
    if let Some(unsafe_component) = first_symlink_component(base_dir, &memo_root)
        .or_else(|| first_symlink_component(base_dir, memo_path))
    {
        tracing::warn!(
            "[markdown-view] {}メモパスがシンボリックリンクを含むため拒否 ({} -> {}): {}",
            request.read_error_log_label(),
            target.file_label(),
            memo_path.display(),
            unsafe_component.display()
        );
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "メモ保存先にシンボリックリンクが含まれているため操作できません",
        ));
    }
    Ok(())
}

fn first_symlink_component(base_dir: &Path, target: &Path) -> Option<PathBuf> {
    let relative = target.strip_prefix(base_dir).ok()?;
    let mut current = base_dir.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Some(current),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => continue,
        }
    }
    None
}

async fn read_memo_file(
    memo_path: &Path,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<String, ApiError> {
    let metadata = tokio::fs::metadata(memo_path)
        .await
        .map_err(|error| io_api_error(target, request, "メタデータ取得", error))?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }

    let file = tokio::fs::File::open(memo_path)
        .await
        .map_err(|error| io_api_error(target, request, "読込", error))?;
    let bytes = read_bytes_with_limit(file)
        .await
        .map_err(|error| read_error_to_api_error(target, request, error))?;
    String::from_utf8(bytes).map_err(|error| {
        tracing::warn!(
            "[markdown-view] {}メモUTF-8デコード失敗 ({}): {}",
            request.read_error_log_label(),
            target.file_label(),
            error
        );
        json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "メモはUTF-8テキストである必要があります",
        )
    })
}

async fn delete_memo_file_if_exists(
    memo_path: &Path,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), ApiError> {
    match tokio::fs::remove_file(memo_path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_api_error(target, request, "削除", error)),
    }
}

fn io_api_error(
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    action: &str,
    error: std::io::Error,
) -> ApiError {
    tracing::warn!(
        "[markdown-view] {}メモ{}エラー ({}): {}",
        request.read_error_log_label(),
        action,
        target.file_label(),
        error
    );
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "メモファイルの操作に失敗しました",
    )
}

fn read_error_to_api_error(
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    error: ReadMarkdownError,
) -> ApiError {
    tracing::warn!(
        "[markdown-view] {}メモ読み込みエラー ({}): {}",
        request.read_error_log_label(),
        target.file_label(),
        error
    );
    match error {
        ReadMarkdownError::TooLarge => json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ),
        ReadMarkdownError::NotUtf8 => json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "メモはUTF-8テキストである必要があります",
        ),
        ReadMarkdownError::Io(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "メモファイルの読み込みに失敗しました",
        ),
    }
}
