use std::path::{Path, PathBuf};

use axum::http::StatusCode;

use super::content::{read_bytes_with_limit, ReadMarkdownError, MAX_FILE_SIZE};
use super::memo_sidecar::{SidecarMemoName, MAX_FILENAME_BYTES};
use super::resolve::ResolvedTarget;
use super::RouteTargetRequest;
use crate::server::guards::json_error;
use crate::server::log_path::sanitize_path_for_logging;
use crate::server::messages::ApiError;
use crate::server::state::AppState;
use crate::template::MemoResponse;

const LEGACY_MEMO_DIR_NAME: &str = ".markdown-view";
const LEGACY_MEMO_SUBDIR_NAME: &str = "memos";

/// メモを読み込み、プレビューHTML付き応答へ変換する。
pub(in crate::server) async fn load_route_memo(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<MemoResponse, ApiError> {
    let memo_paths = memo_paths_for_target(state, target);
    let memo_path = resolve_active_memo_path(state, target, request, &memo_paths).await?;
    if !tokio::fs::try_exists(&memo_path)
        .await
        .map_err(|error| io_api_error(target, request, "存在確認", error))?
    {
        return Ok(MemoResponse::empty(
            target.relative_path().map(ToOwned::to_owned),
        ));
    }

    let raw = read_memo_file(&memo_path, target, request).await?;
    Ok(MemoResponse::from_raw(
        raw,
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
    let memo_paths = memo_paths_for_target(state, target);
    if trimmed.is_empty() {
        delete_legacy_memo_if_safe_strict(state, target, request, &memo_paths.legacy).await?;
        if !sidecar_name_too_long(&memo_paths.sidecar) {
            ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
            delete_memo_file_if_exists(&memo_paths.sidecar, target, request).await?;
        }
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

    let save_target = choose_save_target(state, target, request, &memo_paths).await?;
    let memo_path = match save_target {
        SaveTarget::Sidecar { .. } => &memo_paths.sidecar,
        SaveTarget::Legacy => &memo_paths.legacy,
    };
    ensure_safe_memo_path(memo_path, state, target, request)?;
    if let Some(parent) = memo_path.parent() {
        if let Err(error) = tokio::fs::create_dir_all(parent).await {
            if can_fallback_to_legacy(save_target, &error) {
                return save_memo_to_legacy(state, target, request, &memo_paths.legacy, raw).await;
            }
            return Err(io_api_error(target, request, "ディレクトリ作成", error));
        }
    }
    if let Err(error) = tokio::fs::write(memo_path, raw.as_bytes()).await {
        if can_fallback_to_legacy(save_target, &error) {
            return save_memo_to_legacy(state, target, request, &memo_paths.legacy, raw).await;
        }
        return Err(io_api_error(target, request, "保存", error));
    }

    if let SaveTarget::Sidecar {
        delete_legacy_after_save: true,
        ..
    } = save_target
    {
        cleanup_legacy_memo_if_safe(state, target, request, &memo_paths.legacy).await?;
    }

    Ok(MemoResponse::from_raw(
        raw,
        target.relative_path().map(ToOwned::to_owned),
    ))
}

#[derive(Debug, Clone)]
struct MemoPaths {
    sidecar: PathBuf,
    legacy: PathBuf,
}

#[derive(Debug, Clone, Copy)]
enum SaveTarget {
    Sidecar {
        allow_legacy_fallback: bool,
        delete_legacy_after_save: bool,
    },
    Legacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyMemoState {
    SafeExists,
    SafeMissing,
    Unsafe,
}

fn legacy_memo_root(base_dir: &Path) -> PathBuf {
    base_dir
        .join(LEGACY_MEMO_DIR_NAME)
        .join(LEGACY_MEMO_SUBDIR_NAME)
}

fn memo_paths_for_target(state: &AppState, target: &ResolvedTarget) -> MemoPaths {
    MemoPaths {
        sidecar: sidecar_memo_path_for_target(target),
        legacy: legacy_memo_path_for_target(state, target),
    }
}

fn legacy_memo_path_for_target(state: &AppState, target: &ResolvedTarget) -> PathBuf {
    let root = legacy_memo_root(state.mode().base_dir());
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

fn sidecar_memo_path_for_target(target: &ResolvedTarget) -> PathBuf {
    let target_path = target.file_path();
    let parent = target_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let file_name = target_path
        .file_name()
        .map(SidecarMemoName::from_file_name)
        .unwrap_or_else(SidecarMemoName::fallback);
    parent.join(file_name.as_str())
}

async fn resolve_active_memo_path(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
) -> Result<PathBuf, ApiError> {
    let sidecar_usable = !sidecar_name_too_long(&memo_paths.sidecar);
    if sidecar_usable {
        ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
    }
    if sidecar_usable
        && tokio::fs::try_exists(&memo_paths.sidecar)
            .await
            .map_err(|error| io_api_error(target, request, "存在確認", error))?
    {
        return Ok(memo_paths.sidecar.clone());
    }

    match inspect_legacy_memo(state, target, request, &memo_paths.legacy).await? {
        LegacyMemoState::SafeExists => Ok(memo_paths.legacy.clone()),
        LegacyMemoState::SafeMissing | LegacyMemoState::Unsafe => {
            if sidecar_usable {
                Ok(memo_paths.sidecar.clone())
            } else {
                Ok(memo_paths.legacy.clone())
            }
        }
    }
}

async fn choose_save_target(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
) -> Result<SaveTarget, ApiError> {
    let sidecar_too_long = sidecar_name_too_long(&memo_paths.sidecar);
    if !sidecar_too_long {
        ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
    }
    if !sidecar_too_long
        && tokio::fs::try_exists(&memo_paths.sidecar)
            .await
            .map_err(|error| io_api_error(target, request, "存在確認", error))?
    {
        return Ok(SaveTarget::Sidecar {
            allow_legacy_fallback: false,
            delete_legacy_after_save: false,
        });
    }

    let legacy_state = inspect_legacy_memo(state, target, request, &memo_paths.legacy).await?;
    if sidecar_too_long {
        return Ok(SaveTarget::Legacy);
    }

    if matches!(
        legacy_state,
        LegacyMemoState::SafeMissing | LegacyMemoState::Unsafe
    ) {
        return Ok(SaveTarget::Sidecar {
            allow_legacy_fallback: state.mode().is_directory(),
            delete_legacy_after_save: false,
        });
    }

    Ok(SaveTarget::Sidecar {
        allow_legacy_fallback: true,
        delete_legacy_after_save: true,
    })
}

async fn legacy_memo_exists(
    legacy_memo_path: &Path,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<bool, ApiError> {
    tokio::fs::try_exists(legacy_memo_path)
        .await
        .map_err(|error| io_api_error(target, request, "存在確認", error))
}

async fn inspect_legacy_memo(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    legacy_memo_path: &Path,
) -> Result<LegacyMemoState, ApiError> {
    if let Err(error) = ensure_safe_memo_path(legacy_memo_path, state, target, request) {
        tracing::warn!(
            "[markdown-view] {}unsafeなlegacyメモは未使用扱いにします ({}): {:?}",
            request.read_error_log_label(),
            target.file_label(),
            error
        );
        return Ok(LegacyMemoState::Unsafe);
    }

    if legacy_memo_exists(legacy_memo_path, target, request).await? {
        Ok(LegacyMemoState::SafeExists)
    } else {
        Ok(LegacyMemoState::SafeMissing)
    }
}

fn can_fallback_to_legacy(save_target: SaveTarget, error: &std::io::Error) -> bool {
    matches!(
        save_target,
        SaveTarget::Sidecar {
            allow_legacy_fallback: true,
            ..
        }
    ) && (error.kind() == std::io::ErrorKind::PermissionDenied || is_name_too_long_error(error))
}

fn is_name_too_long_error(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(36)
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

async fn save_memo_to_legacy(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    legacy_path: &Path,
    raw: String,
) -> Result<MemoResponse, ApiError> {
    ensure_safe_memo_path(legacy_path, state, target, request)?;
    if let Some(parent) = legacy_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| io_api_error(target, request, "ディレクトリ作成", error))?;
    }
    tokio::fs::write(legacy_path, raw.as_bytes())
        .await
        .map_err(|error| io_api_error(target, request, "保存", error))?;

    Ok(MemoResponse::from_raw(
        raw,
        target.relative_path().map(ToOwned::to_owned),
    ))
}

async fn cleanup_legacy_memo_if_safe(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    legacy_path: &Path,
) -> Result<(), ApiError> {
    if let Err(error) = ensure_safe_memo_path(legacy_path, state, target, request) {
        tracing::warn!(
            "[markdown-view] {}unsafeなlegacyメモは削除せず無視します ({}): {:?}",
            request.read_error_log_label(),
            target.file_label(),
            error
        );
        return Ok(());
    }

    if !legacy_memo_exists(legacy_path, target, request).await? {
        return Ok(());
    }

    if let Err(error) = delete_memo_file_if_exists(legacy_path, target, request).await {
        tracing::warn!(
            "[markdown-view] {}legacyメモcleanup失敗を無視します ({}): {:?}",
            request.read_error_log_label(),
            target.file_label(),
            error
        );
    }
    Ok(())
}

async fn delete_legacy_memo_if_safe_strict(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    legacy_path: &Path,
) -> Result<(), ApiError> {
    if let Err(error) = ensure_safe_memo_path(legacy_path, state, target, request) {
        tracing::warn!(
            "[markdown-view] {}unsafeなlegacyメモは削除せず無視します ({}): {:?}",
            request.read_error_log_label(),
            target.file_label(),
            error
        );
        return Ok(());
    }

    if !legacy_memo_exists(legacy_path, target, request).await? {
        return Ok(());
    }

    delete_memo_file_if_exists(legacy_path, target, request).await
}

fn sidecar_name_too_long(sidecar_path: &Path) -> bool {
    sidecar_path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().len() > MAX_FILENAME_BYTES)
}

fn ensure_safe_memo_path(
    memo_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), ApiError> {
    let base_dir = state.mode().base_dir();
    if let Some(unsafe_component) = first_symlink_component(base_dir, memo_path) {
        tracing::warn!(
            "[markdown-view] {}メモパスがシンボリックリンクを含むため拒否 ({} -> {}): {}",
            request.read_error_log_label(),
            target.file_label(),
            sanitize_path_for_logging(memo_path, base_dir),
            sanitize_path_for_logging(&unsafe_component, base_dir)
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
