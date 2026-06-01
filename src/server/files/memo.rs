use std::path::{Path, PathBuf};

use axum::http::StatusCode;

use super::catalog::{
    open_relative_dir_nofollow, open_verified_base_dir_checked, OpenVerifiedBaseDirError,
};
use super::content::MAX_FILE_SIZE;
use super::memo_fs::{
    before_access_future, before_remove_future, BeforeAccessCheck, BeforeRemoveCheck,
    CheckedMemoPath, MemoBeforeRenameError, MemoFs, MemoReadError, MemoRemoveError, MemoWriteError,
};
use super::memo_sidecar::SidecarMemoName;
use super::resolve::ResolvedTarget;
use super::run_blocking_file_task;
use super::RouteTargetRequest;
use crate::server::guards::json_error;
use crate::server::log_path::sanitize_path_for_logging_escaped;
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
    ensure_current_memo_root_for_memo_api(state, target, request).await?;
    let Some(raw) = read_active_memo_file(state, target, request, &memo_paths, fs).await? else {
        return Ok(MemoResponse::empty(
            target.relative_path().map(ToOwned::to_owned),
        ));
    };

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
    ensure_current_memo_root_for_memo_api(state, target, request).await?;
    ensure_safe_memo_path(memo_path, state, target, request).await?;
    let before_write = |write_path: &Path| {
        let write_path = write_path.to_path_buf();
        before_access_future(async move {
            checked_safe_memo_access_path(&write_path, state, target, request).await
        })
    };
    fs.write_atomic(
        memo_path,
        raw.as_bytes(),
        &before_write as &BeforeAccessCheck<'_>,
    )
    .await
    .map_err(|error| memo_write_error_to_api_error(target, request, error))?;

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
    ensure_current_memo_root_for_memo_api(state, target, request).await?;
    ensure_safe_memo_path(&memo_paths.sidecar, state, target, request).await?;

    cleanup_compat_sidecar_required(state, target, request, memo_paths, fs).await?;
    cleanup_legacy_memo_required(state, target, request, &memo_paths.legacy, fs).await?;

    match remove_memo_file_checked(state, target, request, &memo_paths.sidecar, fs).await {
        Ok(()) => {}
        Err(MemoRemoveError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(MemoRemoveError::Io(error)) => {
            return Err(io_api_error(target, request, "削除", error))
        }
        Err(MemoRemoveError::BeforeRemove(error)) => {
            return Err(memo_before_remove_error_to_api_error(
                target, request, "削除", error,
            ));
        }
    }

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

async fn read_active_memo_file(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) -> Result<Option<String>, ApiError> {
    if let Some(raw) = read_existing_safe_memo_file(
        state,
        target,
        request,
        &memo_paths.sidecar,
        "sidecar",
        UnsafeMemoPath::Reject,
        fs,
    )
    .await?
    {
        return Ok(Some(raw));
    }

    if let Some(compat_sidecar) = &memo_paths.compat_sidecar {
        if let Some(raw) = read_existing_safe_memo_file(
            state,
            target,
            request,
            compat_sidecar,
            "互換sidecar",
            UnsafeMemoPath::Skip,
            fs,
        )
        .await?
        {
            return Ok(Some(raw));
        }
    }

    read_existing_safe_memo_file(
        state,
        target,
        request,
        &memo_paths.legacy,
        "legacy",
        UnsafeMemoPath::Skip,
        fs,
    )
    .await
}

async fn read_existing_safe_memo_file(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    path: &Path,
    label: &str,
    unsafe_path: UnsafeMemoPath,
    fs: &dyn MemoFs,
) -> Result<Option<String>, ApiError> {
    let Some(path) =
        pick_existing_safe_path(state, target, request, path, label, unsafe_path).await?
    else {
        return Ok(None);
    };

    read_memo_file_if_present(state, &path, target, request, fs).await
}

#[derive(Clone, Copy)]
enum UnsafeMemoPath {
    Reject,
    Skip,
}

async fn pick_existing_safe_path(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    path: &Path,
    label: &str,
    unsafe_path: UnsafeMemoPath,
) -> Result<Option<PathBuf>, ApiError> {
    if let Err(error) = ensure_safe_memo_path(path, state, target, request).await {
        let action = match unsafe_path {
            UnsafeMemoPath::Reject => "読み込みを拒否します",
            UnsafeMemoPath::Skip => "読み込み候補から除外します",
        };
        tracing::warn!(
            "[markdown-view] {}unsafeな{}メモは{} ({}): {:?}",
            request.read_error_log_label(),
            label,
            action,
            target.file_label().escape_debug(),
            error
        );
        return match unsafe_path {
            UnsafeMemoPath::Reject => Err(error),
            UnsafeMemoPath::Skip => Ok(None),
        };
    }

    Ok(Some(path.to_path_buf()))
}

async fn cleanup_compat_sidecar_best_effort(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) {
    if let Some(compat_sidecar) = &memo_paths.compat_sidecar {
        if compat_sidecar == &memo_paths.sidecar {
            return;
        }
        cleanup_memo_path_best_effort(state, target, request, compat_sidecar, "互換sidecar", fs)
            .await;
    }
}

async fn cleanup_compat_sidecar_required(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) -> Result<(), ApiError> {
    if let Some(compat_sidecar) = &memo_paths.compat_sidecar {
        if compat_sidecar == &memo_paths.sidecar {
            return Ok(());
        }
        cleanup_memo_path_required(state, target, request, compat_sidecar, "互換sidecar", fs)
            .await?;
    }
    Ok(())
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

async fn cleanup_legacy_memo_required(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    legacy_path: &Path,
    fs: &dyn MemoFs,
) -> Result<(), ApiError> {
    cleanup_memo_path_required(state, target, request, legacy_path, "legacy", fs).await
}

async fn cleanup_memo_path_best_effort(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    path: &Path,
    label: &str,
    fs: &dyn MemoFs,
) {
    if let Err(error) = ensure_safe_memo_path(path, state, target, request).await {
        tracing::warn!(
            "[markdown-view] {}unsafeな{}メモは削除せず無視します ({}): {:?}",
            request.read_error_log_label(),
            label,
            target.file_label().escape_debug(),
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
                target.file_label().escape_debug(),
                error
            );
            return;
        }
    }

    match remove_memo_file_checked(state, target, request, path, fs).await {
        Ok(()) => {}
        Err(MemoRemoveError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(MemoRemoveError::Io(error)) => {
            tracing::warn!(
                "[markdown-view] {}{}メモcleanup削除失敗を無視します ({}): {}",
                request.read_error_log_label(),
                label,
                target.file_label().escape_debug(),
                error
            );
        }
        Err(MemoRemoveError::BeforeRemove(error)) => {
            tracing::warn!(
                "[markdown-view] {}{}メモcleanup削除直前検証失敗を無視します ({}): {}",
                request.read_error_log_label(),
                label,
                target.file_label().escape_debug(),
                error.user_message()
            );
        }
    }
}

async fn cleanup_memo_path_required(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    path: &Path,
    label: &str,
    fs: &dyn MemoFs,
) -> Result<(), ApiError> {
    if let Err(error) = ensure_safe_memo_path(path, state, target, request).await {
        if error.0 != StatusCode::FORBIDDEN {
            tracing::warn!(
                "[markdown-view] {}{}メモ必須cleanup安全確認失敗 ({}): {:?}",
                request.read_error_log_label(),
                label,
                target.file_label().escape_debug(),
                error
            );
            return Err(error);
        }
        tracing::warn!(
            "[markdown-view] {}unsafeな{}メモは削除せず無視します ({}): {:?}",
            request.read_error_log_label(),
            label,
            target.file_label().escape_debug(),
            error
        );
        return Ok(());
    }

    match fs.try_exists(path).await {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(error) => return Err(io_api_error(target, request, "cleanup存在確認", error)),
    }

    match remove_memo_file_checked(state, target, request, path, fs).await {
        Ok(()) => Ok(()),
        Err(MemoRemoveError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(MemoRemoveError::Io(error)) => Err(io_api_error(target, request, "cleanup削除", error)),
        Err(MemoRemoveError::BeforeRemove(error))
            if error.status_code() == StatusCode::FORBIDDEN =>
        {
            tracing::warn!(
                "[markdown-view] {}unsafeな{}メモは削除せず無視します ({}): {}",
                request.read_error_log_label(),
                label,
                target.file_label().escape_debug(),
                error.user_message()
            );
            Ok(())
        }
        Err(MemoRemoveError::BeforeRemove(error)) => Err(memo_before_remove_error_to_api_error(
            target,
            request,
            "cleanup削除",
            error,
        )),
    }
}

async fn remove_memo_file_checked(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    path: &Path,
    fs: &dyn MemoFs,
) -> Result<(), MemoRemoveError> {
    let before_remove = |remove_path: &Path| {
        let remove_path = remove_path.to_path_buf();
        before_remove_future(async move {
            checked_safe_memo_access_path(&remove_path, state, target, request).await
        })
    };
    fs.remove_file(path, &before_remove as &BeforeRemoveCheck<'_>)
        .await
}

async fn ensure_safe_memo_path(
    memo_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), ApiError> {
    let base_dir = state.mode().base_dir();
    if let Some(unsafe_component) = first_unsafe_memo_path_component(base_dir, memo_path).await {
        log_unsafe_memo_path(&unsafe_component, memo_path, base_dir, target, request);
        return Err(json_error(
            unsafe_component.status_code(),
            unsafe_component.user_message(),
        ));
    }
    Ok(())
}

async fn checked_safe_memo_access_path(
    memo_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<CheckedMemoPath, MemoBeforeRenameError> {
    let checked = checked_current_memo_root_for_memo_rename(state, target, request).await?;
    ensure_current_memo_target_parent_for_memo_rename(target, request).await?;
    ensure_safe_memo_rename_path(memo_path, state, target, request).await?;
    let memo_parent = memo_path
        .parent()
        .ok_or_else(|| MemoBeforeRenameError::internal("メモ保存先の安全確認に失敗しました"))?;
    let relative_parent = memo_parent
        .strip_prefix(checked.root_path.as_path())
        .map_err(|_| MemoBeforeRenameError::internal("メモ保存先の安全確認に失敗しました"))?;
    let file_name = memo_path
        .file_name()
        .ok_or_else(|| MemoBeforeRenameError::internal("メモ保存先の安全確認に失敗しました"))?;
    let parent_dir = open_relative_dir_nofollow(&checked.root_dir, relative_parent)
        .map_err(|error| MemoBeforeRenameError::internal(error.to_string()))?;
    Ok(CheckedMemoPath::new(parent_dir, PathBuf::from(file_name)))
}

async fn ensure_current_memo_root_for_memo_api(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), ApiError> {
    match current_memo_root_status(state, target, request).await {
        Ok(_) => Ok(()),
        Err(MemoRootError::Changed) => Err(json_error(
            StatusCode::FORBIDDEN,
            "メモ保存先の安全確認に失敗しました",
        )),
        Err(MemoRootError::InspectionFailed) => Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "メモ保存先の安全確認に失敗しました",
        )),
    }
}

async fn ensure_current_memo_target_parent_for_memo_rename(
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), MemoBeforeRenameError> {
    let Some(parent) = target.target_parent_canonical().cloned() else {
        tracing::warn!(
            "[markdown-view] {}メモ対象parent directoryの同一性情報がないため拒否 ({})",
            request.read_error_log_label(),
            target.file_label().escape_debug()
        );
        return Err(MemoBeforeRenameError::internal(
            "メモ保存先の安全確認に失敗しました",
        ));
    };
    let file_label = target.file_label().escape_debug().to_string();
    match run_blocking_file_task("メモ対象parent directory同一性確認", move || {
        parent.has_current_identity()
    })
    .await
    {
        Err(_) => Err(MemoBeforeRenameError::internal(
            "メモ保存先の安全確認に失敗しました",
        )),
        Ok(Ok(true)) => Ok(()),
        Ok(Ok(false)) => {
            tracing::warn!(
                "[markdown-view] {}メモ対象parent directoryの実体差し替えを検出しました ({})",
                request.read_error_log_label(),
                file_label
            );
            Err(MemoBeforeRenameError::new(
                "メモ保存先の安全確認に失敗しました",
            ))
        }
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Err(
            MemoBeforeRenameError::new("メモ保存先の安全確認に失敗しました"),
        ),
        Ok(Err(error)) => {
            tracing::warn!(
                "[markdown-view] {}メモ対象parent directoryの同一性確認に失敗しました ({}): kind={:?}",
                request.read_error_log_label(),
                file_label,
                error.kind()
            );
            Err(MemoBeforeRenameError::internal(
                "メモ保存先の安全確認に失敗しました",
            ))
        }
    }
}

async fn checked_current_memo_root_for_memo_rename(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<CheckedMemoRoot, MemoBeforeRenameError> {
    match current_memo_root_status(state, target, request).await {
        Ok(root) => Ok(root),
        Err(MemoRootError::Changed) => Err(MemoBeforeRenameError::new(
            "メモ保存先の安全確認に失敗しました",
        )),
        Err(MemoRootError::InspectionFailed) => Err(MemoBeforeRenameError::internal(
            "メモ保存先の安全確認に失敗しました",
        )),
    }
}

enum MemoRootError {
    Changed,
    InspectionFailed,
}

struct CheckedMemoRoot {
    root_path: crate::server::state::CanonicalPath,
    root_dir: cap_std::fs::Dir,
}

async fn current_memo_root_status(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<CheckedMemoRoot, MemoRootError> {
    let Some((root, label)) = state
        .mode()
        .single_file_parent_canonical()
        .map(|root| (root, "parent directory"))
        .or_else(|| {
            state
                .mode()
                .directory_canonical()
                .map(|root| (root, "base directory"))
        })
    else {
        return Err(MemoRootError::InspectionFailed);
    };
    let root = root.clone();
    let file_label = target.file_label().escape_debug().to_string();
    let log_root = sanitize_path_for_logging_escaped(root.as_path(), state.mode().base_dir());

    match run_blocking_file_task("メモroot directory同一性確認", move || {
        let root_dir = open_verified_base_dir_checked(&root, "メモ保存先root directory")?;
        Ok::<_, OpenVerifiedBaseDirError>(CheckedMemoRoot {
            root_path: root,
            root_dir,
        })
    })
    .await
    {
        Err(_) => Err(MemoRootError::InspectionFailed),
        Ok(Ok(root)) => Ok(root),
        Ok(Err(error)) => match error {
            OpenVerifiedBaseDirError::IdentityChanged => {
                tracing::warn!(
                    "[markdown-view] {}メモ保存先{}の実体差し替えを検出しました ({}; root={})",
                    request.read_error_log_label(),
                    label,
                    file_label,
                    log_root
                );
                Err(MemoRootError::Changed)
            }
            OpenVerifiedBaseDirError::Io(error) => {
                tracing::warn!(
                    "[markdown-view] {}メモ保存先{}の同一性確認に失敗しました ({}; root={}): kind={:?}",
                    request.read_error_log_label(),
                    label,
                    file_label,
                    log_root,
                    error.kind()
                );
                Err(MemoRootError::InspectionFailed)
            }
        },
    }
}

async fn ensure_safe_memo_rename_path(
    memo_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), MemoBeforeRenameError> {
    let base_dir = state.mode().base_dir();
    if let Some(unsafe_component) = first_unsafe_memo_path_component(base_dir, memo_path).await {
        log_unsafe_memo_path(&unsafe_component, memo_path, base_dir, target, request);
        return Err(unsafe_component.before_rename_error());
    }
    Ok(())
}

enum UnsafeMemoPathComponent {
    Symlink(PathBuf),
    InspectionError(PathBuf),
}

impl UnsafeMemoPathComponent {
    fn path(&self) -> &Path {
        match self {
            Self::Symlink(path) | Self::InspectionError(path) => path,
        }
    }

    fn user_message(&self) -> &'static str {
        match self {
            Self::Symlink(_) => "メモ保存先にシンボリックリンクが含まれているため操作できません",
            Self::InspectionError(_) => "メモ保存先の安全確認に失敗しました",
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::Symlink(_) => StatusCode::FORBIDDEN,
            Self::InspectionError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn before_rename_error(&self) -> MemoBeforeRenameError {
        match self {
            Self::Symlink(_) => MemoBeforeRenameError::new(self.user_message()),
            Self::InspectionError(_) => MemoBeforeRenameError::internal(self.user_message()),
        }
    }
}

fn log_unsafe_memo_path(
    unsafe_component: &UnsafeMemoPathComponent,
    memo_path: &Path,
    base_dir: &Path,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) {
    match unsafe_component {
        UnsafeMemoPathComponent::Symlink(_) => tracing::warn!(
            "[markdown-view] {}メモパスがシンボリックリンクを含むため拒否 ({} -> {}): {}",
            request.read_error_log_label(),
            target.file_label().escape_debug(),
            sanitize_path_for_logging_escaped(memo_path, base_dir),
            sanitize_path_for_logging_escaped(unsafe_component.path(), base_dir)
        ),
        UnsafeMemoPathComponent::InspectionError(_) => tracing::warn!(
            "[markdown-view] {}メモパスの安全確認に失敗したため拒否 ({} -> {}): {}",
            request.read_error_log_label(),
            target.file_label().escape_debug(),
            sanitize_path_for_logging_escaped(memo_path, base_dir),
            sanitize_path_for_logging_escaped(unsafe_component.path(), base_dir)
        ),
    }
}

async fn first_unsafe_memo_path_component(
    base_dir: &Path,
    target: &Path,
) -> Option<UnsafeMemoPathComponent> {
    let relative = match target.strip_prefix(base_dir) {
        Ok(relative) => relative,
        Err(_) => {
            tracing::warn!(
                "[markdown-view] メモパスがbase外のため安全側で拒否します: {} (base: {})",
                sanitize_path_for_logging_escaped(target, base_dir),
                base_dir.display()
            );
            return Some(UnsafeMemoPathComponent::InspectionError(
                target.to_path_buf(),
            ));
        }
    };
    let mut current = base_dir.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match tokio::fs::symlink_metadata(&current).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Some(UnsafeMemoPathComponent::Symlink(current));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] メモパス要素のsymlink検査に失敗したため安全側で拒否します ({}): {}",
                    sanitize_path_for_logging_escaped(&current, base_dir),
                    error
                );
                return Some(UnsafeMemoPathComponent::InspectionError(current));
            }
        }
    }
    None
}

async fn read_memo_file_if_present(
    state: &AppState,
    memo_path: &Path,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    fs: &dyn MemoFs,
) -> Result<Option<String>, ApiError> {
    let before_read = |read_path: &Path| {
        let read_path = read_path.to_path_buf();
        before_access_future(async move {
            checked_safe_memo_read_path(&read_path, state, target, request).await
        })
    };
    let bytes = match fs
        .read_with_limit(memo_path, &before_read as &BeforeAccessCheck<'_>)
        .await
    {
        Ok(bytes) => bytes,
        Err(MemoReadError::Open(error)) | Err(MemoReadError::Read(error))
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(None);
        }
        Err(MemoReadError::BeforeAccess(MemoBeforeRenameError::Missing)) => return Ok(None),
        Err(error) => return Err(memo_read_error_to_api_error(target, request, error)),
    };
    String::from_utf8(bytes).map(Some).map_err(|error| {
        tracing::warn!(
            "[markdown-view] {}メモUTF-8デコード失敗 ({}): {}",
            request.read_error_log_label(),
            target.file_label().escape_debug(),
            error
        );
        json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "メモはUTF-8テキストである必要があります",
        )
    })
}

async fn checked_safe_memo_read_path(
    memo_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<CheckedMemoPath, MemoBeforeRenameError> {
    let checked = checked_current_memo_root_for_memo_rename(state, target, request).await?;
    ensure_current_memo_target_parent_for_memo_rename(target, request).await?;
    ensure_safe_memo_rename_path(memo_path, state, target, request).await?;
    let memo_parent = memo_path
        .parent()
        .ok_or_else(|| MemoBeforeRenameError::internal("メモ保存先の安全確認に失敗しました"))?;
    let relative_parent = memo_parent
        .strip_prefix(checked.root_path.as_path())
        .map_err(|_| MemoBeforeRenameError::internal("メモ保存先の安全確認に失敗しました"))?;
    let file_name = memo_path
        .file_name()
        .ok_or_else(|| MemoBeforeRenameError::internal("メモ保存先の安全確認に失敗しました"))?;
    let parent_dir =
        open_relative_dir_nofollow(&checked.root_dir, relative_parent).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                MemoBeforeRenameError::missing()
            } else {
                MemoBeforeRenameError::internal(error.to_string())
            }
        })?;
    Ok(CheckedMemoPath::new(parent_dir, PathBuf::from(file_name)))
}

fn memo_write_error_to_api_error(
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    error: MemoWriteError,
) -> ApiError {
    match error {
        MemoWriteError::Io(error) => io_api_error(target, request, "保存", error),
        MemoWriteError::BeforeRename(error) => {
            tracing::warn!(
                "[markdown-view] {}メモrename直前検証エラー ({}): {}",
                request.read_error_log_label(),
                target.file_label().escape_debug(),
                error.user_message()
            );
            json_error(error.status_code(), error.user_message())
        }
    }
}

fn memo_before_remove_error_to_api_error(
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    action: &str,
    error: MemoBeforeRenameError,
) -> ApiError {
    tracing::warn!(
        "[markdown-view] {}メモ{}直前検証エラー ({}): {}",
        request.read_error_log_label(),
        action,
        target.file_label().escape_debug(),
        error.user_message()
    );
    json_error(error.status_code(), error.user_message())
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
        target.file_label().escape_debug(),
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
        target.file_label().escape_debug(),
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
        MemoReadError::BeforeAccess(error) => {
            memo_before_remove_error_to_api_error(target, request, "読み込み", error)
        }
        MemoReadError::TooLarge => json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ),
    }
}

#[cfg(test)]
mod unsafe_memo_path_component_tests {
    use super::{first_unsafe_memo_path_component, UnsafeMemoPathComponent};

    #[tokio::test]
    async fn test_first_unsafe_memo_path_component_メタデータエラーは安全側で拒否する() {
        let dir = tempfile::tempdir().expect("tempdirを作成できる");
        let blocking_file = dir.path().join("blocked");
        std::fs::write(&blocking_file, b"not a directory").expect("検査用ファイルを作成できる");
        let target = blocking_file.join("memo.md");

        match first_unsafe_memo_path_component(dir.path(), &target).await {
            Some(UnsafeMemoPathComponent::InspectionError(path)) => assert_eq!(path, target),
            _ => panic!("メタデータエラーは安全確認失敗として返すべき"),
        }
    }

    #[tokio::test]
    async fn test_first_unsafe_memo_path_component_base外pathは安全確認失敗として拒否する() {
        let base = tempfile::tempdir().expect("base tempdirを作成できる");
        let outside = tempfile::tempdir().expect("outside tempdirを作成できる");
        let target = outside.path().join("memo.md");

        match first_unsafe_memo_path_component(base.path(), &target).await {
            Some(UnsafeMemoPathComponent::InspectionError(path)) => assert_eq!(path, target),
            _ => panic!("base外pathは安全確認失敗として返すべき"),
        }
    }
}
