use std::path::{Path, PathBuf};

use axum::http::StatusCode;

use super::content::MAX_FILE_SIZE;
use super::memo_fs::{
    before_rename_future, BeforeRenameCheck, MemoBeforeRenameError, MemoFs, MemoReadError,
    MemoWriteError,
};
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
    ensure_safe_memo_path(memo_path, state, target, request).await?;
    if let Some(parent) = memo_path.parent() {
        fs.create_dir_all(parent)
            .await
            .map_err(|error| io_api_error(target, request, "ディレクトリ作成", error))?;
    }
    let before_rename = |final_path: &Path, tmp_path: &Path| {
        let final_path = final_path.to_path_buf();
        let tmp_path = tmp_path.to_path_buf();
        before_rename_future(async move {
            ensure_safe_memo_rename_paths(&final_path, &tmp_path, state, target, request).await
        })
    };
    fs.write_atomic(
        memo_path,
        raw.as_bytes(),
        &before_rename as &BeforeRenameCheck<'_>,
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
    ensure_safe_memo_path(&memo_paths.sidecar, state, target, request).await?;

    cleanup_compat_sidecar_required(state, target, request, memo_paths, fs).await?;
    cleanup_legacy_memo_required(state, target, request, &memo_paths.legacy, fs).await?;

    match fs.remove_file(&memo_paths.sidecar).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_api_error(target, request, "削除", error)),
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
        pick_existing_safe_path(state, target, request, path, label, unsafe_path, fs).await?
    else {
        return Ok(None);
    };

    read_memo_file_if_present(&path, target, request, fs).await
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
    fs: &dyn MemoFs,
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
            target.file_label(),
            error
        );
        return match unsafe_path {
            UnsafeMemoPath::Reject => Err(error),
            UnsafeMemoPath::Skip => Ok(None),
        };
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
                target.file_label(),
                error
            );
            return Err(error);
        }
        tracing::warn!(
            "[markdown-view] {}unsafeな{}メモは削除せず無視します ({}): {:?}",
            request.read_error_log_label(),
            label,
            target.file_label(),
            error
        );
        return Ok(());
    }

    match fs.remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_api_error(target, request, "cleanup削除", error)),
    }
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

async fn ensure_safe_memo_rename_paths(
    final_path: &Path,
    tmp_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), MemoBeforeRenameError> {
    let base_dir = state.mode().base_dir();
    if final_path.parent() != tmp_path.parent() {
        tracing::warn!(
            "[markdown-view] {}メモ一時ファイルが保存先と同一ディレクトリではないため拒否 ({}): {} -> {}",
            request.read_error_log_label(),
            target.file_label(),
            sanitize_path_for_logging(final_path, base_dir),
            sanitize_path_for_logging(tmp_path, base_dir)
        );
        return Err(MemoBeforeRenameError::new(
            "メモ保存の内部状態が不正なため操作を中止しました",
        ));
    }

    ensure_safe_memo_rename_path(final_path, state, target, request).await?;
    ensure_safe_memo_rename_path(tmp_path, state, target, request).await?;
    Ok(())
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
            target.file_label(),
            sanitize_path_for_logging(memo_path, base_dir),
            sanitize_path_for_logging(unsafe_component.path(), base_dir)
        ),
        UnsafeMemoPathComponent::InspectionError(_) => tracing::warn!(
            "[markdown-view] {}メモパスの安全確認に失敗したため拒否 ({} -> {}): {}",
            request.read_error_log_label(),
            target.file_label(),
            sanitize_path_for_logging(memo_path, base_dir),
            sanitize_path_for_logging(unsafe_component.path(), base_dir)
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
                sanitize_path_for_logging(target, base_dir),
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
                    sanitize_path_for_logging(&current, base_dir),
                    error
                );
                return Some(UnsafeMemoPathComponent::InspectionError(current));
            }
        }
    }
    None
}

async fn read_memo_file_if_present(
    memo_path: &Path,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    fs: &dyn MemoFs,
) -> Result<Option<String>, ApiError> {
    let metadata = match fs.metadata(memo_path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_api_error(target, request, "メタデータ取得", error)),
    };
    if metadata.len() > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }

    let bytes = match fs.read_with_limit(memo_path).await {
        Ok(bytes) => bytes,
        Err(MemoReadError::Open(error)) | Err(MemoReadError::Read(error))
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(None);
        }
        Err(error) => return Err(memo_read_error_to_api_error(target, request, error)),
    };
    if bytes.len() as u64 > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }
    String::from_utf8(bytes).map(Some).map_err(|error| {
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
                target.file_label(),
                error.user_message()
            );
            json_error(error.status_code(), error.user_message())
        }
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
