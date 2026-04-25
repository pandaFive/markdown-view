use std::path::{Path, PathBuf};

use axum::http::StatusCode;

use super::content::{read_bytes_with_limit, ReadMarkdownError, MAX_FILE_SIZE};
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
        ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
        delete_memo_file_if_exists(&memo_paths.sidecar, target, request).await?;
        if let Some(compat_sidecar) = &memo_paths.compat_sidecar {
            delete_compat_sidecar_if_safe(state, target, request, compat_sidecar).await?;
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
    let memo_path = &memo_paths.sidecar;
    ensure_safe_memo_path(memo_path, state, target, request)?;
    if let Some(parent) = memo_path.parent() {
        if let Err(error) = tokio::fs::create_dir_all(parent).await {
            if let Some(fallback) = sidecar_fallback_for_error(save_target, &error) {
                return save_memo_to_fallback(state, target, request, &memo_paths, fallback, raw)
                    .await;
            }
            return Err(io_api_error(target, request, "ディレクトリ作成", error));
        }
    }
    if let Err(error) = tokio::fs::write(memo_path, raw.as_bytes()).await {
        if let Some(fallback) = sidecar_fallback_for_error(save_target, &error) {
            return save_memo_to_fallback(state, target, request, &memo_paths, fallback, raw).await;
        }
        return Err(io_api_error(target, request, "保存", error));
    }

    if save_target.delete_legacy_after_save {
        cleanup_legacy_memo_if_safe(state, target, request, &memo_paths.legacy).await?;
    }
    if save_target.delete_compat_after_save {
        if let Some(compat_sidecar) = &memo_paths.compat_sidecar {
            cleanup_compat_sidecar_if_safe(state, target, request, compat_sidecar).await?;
        }
    }

    Ok(MemoResponse::from_raw(
        raw,
        target.relative_path().map(ToOwned::to_owned),
    ))
}

#[derive(Debug, Clone)]
struct MemoPaths {
    sidecar: PathBuf,
    compat_sidecar: Option<PathBuf>,
    legacy: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct SaveTarget {
    fallback: SidecarFallback,
    delete_legacy_after_save: bool,
    delete_compat_after_save: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarFallback {
    None,
    Legacy,
    Compat,
    CompatThenLegacy,
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
) -> Result<PathBuf, ApiError> {
    ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
    if tokio::fs::try_exists(&memo_paths.sidecar)
        .await
        .map_err(|error| io_api_error(target, request, "存在確認", error))?
    {
        return Ok(memo_paths.sidecar.clone());
    }
    if compat_sidecar_exists(state, target, request, memo_paths).await? {
        return Ok(memo_paths
            .compat_sidecar
            .clone()
            .expect("compat path exists"));
    }

    match inspect_legacy_memo(state, target, request, &memo_paths.legacy).await? {
        LegacyMemoState::SafeExists => Ok(memo_paths.legacy.clone()),
        LegacyMemoState::SafeMissing | LegacyMemoState::Unsafe => Ok(memo_paths.sidecar.clone()),
    }
}

async fn choose_save_target(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
) -> Result<SaveTarget, ApiError> {
    ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
    if tokio::fs::try_exists(&memo_paths.sidecar)
        .await
        .map_err(|error| io_api_error(target, request, "存在確認", error))?
    {
        return Ok(SaveTarget {
            fallback: SidecarFallback::None,
            delete_legacy_after_save: false,
            delete_compat_after_save: false,
        });
    }
    let compat_sidecar_exists = compat_sidecar_exists(state, target, request, memo_paths).await?;
    let legacy_state = inspect_legacy_memo(state, target, request, &memo_paths.legacy).await?;
    if compat_sidecar_exists {
        return Ok(SaveTarget {
            fallback: if legacy_state == LegacyMemoState::SafeExists {
                SidecarFallback::CompatThenLegacy
            } else {
                SidecarFallback::Compat
            },
            delete_legacy_after_save: false,
            delete_compat_after_save: true,
        });
    }

    if matches!(
        legacy_state,
        LegacyMemoState::SafeMissing | LegacyMemoState::Unsafe
    ) {
        return Ok(SaveTarget {
            fallback: if state.mode().is_directory() {
                SidecarFallback::Legacy
            } else {
                SidecarFallback::None
            },
            delete_legacy_after_save: false,
            delete_compat_after_save: false,
        });
    }

    Ok(SaveTarget {
        fallback: SidecarFallback::Legacy,
        delete_legacy_after_save: true,
        delete_compat_after_save: false,
    })
}

async fn compat_sidecar_exists(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
) -> Result<bool, ApiError> {
    let Some(compat_sidecar) = &memo_paths.compat_sidecar else {
        return Ok(false);
    };
    if let Err(error) = ensure_safe_memo_path(compat_sidecar, state, target, request) {
        tracing::warn!(
            "[markdown-view] {}unsafeな互換sidecarメモは未使用扱いにします ({}): {:?}",
            request.read_error_log_label(),
            target.file_label(),
            error
        );
        return Ok(false);
    }
    tokio::fs::try_exists(compat_sidecar)
        .await
        .map_err(|error| io_api_error(target, request, "存在確認", error))
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

fn sidecar_fallback_for_error(
    save_target: SaveTarget,
    error: &std::io::Error,
) -> Option<SidecarFallback> {
    if error.kind() != std::io::ErrorKind::PermissionDenied && !is_name_too_long_error(error) {
        return None;
    }
    if save_target.fallback != SidecarFallback::None {
        Some(save_target.fallback)
    } else {
        None
    }
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

async fn save_memo_to_fallback(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fallback: SidecarFallback,
    raw: String,
) -> Result<MemoResponse, ApiError> {
    match fallback {
        SidecarFallback::Legacy => {
            save_memo_to_legacy(state, target, request, &memo_paths.legacy, raw).await
        }
        SidecarFallback::Compat => {
            let Some(compat_sidecar) = &memo_paths.compat_sidecar else {
                return Err(json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "メモファイルの操作に失敗しました",
                ));
            };
            save_memo_to_existing_compat(state, target, request, compat_sidecar, raw).await
        }
        SidecarFallback::CompatThenLegacy => {
            let Some(compat_sidecar) = &memo_paths.compat_sidecar else {
                return save_memo_to_legacy(state, target, request, &memo_paths.legacy, raw).await;
            };
            save_memo_to_existing_compat_or_legacy(
                state,
                target,
                request,
                compat_sidecar,
                &memo_paths.legacy,
                raw,
            )
            .await
        }
        SidecarFallback::None => Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "メモファイルの操作に失敗しました",
        )),
    }
}

async fn save_memo_to_existing_compat_or_legacy(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    compat_sidecar: &Path,
    legacy_path: &Path,
    raw: String,
) -> Result<MemoResponse, ApiError> {
    ensure_safe_memo_path(compat_sidecar, state, target, request)?;
    match tokio::fs::write(compat_sidecar, raw.as_bytes()).await {
        Ok(()) => Ok(MemoResponse::from_raw(
            raw,
            target.relative_path().map(ToOwned::to_owned),
        )),
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || is_name_too_long_error(&error) =>
        {
            save_memo_to_legacy(state, target, request, legacy_path, raw).await
        }
        Err(error) => Err(io_api_error(target, request, "保存", error)),
    }
}

async fn save_memo_to_existing_compat(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    compat_sidecar: &Path,
    raw: String,
) -> Result<MemoResponse, ApiError> {
    ensure_safe_memo_path(compat_sidecar, state, target, request)?;
    tokio::fs::write(compat_sidecar, raw.as_bytes())
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

async fn cleanup_compat_sidecar_if_safe(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    compat_sidecar: &Path,
) -> Result<(), ApiError> {
    if let Err(error) = delete_compat_sidecar_if_safe(state, target, request, compat_sidecar).await
    {
        tracing::warn!(
            "[markdown-view] {}互換sidecarメモcleanup失敗を無視します ({}): {:?}",
            request.read_error_log_label(),
            target.file_label(),
            error
        );
    }
    Ok(())
}

async fn delete_compat_sidecar_if_safe(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    compat_sidecar: &Path,
) -> Result<(), ApiError> {
    if let Err(error) = ensure_safe_memo_path(compat_sidecar, state, target, request) {
        tracing::warn!(
            "[markdown-view] {}unsafeな互換sidecarメモは削除せず無視します ({}): {:?}",
            request.read_error_log_label(),
            target.file_label(),
            error
        );
        return Ok(());
    }
    delete_memo_file_if_exists(compat_sidecar, target, request).await
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
