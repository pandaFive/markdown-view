use std::path::{Path, PathBuf};

use axum::http::StatusCode;

use super::content::MAX_FILE_SIZE;
use super::memo_fs::{MemoFs, MemoReadError};
use super::memo_sidecar::SidecarMemoName;
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
    let fs = state.memo_fs().as_ref();
    let memo_paths = memo_paths_for_target(state, target);
    let Some(memo_path) = resolve_active_memo_path(state, target, request, &memo_paths, fs).await?
    else {
        return Ok(MemoResponse::empty(
            target.relative_path().map(ToOwned::to_owned),
        ));
    };

    let raw = read_memo_file(&memo_path, target, request, fs).await?;
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
    let fs = state.memo_fs().as_ref();
    let trimmed = raw.trim();
    let memo_paths = memo_paths_for_target(state, target);
    if trimmed.is_empty() {
        return delete_route_memo(state, target, request, &memo_paths, fs).await;
    }

    if raw.len() as u64 > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }

    let memo_path = &memo_paths.sidecar;
    ensure_safe_memo_path(memo_path, state, target, request)?;
    if let Some(parent) = memo_path.parent() {
        fs.create_dir_all(parent)
            .await
            .map_err(|error| io_api_error(target, request, "ディレクトリ作成", error))?;
    }
    fs.write(memo_path, raw.as_bytes())
        .await
        .map_err(|error| io_api_error(target, request, "保存", error))?;

    cleanup_compat_sidecar_best_effort(state, target, request, &memo_paths, fs).await;
    cleanup_legacy_memo_best_effort(state, target, request, &memo_paths.legacy, fs).await;

    Ok(MemoResponse::from_raw(
        raw,
        target.relative_path().map(ToOwned::to_owned),
    ))
}

async fn delete_route_memo(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) -> Result<MemoResponse, ApiError> {
    ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
    match fs.remove_file(&memo_paths.sidecar).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_api_error(target, request, "削除", error)),
    }

    cleanup_compat_sidecar_best_effort(state, target, request, memo_paths, fs).await;
    cleanup_legacy_memo_best_effort(state, target, request, &memo_paths.legacy, fs).await;

    Ok(MemoResponse::empty(
        target.relative_path().map(ToOwned::to_owned),
    ))
}

#[derive(Debug, Clone)]
struct MemoPaths {
    sidecar: PathBuf,
    compat_sidecar: Option<PathBuf>,
    legacy: PathBuf,
}

fn legacy_memo_root(base_dir: &Path) -> PathBuf {
    base_dir
        .join(LEGACY_MEMO_DIR_NAME)
        .join(LEGACY_MEMO_SUBDIR_NAME)
}

fn memo_paths_for_target(state: &AppState, target: &ResolvedTarget) -> MemoPaths {
    MemoPaths {
        sidecar: sidecar_memo_path_for_target(target, state.mode().base_dir()),
        compat_sidecar: compat_sidecar_memo_path_for_target(target, state.mode().base_dir()),
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

fn sidecar_memo_path_for_target(target: &ResolvedTarget, base_dir: &Path) -> PathBuf {
    let target_path = target.file_path();
    let parent = sidecar_parent_for_target_path(target_path, base_dir);
    let file_name = target_path
        .file_name()
        .map(SidecarMemoName::from_file_name)
        .unwrap_or_else(SidecarMemoName::fallback);
    parent.join(file_name.as_str())
}

fn compat_sidecar_memo_path_for_target(
    target: &ResolvedTarget,
    base_dir: &Path,
) -> Option<PathBuf> {
    let target_path = target.file_path();
    let parent = sidecar_parent_for_target_path(target_path, base_dir);
    let file_name = target_path
        .file_name()
        .and_then(SidecarMemoName::compat_from_file_name)?;
    Some(parent.join(file_name.as_str()))
}

pub(super) fn sidecar_parent_for_target_path(target_path: &Path, base_dir: &Path) -> PathBuf {
    debug_assert!(
        target_path.is_absolute(),
        "ResolvedTarget::file_path must be absolute"
    );
    sidecar_parent_or_base(target_path, base_dir)
}

pub(super) fn sidecar_parent_or_base(target_path: &Path, base_dir: &Path) -> PathBuf {
    target_path
        .parent()
        .filter(|parent| parent.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| base_dir.to_path_buf())
}

async fn resolve_active_memo_path(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) -> Result<Option<PathBuf>, ApiError> {
    if let Some(path) =
        pick_existing_safe_path(state, target, request, &memo_paths.sidecar, "sidecar", fs).await?
    {
        return Ok(Some(path));
    }

    if let Some(compat_sidecar) = &memo_paths.compat_sidecar {
        if let Some(path) =
            pick_existing_safe_path(state, target, request, compat_sidecar, "互換sidecar", fs)
                .await?
        {
            return Ok(Some(path));
        }
    }

    pick_existing_safe_path(state, target, request, &memo_paths.legacy, "legacy", fs).await
}

async fn pick_existing_safe_path(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    path: &Path,
    label: &str,
    fs: &dyn MemoFs,
) -> Result<Option<PathBuf>, ApiError> {
    if let Err(error) = ensure_safe_memo_path(path, state, target, request) {
        tracing::warn!(
            "[markdown-view] {}unsafeな{}メモは読み込み候補から除外します ({}): {:?}",
            request.read_error_log_label(),
            label,
            target.file_label(),
            error
        );
        return Ok(None);
    }

    match fs.try_exists(path).await {
        Ok(true) => Ok(Some(path.to_path_buf())),
        Ok(false) => Ok(None),
        Err(error) => Err(io_api_error(target, request, "存在確認", error)),
    }
}

async fn cleanup_compat_sidecar_best_effort(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) {
    if let Some(compat_sidecar) = &memo_paths.compat_sidecar {
        cleanup_memo_path_best_effort(state, target, request, compat_sidecar, "互換sidecar", fs)
            .await;
    }
}

async fn cleanup_legacy_memo_best_effort(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    legacy_path: &Path,
    fs: &dyn MemoFs,
) {
    cleanup_memo_path_best_effort(state, target, request, legacy_path, "legacy", fs).await;
}

async fn cleanup_memo_path_best_effort(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    path: &Path,
    label: &str,
    fs: &dyn MemoFs,
) {
    if let Err(error) = ensure_safe_memo_path(path, state, target, request) {
        tracing::warn!(
            "[markdown-view] {}unsafeな{}メモは削除せず無視します ({}): {:?}",
            request.read_error_log_label(),
            label,
            target.file_label(),
            error
        );
        return;
    }

    match fs.try_exists(path).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::warn!(
                "[markdown-view] {}{}メモcleanup存在確認失敗を無視します ({}): {}",
                request.read_error_log_label(),
                label,
                target.file_label(),
                error
            );
            return;
        }
    }

    match fs.remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(
                "[markdown-view] {}{}メモcleanup削除失敗を無視します ({}): {}",
                request.read_error_log_label(),
                label,
                target.file_label(),
                error
            );
        }
    }
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
    fs: &dyn MemoFs,
) -> Result<String, ApiError> {
    let metadata = fs
        .metadata(memo_path)
        .await
        .map_err(|error| io_api_error(target, request, "メタデータ取得", error))?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }

    let bytes = fs
        .read_with_limit(memo_path)
        .await
        .map_err(|error| memo_read_error_to_api_error(target, request, error))?;
    if bytes.len() as u64 > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }
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

fn read_io_api_error(
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    error: std::io::Error,
) -> ApiError {
    tracing::warn!(
        "[markdown-view] {}メモ読み込みエラー ({}): {}",
        request.read_error_log_label(),
        target.file_label(),
        error
    );
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "メモファイルの読み込みに失敗しました",
    )
}

fn memo_read_error_to_api_error(
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    error: MemoReadError,
) -> ApiError {
    match error {
        MemoReadError::Open(error) => io_api_error(target, request, "読込", error),
        MemoReadError::Read(error) => read_io_api_error(target, request, error),
        MemoReadError::TooLarge => json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ),
    }
}
