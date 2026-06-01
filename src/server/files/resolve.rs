use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};

use axum::http::StatusCode;

use super::catalog::{
    list_markdown_files_from_canonical_base, open_relative_dir_nofollow,
    open_relative_file_nofollow, open_verified_base_dir,
};
use super::run_blocking_file_task;
use crate::server::guards::json_error;
use crate::server::log_path::{sanitize_path_for_logging, sanitize_path_for_logging_escaped};
use crate::server::messages::ApiError;
use crate::server::state::{AppMode, AppState, CanonicalPath};
use crate::template::UpdateMessage;
use crate::workspace_exclusion::exclusion_reason_for_relative_path;

#[cfg(test)]
type SingleFileAfterParentVerificationHook = std::sync::Arc<dyn Fn(&Path) + Send + Sync + 'static>;

#[cfg(test)]
static SINGLE_FILE_AFTER_PARENT_VERIFICATION_HOOK: std::sync::OnceLock<
    std::sync::Mutex<Option<SingleFileAfterParentVerificationHook>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
static SINGLE_FILE_AFTER_PARENT_VERIFICATION_HOOK_TEST_LOCK: std::sync::OnceLock<
    std::sync::Mutex<()>,
> = std::sync::OnceLock::new();

#[cfg(test)]
pub(in crate::server) struct SingleFileAfterParentVerificationHookGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for SingleFileAfterParentVerificationHookGuard {
    fn drop(&mut self) {
        SINGLE_FILE_AFTER_PARENT_VERIFICATION_HOOK
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .expect("single file hook mutex should not be poisoned")
            .take();
    }
}

#[cfg(test)]
pub(in crate::server) fn set_single_file_after_parent_verification_hook_for_test(
    hook: SingleFileAfterParentVerificationHook,
) -> SingleFileAfterParentVerificationHookGuard {
    let lock = SINGLE_FILE_AFTER_PARENT_VERIFICATION_HOOK_TEST_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .expect("single file hook test lock should not be poisoned");
    SINGLE_FILE_AFTER_PARENT_VERIFICATION_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("single file hook mutex should not be poisoned")
        .replace(hook);
    SingleFileAfterParentVerificationHookGuard { _lock: lock }
}

#[cfg(test)]
fn notify_single_file_after_parent_verification_for_test(file_path: &Path) {
    let hook = SINGLE_FILE_AFTER_PARENT_VERIFICATION_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("single file hook mutex should not be poisoned")
        .clone();
    if let Some(hook) = hook {
        hook(file_path);
    }
}

#[cfg(not(test))]
fn notify_single_file_after_parent_verification_for_test(_file_path: &Path) {}

#[cfg(test)]
type DirectoryFileAfterOpenHook = std::sync::Arc<dyn Fn(&Path) + Send + Sync + 'static>;

#[cfg(test)]
static DIRECTORY_FILE_AFTER_OPEN_HOOK: std::sync::OnceLock<
    std::sync::Mutex<Option<DirectoryFileAfterOpenHook>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
static DIRECTORY_FILE_AFTER_OPEN_HOOK_TEST_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
    std::sync::OnceLock::new();

#[cfg(test)]
pub(in crate::server) struct DirectoryFileAfterOpenHookGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for DirectoryFileAfterOpenHookGuard {
    fn drop(&mut self) {
        DIRECTORY_FILE_AFTER_OPEN_HOOK
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .expect("directory file hook mutex should not be poisoned")
            .take();
    }
}

#[cfg(test)]
pub(in crate::server) fn set_directory_file_after_open_hook_for_test(
    hook: DirectoryFileAfterOpenHook,
) -> DirectoryFileAfterOpenHookGuard {
    let lock = DIRECTORY_FILE_AFTER_OPEN_HOOK_TEST_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .expect("directory file hook test lock should not be poisoned");
    DIRECTORY_FILE_AFTER_OPEN_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("directory file hook mutex should not be poisoned")
        .replace(hook);
    DirectoryFileAfterOpenHookGuard { _lock: lock }
}

#[cfg(test)]
fn notify_directory_file_after_open_for_test(file_path: &Path) {
    let hook = DIRECTORY_FILE_AFTER_OPEN_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("directory file hook mutex should not be poisoned")
        .clone();
    if let Some(hook) = hook {
        hook(file_path);
    }
}

#[cfg(not(test))]
fn notify_directory_file_after_open_for_test(_file_path: &Path) {}

#[derive(Debug, Clone)]
/// ファイル解決結果。ターゲットファイルのパス、ファイル一覧、相対パス、表示用ラベルを保持する。
pub(in crate::server) struct ResolvedTarget {
    file_path: PathBuf,
    file_list: Option<Vec<String>>,
    relative_path: Option<String>,
    file_label: String,
    read_file: Option<std::sync::Arc<std::fs::File>>,
    target_parent: Option<CanonicalPath>,
}

impl ResolvedTarget {
    fn new(
        file_path: PathBuf,
        file_list: Option<Vec<String>>,
        relative_path: Option<String>,
        read_file: Option<std::fs::File>,
        target_parent: Option<CanonicalPath>,
    ) -> Self {
        let file_label = relative_path
            .clone()
            .unwrap_or_else(|| file_display_name(&file_path));

        Self {
            file_path,
            file_list,
            relative_path,
            file_label,
            read_file: read_file.map(std::sync::Arc::new),
            target_parent,
        }
    }

    #[cfg(test)]
    pub(in crate::server) fn for_test(
        file_path: PathBuf,
        file_list: Option<Vec<String>>,
        relative_path: Option<String>,
    ) -> Self {
        Self::new(file_path, file_list, relative_path, None, None)
    }

    pub(in crate::server) fn file_path(&self) -> &Path {
        &self.file_path
    }

    pub(in crate::server) fn file_list(&self) -> Option<&[String]> {
        self.file_list.as_deref()
    }

    pub(in crate::server) fn relative_path(&self) -> Option<&str> {
        self.relative_path.as_deref()
    }

    pub(in crate::server) fn file_label(&self) -> &str {
        &self.file_label
    }

    pub(in crate::server) fn read_file(&self) -> std::io::Result<Option<std::fs::File>> {
        let Some(file) = &self.read_file else {
            return Ok(None);
        };
        let mut file = file.try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        Ok(Some(file))
    }

    pub(in crate::server) fn target_parent_canonical(&self) -> Option<&CanonicalPath> {
        self.target_parent.as_ref()
    }

    pub(super) fn attach_file_info(&self, update: UpdateMessage) -> UpdateMessage {
        update.with_file(self.relative_path.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::server) struct RouteTargetRequest<'a> {
    kind: RouteTargetKind,
    query_file: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::server) enum RouteTargetKind {
    Page,
    ApiContent,
    ApiMemo,
}

impl<'a> RouteTargetRequest<'a> {
    pub(in crate::server) fn page(query_file: Option<&'a str>) -> Self {
        Self {
            kind: RouteTargetKind::Page,
            query_file,
        }
    }

    pub(in crate::server) fn api_content(query_file: Option<&'a str>) -> Self {
        Self {
            kind: RouteTargetKind::ApiContent,
            query_file,
        }
    }

    pub(in crate::server) fn api_memo(query_file: Option<&'a str>) -> Self {
        Self {
            kind: RouteTargetKind::ApiMemo,
            query_file,
        }
    }

    pub(in crate::server) fn query_file(self) -> Option<&'a str> {
        self.query_file
    }

    #[cfg(test)]
    pub(in crate::server) fn kind(self) -> RouteTargetKind {
        self.kind
    }

    fn include_file_list(self) -> bool {
        match self.kind {
            RouteTargetKind::Page => true,
            RouteTargetKind::ApiContent | RouteTargetKind::ApiMemo => false,
        }
    }

    fn not_found_message(self) -> &'static str {
        match self.kind {
            RouteTargetKind::Page => "表示可能なMarkdownファイルが見つかりません",
            RouteTargetKind::ApiContent | RouteTargetKind::ApiMemo => {
                "指定したファイルが見つかりません"
            }
        }
    }

    pub(in crate::server) fn read_error_log_label(self) -> &'static str {
        match self.kind {
            RouteTargetKind::Page => "index",
            RouteTargetKind::ApiContent => "api/content",
            RouteTargetKind::ApiMemo => "api/memo",
        }
    }
}

/// 対象ファイル解決エラーをエンドポイント文脈に応じたAPIエラーへ変換する。
pub(in crate::server) async fn resolve_route_target(
    state: &AppState,
    request: RouteTargetRequest<'_>,
) -> Result<ResolvedTarget, ApiError> {
    let (file_path, file_list, read_file, target_parent) = resolve_request_target(state, request)
        .await
        .map_err(|error| json_error(error.status, error.message))?;

    Ok(build_resolved_target(
        state,
        file_path,
        file_list,
        read_file,
        target_parent,
        "ターゲットファイルの相対パス算出失敗",
    ))
}

pub(super) async fn resolve_single_file_target_blocking(
    state: &AppState,
    warn_label: &'static str,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    let mode = state.mode().clone();
    run_blocking_file_task("単一ファイルターゲット解決", move || {
        resolve_single_file_target_from_mode(&mode, warn_label)
    })
    .await
    .map_err(|_| ResolveFileError::InternalState)?
}

#[cfg(test)]
pub(super) fn resolve_change_target(
    state: &AppState,
    changed_file: &Path,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    resolve_change_target_from_mode(state.mode(), changed_file)
}

pub(super) async fn resolve_change_target_blocking(
    state: &AppState,
    changed_file: &Path,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    let mode = state.mode().clone();
    let changed_file = changed_file.to_path_buf();
    run_blocking_file_task("変更ターゲット解決", move || {
        resolve_change_target_from_mode(&mode, &changed_file)
    })
    .await
    .map_err(|_| ResolveFileError::InternalState)?
}

fn resolve_single_file_target_from_mode(
    mode: &AppMode,
    warn_label: &'static str,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    resolve_single_file_handle_from_mode(mode)?.map_or(Ok(None), |handle| {
        Ok(Some(build_resolved_target_for_mode(
            mode,
            handle.path,
            None,
            Some(handle.file),
            Some(handle.parent),
            warn_label,
        )))
    })
}

fn resolve_change_target_from_mode(
    mode: &AppMode,
    changed_file: &Path,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    if let Some(handle) = resolve_single_file_handle_from_mode(mode)? {
        return Ok(Some(build_resolved_target_for_mode(
            mode,
            handle.path,
            None,
            Some(handle.file),
            Some(handle.parent),
            "更新対象の相対パス算出失敗",
        )));
    }

    resolve_directory_change_target(mode, changed_file)
}

async fn resolve_request_target(
    state: &AppState,
    request: RouteTargetRequest<'_>,
) -> Result<
    (
        PathBuf,
        Option<Vec<String>>,
        Option<std::fs::File>,
        Option<CanonicalPath>,
    ),
    ResolveRequestError,
> {
    if state.mode().single_file_canonical().is_some() {
        let handle = resolve_single_file_handle_blocking(state.mode())
            .await
            .map_err(ResolveRequestError::file_resolution_status)?
            .map_err(|error| {
                tracing::warn!("[markdown-view] 単一ファイル解決エラー: {}", error);
                ResolveRequestError::from_file_error(error, request)
            })?;
        let Some(handle) = handle else {
            tracing::error!("[markdown-view] 単一ファイルAppModeの解決結果が空です");
            return Err(ResolveRequestError::file_resolution_status(
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        };
        return Ok((handle.path, None, Some(handle.file), Some(handle.parent)));
    }

    let Some(base_dir) = state.mode().directory_canonical() else {
        tracing::error!("[markdown-view] 未知のAppModeです");
        return Err(ResolveRequestError::file_resolution_status(
            StatusCode::INTERNAL_SERVER_ERROR,
        ));
    };
    let mut precomputed_files = None;
    let (file_path, read_file, target_parent) = if let Some(relative) = request.query_file() {
        let resolved = resolve_file_blocking(base_dir, relative)
            .await
            .map_err(ResolveRequestError::file_resolution_status)?
            .map_err(|error| {
                tracing::warn!("[markdown-view] ファイル解決エラー: {}", error);
                ResolveRequestError::from_file_error(error, request)
            })?;
        (resolved.path, Some(resolved.file), Some(resolved.parent))
    } else {
        let files = list_markdown_files_blocking(base_dir)
            .await
            .map_err(ResolveRequestError::file_list_status)?;
        precomputed_files = Some(files.clone());

        let default_file = files
            .iter()
            .find(|file| file.eq_ignore_ascii_case("readme.md"))
            .or_else(|| files.first());

        match default_file {
            Some(relative) => {
                let resolved = resolve_file_blocking(base_dir, relative)
                    .await
                    .map_err(ResolveRequestError::file_resolution_status)?
                    .map_err(|error| {
                        tracing::warn!("[markdown-view] デフォルトファイル解決エラー: {}", error);
                        ResolveRequestError::file_resolution_status(
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                    })?;
                (resolved.path, Some(resolved.file), Some(resolved.parent))
            }
            None => return Err(ResolveRequestError::not_found(request)),
        }
    };

    let file_list = if request.include_file_list() {
        match precomputed_files {
            Some(files) => Some(files),
            None => Some(
                list_markdown_files_blocking(base_dir)
                    .await
                    .map_err(ResolveRequestError::file_list_status)?,
            ),
        }
    } else {
        None
    };

    Ok((file_path, file_list, read_file, target_parent))
}

#[derive(Debug, Clone, Copy)]
struct ResolveRequestError {
    status: StatusCode,
    message: &'static str,
}

impl ResolveRequestError {
    fn not_found(request: RouteTargetRequest<'_>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: request.not_found_message(),
        }
    }

    fn file_list_status(status: StatusCode) -> Self {
        Self {
            status,
            message: "ファイル一覧の取得に失敗しました",
        }
    }

    fn file_resolution_status(status: StatusCode) -> Self {
        Self {
            status,
            message: "ファイル解決に失敗しました",
        }
    }

    fn from_file_error(error: ResolveFileError, request: RouteTargetRequest<'_>) -> Self {
        let status = error.status_code();
        if status == StatusCode::NOT_FOUND {
            return Self::not_found(request);
        }
        Self::file_resolution_status(status)
    }

    #[cfg(test)]
    pub(in crate::server) fn message(self) -> &'static str {
        self.message
    }

    #[cfg(test)]
    pub(in crate::server) fn status(self) -> StatusCode {
        self.status
    }
}

#[cfg(test)]
pub(in crate::server) fn map_file_resolve_error_for_test(
    error: ResolveFileError,
    request: RouteTargetRequest<'_>,
) -> (StatusCode, &'static str) {
    let error = ResolveRequestError::from_file_error(error, request);
    (error.status(), error.message())
}

async fn list_markdown_files_blocking(base_dir: &CanonicalPath) -> Result<Vec<String>, StatusCode> {
    let base_dir = base_dir.clone();
    run_blocking_file_task("ファイル一覧取得", move || {
        list_markdown_files_from_canonical_base(&base_dir, super::catalog::MAX_FILE_LIST)
    })
    .await?
    .map_err(|error| {
        tracing::warn!("[markdown-view] ファイル一覧取得エラー: {}", error);
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

async fn resolve_file_blocking(
    base_dir: &CanonicalPath,
    relative: &str,
) -> Result<Result<ResolvedFileHandle, ResolveFileError>, StatusCode> {
    let base_dir = base_dir.clone();
    let relative = relative.to_owned();
    run_blocking_file_task("ファイル解決", move || {
        resolve_file_from_canonical_base(&base_dir, &relative)
    })
    .await
}

async fn resolve_single_file_handle_blocking(
    mode: &AppMode,
) -> Result<Result<Option<ResolvedFileHandle>, ResolveFileError>, StatusCode> {
    let mode = mode.clone();
    run_blocking_file_task("単一ファイル解決", move || {
        resolve_single_file_handle_from_mode(&mode)
    })
    .await
}

#[derive(Debug)]
struct ResolvedFileHandle {
    path: PathBuf,
    file: std::fs::File,
    parent: CanonicalPath,
}

fn resolve_single_file_handle_from_mode(
    mode: &AppMode,
) -> Result<Option<ResolvedFileHandle>, ResolveFileError> {
    let Some(file_path) = mode.single_file_canonical() else {
        return Ok(None);
    };
    let parent_dir = mode
        .single_file_parent_canonical()
        .ok_or(ResolveFileError::InternalState)?;
    let parent_handle = open_verified_single_file_parent_dir(parent_dir)?;
    notify_single_file_after_parent_verification_for_test(file_path.as_path());
    let path = revalidate_single_file_target(file_path, mode.base_dir())?;
    let file = open_single_file_for_read(file_path, parent_dir, &parent_handle)?;
    verify_current_single_file_parent_identity(parent_dir)?;
    Ok(Some(ResolvedFileHandle {
        path,
        file,
        parent: parent_dir.clone(),
    }))
}

fn verify_current_single_file_parent_identity(
    parent: &CanonicalPath,
) -> Result<(), ResolveFileError> {
    match parent.has_current_identity() {
        Ok(true) => Ok(()),
        Ok(false) => {
            tracing::warn!(
                "[markdown-view] 単一ファイルparent directoryの実体差し替えを検出しました"
            );
            Err(ResolveFileError::Traversal)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(ResolveFileError::NotFound)
        }
        Err(error) => Err(ResolveFileError::Io(error.kind())),
    }
}

fn resolve_file_from_canonical_base(
    base_dir: &CanonicalPath,
    relative: &str,
) -> Result<ResolvedFileHandle, ResolveFileError> {
    validate_directory_base_identity_for_resolve(base_dir)?;
    let path = resolve_file(base_dir.as_path(), relative)?;
    let relative_path = path
        .strip_prefix(base_dir.as_path())
        .map_err(|_| ResolveFileError::Traversal)?;
    let relative_parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    let file_name = relative_path
        .file_name()
        .ok_or(ResolveFileError::InvalidPath)?;
    let parent = canonical_parent_for_resolved_file(&path)?;
    let verified_base = open_verified_base_dir(base_dir, "ファイル解決base directory")
        .map_err(|error| ResolveFileError::Io(error.kind()))?;
    let parent_dir = open_relative_dir_nofollow(&verified_base, relative_parent)
        .map_err(|error| ResolveFileError::Io(error.kind()))?;
    let parent_metadata = parent_dir
        .dir_metadata()
        .map_err(|error| ResolveFileError::Io(error.kind()))?;
    if !parent.matches_cap_metadata_identity(&parent_metadata) {
        tracing::warn!(
            "[markdown-view] ディレクトリモードtarget parent directoryの実体差し替えを検出しました"
        );
        return Err(ResolveFileError::Traversal);
    }
    let file = match open_relative_file_nofollow(&parent_dir, Path::new(file_name)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ResolveFileError::NotFound);
        }
        Err(error) => return Err(ResolveFileError::Io(error.kind())),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ResolveFileError::NotFound);
        }
        Err(error) => return Err(ResolveFileError::Io(error.kind())),
    };
    if !metadata.is_file() {
        return Err(ResolveFileError::NotFile);
    }
    notify_directory_file_after_open_for_test(&path);
    verify_current_directory_target_parent_identity(&parent)?;
    Ok(ResolvedFileHandle {
        path,
        file: file.into_std(),
        parent,
    })
}

fn verify_current_directory_target_parent_identity(
    parent: &CanonicalPath,
) -> Result<(), ResolveFileError> {
    match parent.has_current_identity() {
        Ok(true) => Ok(()),
        Ok(false) => {
            tracing::warn!(
                "[markdown-view] ディレクトリモードtarget parent directoryの実体差し替えを検出しました"
            );
            Err(ResolveFileError::Traversal)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(ResolveFileError::NotFound)
        }
        Err(error) => Err(ResolveFileError::Io(error.kind())),
    }
}

fn canonical_parent_for_resolved_file(path: &Path) -> Result<CanonicalPath, ResolveFileError> {
    let parent = path.parent().ok_or(ResolveFileError::InvalidPath)?;
    CanonicalPath::try_from_path(parent).map_err(canonical_path_error_to_resolve_error)
}

fn canonical_path_error_to_resolve_error(
    error: crate::server::state::CanonicalPathError,
) -> ResolveFileError {
    match error {
        crate::server::state::CanonicalPathError::Canonicalize(error)
        | crate::server::state::CanonicalPathError::Metadata(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            ResolveFileError::NotFound
        }
        crate::server::state::CanonicalPathError::Canonicalize(error)
        | crate::server::state::CanonicalPathError::Metadata(error) => {
            ResolveFileError::Io(error.kind())
        }
        crate::server::state::CanonicalPathError::UnsupportedIdentity => {
            ResolveFileError::InternalState
        }
    }
}

fn open_single_file_for_read(
    path: &CanonicalPath,
    parent: &CanonicalPath,
    parent_dir: &cap_std::fs::Dir,
) -> Result<std::fs::File, ResolveFileError> {
    let path_parent = path
        .as_path()
        .parent()
        .ok_or(ResolveFileError::InvalidPath)?;
    if path_parent != parent.as_path() {
        return Err(ResolveFileError::InternalState);
    }
    let file_name = path
        .as_path()
        .file_name()
        .ok_or(ResolveFileError::InvalidPath)?;
    let file = match open_relative_file_nofollow(parent_dir, Path::new(file_name)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ResolveFileError::NotFound);
        }
        Err(error) => return Err(ResolveFileError::Io(error.kind())),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ResolveFileError::NotFound);
        }
        Err(error) => return Err(ResolveFileError::Io(error.kind())),
    };
    if !metadata.is_file() {
        return Err(ResolveFileError::NotFile);
    }
    Ok(file.into_std())
}

fn open_verified_single_file_parent_dir(
    parent: &CanonicalPath,
) -> Result<cap_std::fs::Dir, ResolveFileError> {
    let parent_dir =
        cap_std::fs::Dir::open_ambient_dir(parent.as_path(), cap_std::ambient_authority())
            .map_err(|error| ResolveFileError::Io(error.kind()))?;
    let metadata = parent_dir
        .dir_metadata()
        .map_err(|error| ResolveFileError::Io(error.kind()))?;
    if parent.matches_cap_metadata_identity(&metadata) {
        return Ok(parent_dir);
    }

    tracing::warn!("[markdown-view] 単一ファイルparent directoryの実体差し替えを検出しました");
    Err(ResolveFileError::Traversal)
}

fn validate_directory_base_identity_for_resolve(
    base_dir: &CanonicalPath,
) -> Result<(), ResolveFileError> {
    match base_dir.has_current_identity() {
        Ok(true) => Ok(()),
        Ok(false) => {
            tracing::warn!(
                "[markdown-view] ファイル解決base directory pathの実体差し替えを検出しました"
            );
            Err(ResolveFileError::Traversal)
        }
        Err(error) => {
            tracing::warn!(
                "[markdown-view] ファイル解決base directory pathの実体検証に失敗しました: {}",
                error
            );
            if error.kind() == std::io::ErrorKind::NotFound {
                Err(ResolveFileError::NotFound)
            } else {
                Err(ResolveFileError::Io(error.kind()))
            }
        }
    }
}

fn build_resolved_target(
    state: &AppState,
    file_path: PathBuf,
    file_list: Option<Vec<String>>,
    read_file: Option<std::fs::File>,
    target_parent: Option<CanonicalPath>,
    warn_label: &'static str,
) -> ResolvedTarget {
    build_resolved_target_for_mode(
        state.mode(),
        file_path,
        file_list,
        read_file,
        target_parent,
        warn_label,
    )
}

fn build_resolved_target_for_mode(
    mode: &AppMode,
    file_path: PathBuf,
    file_list: Option<Vec<String>>,
    read_file: Option<std::fs::File>,
    target_parent: Option<CanonicalPath>,
    warn_label: &'static str,
) -> ResolvedTarget {
    let relative_path = mode.relative_path_of(&file_path);
    if mode.is_directory() && relative_path.is_none() {
        tracing::warn!(
            "[markdown-view] {}: {} はベース {} の配下ではありません",
            warn_label,
            sanitize_path_for_logging(&file_path, mode.base_dir()),
            mode.base_dir().display()
        );
        // 相対パス算出失敗時の方針（呼び出し経路ごとに後段で扱いを変える）:
        // - 本関数は警告ログのみで描画継続を許容する（graceful degradation）
        // - HTTP経路: サイドバーのハイライトが落ちるだけで本文描画は継続
        // - WebSocket変更通知経路: 変更ターゲット解決時の再検証後にここへ到達したら内部不整合
    }

    ResolvedTarget::new(
        file_path,
        file_list,
        relative_path,
        read_file,
        target_parent,
    )
}

fn resolve_directory_change_target(
    mode: &AppMode,
    changed_file: &Path,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    let Some(base_dir) = mode.directory_canonical() else {
        tracing::error!("[markdown-view] 未知のAppModeです");
        return Err(ResolveFileError::InternalState);
    };

    let relative = relative_change_path(base_dir.as_path(), changed_file)?;
    let relative_string = relative_change_path_to_query(&relative)?;
    let resolved = resolve_file_from_canonical_base(base_dir, &relative_string)?;
    Ok(Some(build_resolved_target_for_mode(
        mode,
        resolved.path,
        None,
        Some(resolved.file),
        Some(resolved.parent),
        "更新対象の相対パス算出失敗",
    )))
}

fn relative_change_path_to_query(relative: &Path) -> Result<String, ResolveFileError> {
    relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or(ResolveFileError::InvalidPath)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|components| components.join("/"))
}

fn relative_change_path(base_dir: &Path, changed_file: &Path) -> Result<PathBuf, ResolveFileError> {
    if let Ok(relative) = changed_file.strip_prefix(base_dir) {
        return Ok(relative.to_path_buf());
    }

    let canonical_base = base_dir.canonicalize().map_err(|error| {
        let error_kind = error.kind();
        tracing::warn!(
            "[markdown-view] watcher変更ターゲット: ベース正規化失敗: {} ({})",
            base_dir.display(),
            error
        );
        resolve_canonicalize_error(error_kind)
    })?;

    let canonical_changed = changed_file.canonicalize().map_err(|error| {
        let error_kind = error.kind();
        tracing::warn!(
            "[markdown-view] watcher変更ターゲット: パス正規化失敗: {} ({})",
            sanitize_path_for_logging(changed_file, base_dir),
            error
        );
        resolve_canonicalize_error(error_kind)
    })?;

    canonical_changed
        .strip_prefix(&canonical_base)
        .map(Path::to_path_buf)
        .map_err(|_| ResolveFileError::Traversal)
}

fn resolve_canonicalize_error(error_kind: std::io::ErrorKind) -> ResolveFileError {
    if error_kind == std::io::ErrorKind::NotFound {
        ResolveFileError::NotFound
    } else {
        ResolveFileError::Io(error_kind)
    }
}

/// 相対パスを安全に解決する（ディレクトリトラバーサル防止）
pub fn resolve_file(base_dir: &Path, relative: &str) -> Result<PathBuf, ResolveFileError> {
    resolve_file_with_canonicalize_error(base_dir, relative, resolve_canonicalize_error)
}

fn resolve_file_with_canonicalize_error(
    base_dir: &Path,
    relative: &str,
    map_canonicalize_error: fn(std::io::ErrorKind) -> ResolveFileError,
) -> Result<PathBuf, ResolveFileError> {
    if relative.is_empty() {
        return Err(ResolveFileError::EmptyPath);
    }
    if relative.contains('\0') {
        return Err(ResolveFileError::InvalidPath);
    }

    let relative_path = Path::new(relative);
    if relative_path.is_absolute() {
        return Err(ResolveFileError::InvalidPath);
    }
    if exclusion_reason_for_relative_path(relative_path).is_some() {
        return Err(ResolveFileError::Hidden);
    }

    let candidate = base_dir.join(relative_path);
    let canonical = candidate.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] ファイルパス正規化失敗: {} ({})",
            sanitize_path_for_logging_escaped(&candidate, base_dir),
            error
        );
        map_canonicalize_error(error.kind())
    })?;

    let canonical_base = base_dir.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] ベースディレクトリ正規化失敗: {} ({})",
            base_dir.display(),
            error
        );
        map_canonicalize_error(error.kind())
    })?;
    if !canonical.starts_with(&canonical_base) {
        return Err(ResolveFileError::Traversal);
    }

    if let Ok(resolved_relative) = canonical.strip_prefix(&canonical_base) {
        if exclusion_reason_for_relative_path(resolved_relative).is_some() {
            return Err(ResolveFileError::Hidden);
        }
    }

    if !canonical.is_file() {
        return Err(ResolveFileError::NotFile);
    }

    match canonical.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("md") => Ok(canonical),
        _ => Err(ResolveFileError::NotMarkdown),
    }
}

/// 単一ファイルモードの対象ファイルpath制約を再検証する。
///
/// 起動時に正規化したパスと現在のパスを比較し、正規化後のパスが起動時と
/// 異なる場合だけトラバーサルとして拒否する。この関数単体ではtarget実体の
/// 同一性を固定しない。実体の安全性は、呼び出し側で検証済み親ディレクトリ
/// からの`nofollow` openと親ディレクトリidentity再確認を組み合わせて担保する。
pub(super) fn revalidate_single_file_target(
    expected_path: &CanonicalPath,
    base_dir: &Path,
) -> Result<PathBuf, ResolveFileError> {
    let canonical = expected_path.as_path().canonicalize().map_err(|error| {
        let error_kind = error.kind();
        tracing::warn!(
            "[markdown-view] 単一ファイルパス正規化失敗: {} ({:?}: {})",
            sanitize_path_for_logging(expected_path.as_path(), base_dir),
            error_kind,
            error
        );
        resolve_canonicalize_error(error_kind)
    })?;

    if canonical != expected_path.as_path() {
        return Err(ResolveFileError::Traversal);
    }
    if !canonical.is_file() {
        return Err(ResolveFileError::NotFile);
    }

    match canonical.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("md") => Ok(canonical),
        _ => Err(ResolveFileError::NotMarkdown),
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
    /// 通常ファイルではない
    NotFile,
    /// ディレクトリトラバーサル検出
    Traversal,
    /// Markdownファイルではない
    NotMarkdown,
    /// 隠しファイルまたは除外対象へのアクセス
    Hidden,
    /// watcher再検証中の一時不在ではないI/O失敗
    Io(std::io::ErrorKind),
    /// AppModeの内部不整合
    InternalState,
}

impl std::fmt::Display for ResolveFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveFileError::EmptyPath => write!(f, "ファイルパスが空です"),
            ResolveFileError::InvalidPath => write!(f, "無効なパスです"),
            ResolveFileError::NotFound => write!(f, "ファイルが見つかりません"),
            ResolveFileError::NotFile => write!(f, "通常ファイルではありません"),
            ResolveFileError::Traversal => {
                write!(f, "安全でないファイル参照は禁止されています")
            }
            ResolveFileError::NotMarkdown => write!(f, ".mdファイルのみアクセス可能です"),
            ResolveFileError::Hidden => write!(
                f,
                "隠しファイルまたは除外対象へのアクセスは禁止されています"
            ),
            ResolveFileError::Io(kind) => {
                write!(f, "ファイル解決中にI/Oエラーが発生しました ({kind:?})")
            }
            ResolveFileError::InternalState => write!(f, "内部状態が不整合です"),
        }
    }
}

impl std::error::Error for ResolveFileError {}

impl ResolveFileError {
    /// 外部入力由来の解決失敗は404へマスクし、内部状態不整合だけ500へ分離する。
    pub fn status_code(&self) -> StatusCode {
        match self {
            ResolveFileError::InternalState => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::NOT_FOUND,
        }
    }

    pub(in crate::server) fn user_message(&self) -> &'static str {
        match self {
            ResolveFileError::EmptyPath => "ファイルパスが空です",
            ResolveFileError::InvalidPath => "無効なパスです",
            ResolveFileError::NotFound => "ファイルが見つかりません",
            ResolveFileError::NotFile => "通常ファイルではありません",
            ResolveFileError::Traversal => "安全でないファイル参照は禁止されています",
            ResolveFileError::NotMarkdown => ".mdファイルのみアクセス可能です",
            ResolveFileError::Hidden => "隠しファイルまたは除外対象へのアクセスは禁止されています",
            ResolveFileError::Io(_) => "ファイルの検証に失敗しました",
            ResolveFileError::InternalState => "内部エラーが発生しました",
        }
    }
}

/// パスから表示用ラベルを生成する（`file_name` があればそれ、なければ `display()` フォールバック）
pub(super) fn file_display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
