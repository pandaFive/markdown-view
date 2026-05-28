use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use cap_primitives::fs::{open_dir_nofollow, FollowSymlinks};
use cap_std::fs::{Dir, File as CapFile, OpenOptions as CapOpenOptions};

use crate::server::log_path::sanitize_path_for_logging_escaped;
#[cfg(test)]
use crate::server::log_path::sanitize_path_for_logging_lexical_escaped;
use crate::server::{CanonicalPath, CanonicalPathError};
use crate::workspace_exclusion::{exclusion_reason_for_name, exclusion_reason_for_relative_path};

/// ファイル一覧の最大件数
pub(in crate::server) const MAX_FILE_LIST: usize = 1000;

/// ディレクトリ走査の最大深度（スタックオーバーフロー防止）
pub(super) const MAX_DIR_DEPTH: usize = 32;
pub(super) const MAX_CATALOG_ENTRIES: usize = 50_000;
pub(super) const MAX_CATALOG_DIRS: usize = 10_000;

#[cfg(test)]
type CatalogProgressHook = std::sync::Arc<dyn Fn(&Path) + Send + Sync + 'static>;

#[cfg(test)]
type CatalogBeforeRecurseHook = std::sync::Arc<dyn Fn(&Path) + Send + Sync + 'static>;

#[cfg(test)]
type CatalogAfterCanonicalizeHook = std::sync::Arc<dyn Fn(&Path) + Send + Sync + 'static>;

#[cfg(test)]
static CATALOG_PROGRESS_HOOK: std::sync::OnceLock<std::sync::Mutex<Option<CatalogProgressHook>>> =
    std::sync::OnceLock::new();

#[cfg(test)]
static CATALOG_BEFORE_RECURSE_HOOK: std::sync::OnceLock<
    std::sync::Mutex<Option<CatalogBeforeRecurseHook>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
static CATALOG_AFTER_CANONICALIZE_HOOK: std::sync::OnceLock<
    std::sync::Mutex<Option<CatalogAfterCanonicalizeHook>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
#[allow(dead_code)]
pub(in crate::server) struct CatalogProgressHookGuard;

#[cfg(test)]
pub(in crate::server) struct CatalogBeforeRecurseHookGuard;

#[cfg(test)]
pub(in crate::server) struct CatalogAfterCanonicalizeHookGuard;

#[cfg(test)]
impl Drop for CatalogProgressHookGuard {
    fn drop(&mut self) {
        let hook = CATALOG_PROGRESS_HOOK.get_or_init(|| std::sync::Mutex::new(None));
        *hook.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

#[cfg(test)]
impl Drop for CatalogBeforeRecurseHookGuard {
    fn drop(&mut self) {
        let hook = CATALOG_BEFORE_RECURSE_HOOK.get_or_init(|| std::sync::Mutex::new(None));
        *hook.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

#[cfg(test)]
impl Drop for CatalogAfterCanonicalizeHookGuard {
    fn drop(&mut self) {
        let hook = CATALOG_AFTER_CANONICALIZE_HOOK.get_or_init(|| std::sync::Mutex::new(None));
        *hook.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(in crate::server) fn set_catalog_progress_hook_for_test(
    hook: CatalogProgressHook,
) -> CatalogProgressHookGuard {
    let slot = CATALOG_PROGRESS_HOOK.get_or_init(|| std::sync::Mutex::new(None));
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    CatalogProgressHookGuard
}

#[cfg(test)]
pub(in crate::server) fn set_catalog_before_recurse_hook_for_test(
    hook: CatalogBeforeRecurseHook,
) -> CatalogBeforeRecurseHookGuard {
    let slot = CATALOG_BEFORE_RECURSE_HOOK.get_or_init(|| std::sync::Mutex::new(None));
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    CatalogBeforeRecurseHookGuard
}

#[cfg(test)]
pub(in crate::server) fn set_catalog_after_canonicalize_hook_for_test(
    hook: CatalogAfterCanonicalizeHook,
) -> CatalogAfterCanonicalizeHookGuard {
    let slot = CATALOG_AFTER_CANONICALIZE_HOOK.get_or_init(|| std::sync::Mutex::new(None));
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    CatalogAfterCanonicalizeHookGuard
}

#[cfg(test)]
fn notify_catalog_progress_for_test(display_path: &Path) {
    let hook = CATALOG_PROGRESS_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(hook) = hook {
        hook(display_path);
    }
}

#[cfg(not(test))]
fn notify_catalog_progress_for_test(_display_path: &Path) {}

#[cfg(test)]
fn notify_catalog_before_recurse_for_test(display_path: &Path) {
    let hook = CATALOG_BEFORE_RECURSE_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(hook) = hook {
        hook(display_path);
    }
}

#[cfg(not(test))]
fn notify_catalog_before_recurse_for_test(_display_path: &Path) {}

#[cfg(test)]
fn notify_catalog_after_canonicalize_for_test(display_path: &Path) {
    let hook = CATALOG_AFTER_CANONICALIZE_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(hook) = hook {
        hook(display_path);
    }
}

#[cfg(not(test))]
fn notify_catalog_after_canonicalize_for_test(_display_path: &Path) {}

/// ディレクトリ内の.mdファイルを再帰的に列挙する
pub fn list_markdown_files(base_dir: &Path) -> std::io::Result<Vec<String>> {
    let canonical = CanonicalPath::try_from_path(base_dir).map_err(|error| match error {
        CanonicalPathError::Canonicalize(error) | CanonicalPathError::Metadata(error) => error,
        CanonicalPathError::UnsupportedIdentity => std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "このファイルシステムではパスの実体IDを取得できません",
        ),
    })?;
    list_markdown_files_from_canonical_base(&canonical, MAX_FILE_LIST)
}

pub(in crate::server) fn list_markdown_files_from_canonical_base(
    base_dir: &CanonicalPath,
    max_files: usize,
) -> std::io::Result<Vec<String>> {
    Ok(
        list_markdown_files_from_canonical_base_until_cancelled(base_dir, max_files, &|| false)?
            .files,
    )
}

pub(in crate::server) fn list_markdown_files_from_canonical_base_until_cancelled(
    base_dir: &CanonicalPath,
    max_files: usize,
    is_cancelled: &dyn Fn() -> bool,
) -> std::io::Result<CatalogList> {
    let verified_base_dir = open_verified_base_dir(base_dir, "ファイル一覧base directory")?;
    list_markdown_files_from_verified_base_until_cancelled(
        &verified_base_dir,
        base_dir.as_path(),
        max_files,
        is_cancelled,
        &|display_path| notify_catalog_progress_for_test(display_path),
    )
}

pub(in crate::server) fn open_verified_base_dir(
    base_dir: &CanonicalPath,
    label: &'static str,
) -> std::io::Result<Dir> {
    let verified_base_dir =
        Dir::open_ambient_dir(base_dir.as_path(), cap_std::ambient_authority())?;
    let metadata = verified_base_dir.dir_metadata()?;
    if base_dir.matches_cap_metadata_identity(&metadata) {
        return Ok(verified_base_dir);
    }

    tracing::warn!("[markdown-view] {}の実体差し替えを検出しました", label);
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!("{}が起動時と異なります", label),
    ))
}

pub(in crate::server) fn open_relative_dir_nofollow(
    base_dir: &Dir,
    relative: &Path,
) -> std::io::Result<Dir> {
    let mut current = base_dir.try_clone()?.into_std_file();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "ディレクトリpathに通常component以外が含まれています",
            ));
        };
        current = open_dir_nofollow(&current, Path::new(name))?;
    }
    Ok(Dir::from_std_file(current))
}

pub(in crate::server) fn open_relative_file_nofollow(
    base_dir: &Dir,
    relative: &Path,
) -> std::io::Result<CapFile> {
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let file_name = relative.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "ファイルpathにファイル名がありません",
        )
    })?;
    let parent_dir = open_relative_dir_nofollow(base_dir, parent)?;
    let mut options = CapOpenOptions::new();
    options.read(true);
    options._cap_fs_ext_follow(FollowSymlinks::No);
    parent_dir.open_with(Path::new(file_name), &options)
}

pub(in crate::server) fn list_markdown_files_from_verified_base_until_cancelled(
    base_dir: &Dir,
    canonical_base_dir: &Path,
    max_files: usize,
    is_cancelled: &dyn Fn() -> bool,
    on_progress: &dyn Fn(&Path),
) -> std::io::Result<CatalogList> {
    list_markdown_files_from_verified_base_with_limits_until_cancelled(
        base_dir,
        canonical_base_dir,
        max_files,
        MAX_CATALOG_ENTRIES,
        MAX_CATALOG_DIRS,
        is_cancelled,
        on_progress,
    )
}

#[cfg(test)]
pub(in crate::server) fn list_markdown_files_from_verified_base_with_limits_for_test(
    base_dir: &Dir,
    canonical_base_dir: &Path,
    max_files: usize,
    max_entries: usize,
    max_dirs: usize,
) -> std::io::Result<CatalogList> {
    list_markdown_files_from_verified_base_with_limits_until_cancelled(
        base_dir,
        canonical_base_dir,
        max_files,
        max_entries,
        max_dirs,
        &|| false,
        &|_| {},
    )
}

fn list_markdown_files_from_verified_base_with_limits_until_cancelled(
    base_dir: &Dir,
    canonical_base_dir: &Path,
    max_files: usize,
    max_entries: usize,
    max_dirs: usize,
    is_cancelled: &dyn Fn() -> bool,
    on_progress: &dyn Fn(&Path),
) -> std::io::Result<CatalogList> {
    let mut files = Vec::new();
    let mut visited_dirs = HashSet::new();
    visited_dirs.insert(PathBuf::new());
    let mut budget = CatalogTraversalBudget::new(max_entries, max_dirs);
    let traversal = CapCatalogTraversal {
        base_dir,
        canonical_base_dir,
        max_files,
        is_cancelled,
        on_progress,
    };

    list_markdown_files_from_verified_base_recursive(
        &traversal,
        base_dir,
        Path::new(""),
        Path::new(""),
        &mut CatalogTraversalState {
            files: &mut files,
            visited_dirs: &mut visited_dirs,
            budget: &mut budget,
        },
        0,
    )?;
    files.sort();
    files.truncate(max_files);
    Ok(CatalogList {
        files,
        truncated: budget.truncated,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::server) struct CatalogList {
    pub(in crate::server) files: Vec<String>,
    pub(in crate::server) truncated: bool,
}

struct CapCatalogTraversal<'a> {
    base_dir: &'a Dir,
    canonical_base_dir: &'a Path,
    max_files: usize,
    is_cancelled: &'a dyn Fn() -> bool,
    on_progress: &'a dyn Fn(&Path),
}

struct CatalogTraversalState<'a> {
    files: &'a mut Vec<String>,
    visited_dirs: &'a mut HashSet<PathBuf>,
    budget: &'a mut CatalogTraversalBudget,
}

#[derive(Debug, Default)]
struct CatalogTraversalBudget {
    entries: usize,
    dirs: usize,
    max_entries: usize,
    max_dirs: usize,
    truncated: bool,
}

impl CatalogTraversalBudget {
    fn new(max_entries: usize, max_dirs: usize) -> Self {
        Self {
            entries: 0,
            dirs: 0,
            max_entries,
            max_dirs,
            truncated: false,
        }
    }

    fn try_count_entry(&mut self, display_path: &Path) -> bool {
        if self.entries >= self.max_entries {
            self.truncated = true;
            tracing::warn!(
                "[markdown-view] ファイル一覧entry走査上限に到達（部分結果を返します）: limit={} path={}",
                self.max_entries,
                log_catalog_relative(display_path)
            );
            return false;
        }
        self.entries += 1;
        true
    }

    fn try_count_dir(&mut self, display_path: &Path) -> bool {
        if self.dirs >= self.max_dirs {
            self.truncated = true;
            tracing::warn!(
                "[markdown-view] ファイル一覧directory走査上限に到達（部分結果を返します）: limit={} path={}",
                self.max_dirs,
                log_catalog_relative(display_path)
            );
            return false;
        }
        self.dirs += 1;
        true
    }
}

fn relative_path_to_slash_string(relative: &Path) -> String {
    let mut output = String::new();
    for component in relative.components() {
        if !output.is_empty() {
            output.push('/');
        }
        output.push_str(&component.as_os_str().to_string_lossy());
    }
    output
}

fn list_markdown_files_from_verified_base_recursive(
    traversal: &CapCatalogTraversal<'_>,
    current_dir: &Dir,
    current_relative: &Path,
    display_relative: &Path,
    state: &mut CatalogTraversalState<'_>,
    depth: usize,
) -> std::io::Result<()> {
    if (traversal.is_cancelled)() {
        return Ok(());
    }

    if depth >= MAX_DIR_DEPTH {
        tracing::warn!(
            "[markdown-view] ディレクトリ深度上限に到達（スキップ）: {}",
            log_catalog_relative(display_relative)
        );
        return Ok(());
    }

    let entries = current_dir.entries()?;
    for entry in entries {
        if (traversal.is_cancelled)() {
            return Ok(());
        }

        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] ディレクトリエントリ読み取りエラー（スキップ）: {} ({})",
                    log_catalog_relative(display_relative),
                    error
                );
                continue;
            }
        };

        let name = entry.file_name();
        let display_path = display_relative.join(&name);
        if !state.budget.try_count_entry(&display_path) {
            return Ok(());
        }
        if exclusion_reason_for_name(&name).is_some() {
            continue;
        }

        let access_relative = current_relative.join(&name);
        (traversal.on_progress)(&display_path);
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] ファイルタイプ取得エラー（スキップ）: {} ({})",
                    log_catalog_relative(&display_path),
                    error
                );
                continue;
            }
        };

        if file_type.is_dir() || file_type.is_symlink() {
            if state.files.len() >= traversal.max_files {
                return Ok(());
            }
            if !state.budget.try_count_dir(&display_path) {
                return Ok(());
            }
            let Some((traversal_relative, traversal_dir)) = resolve_cap_recursable_directory(
                traversal.base_dir,
                traversal.canonical_base_dir,
                &access_relative,
                &display_path,
                file_type.is_symlink(),
            )?
            else {
                continue;
            };
            if !state.visited_dirs.insert(traversal_relative.clone()) {
                if file_type.is_symlink() {
                    tracing::warn!(
                        "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                        log_catalog_relative(&display_path)
                    );
                }
                continue;
            }
            notify_catalog_before_recurse_for_test(&display_path);
            list_markdown_files_from_verified_base_recursive(
                traversal,
                &traversal_dir,
                &traversal_relative,
                &display_path,
                state,
                depth + 1,
            )?;
        } else if file_type.is_file()
            && display_path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            state
                .files
                .push(relative_path_to_slash_string(&display_path));
            if state.files.len() >= traversal.max_files {
                return Ok(());
            }
        }
    }

    Ok(())
}

fn resolve_cap_recursable_directory(
    base_dir: &Dir,
    canonical_base_dir: &Path,
    access_relative: &Path,
    display_path: &Path,
    is_symlink: bool,
) -> std::io::Result<Option<(PathBuf, Dir)>> {
    let canonical_relative = match canonical_cap_relative_dir(base_dir, access_relative) {
        Ok(relative) => relative,
        Err(error) if is_symlink => {
            match canonical_symlink_relative(canonical_base_dir, access_relative, display_path)? {
                Some(relative) => relative,
                None => {
                    tracing::debug!(
                    "[markdown-view] シンボリックリンクディレクトリ解決失敗（スキップ）: {} ({})",
                    log_catalog_relative(display_path),
                    error
                );
                    return Ok(None);
                }
            }
        }
        Err(error) => return Err(error),
    };
    notify_catalog_after_canonicalize_for_test(display_path);

    if is_symlink {
        let metadata = match base_dir.metadata(&canonical_relative) {
            Ok(metadata) => metadata,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] シンボリックリンクのメタデータ取得に失敗（スキップ）: {} ({})",
                    log_catalog_relative(display_path),
                    error
                );
                return Ok(None);
            }
        };
        if !metadata.is_dir() {
            tracing::debug!(
                "[markdown-view] シンボリックリンクが通常ファイルを指すためスキップ: {} -> {}",
                log_catalog_relative(display_path),
                log_catalog_relative(&canonical_relative)
            );
            return Ok(None);
        }
    } else if canonical_relative != access_relative {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "通常ディレクトリの正規化先が走査前検証時と一致しません",
        ));
    }

    let traversal_dir = match open_relative_dir_nofollow(base_dir, &canonical_relative) {
        Ok(dir) => dir,
        Err(error) => {
            tracing::warn!(
                "[markdown-view] 走査対象ディレクトリopen検証エラー（スキップ）: {} ({})",
                log_catalog_relative(display_path),
                error
            );
            return Ok(None);
        }
    };
    Ok(Some((canonical_relative, traversal_dir)))
}

fn canonical_symlink_relative(
    canonical_base_dir: &Path,
    access_relative: &Path,
    display_path: &Path,
) -> std::io::Result<Option<PathBuf>> {
    let absolute_candidate = canonical_base_dir.join(access_relative);
    let resolved = match absolute_candidate.canonicalize() {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::warn!(
                "[markdown-view] シンボリックリンクの正規化に失敗（スキップ）: {} ({})",
                log_catalog_relative(display_path),
                error
            );
            return Ok(None);
        }
    };
    if !resolved.starts_with(canonical_base_dir) {
        tracing::warn!(
            "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
            log_catalog_relative(display_path),
            sanitize_path_for_logging_escaped(&resolved, canonical_base_dir)
        );
        return Ok(None);
    }
    let relative = resolved
        .strip_prefix(canonical_base_dir)
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "シンボリックリンクの正規化先がベースディレクトリ外です",
            )
        })?
        .to_path_buf();
    if exclusion_reason_for_relative_path(&relative).is_some() {
        tracing::debug!(
            "[markdown-view] シンボリックリンクが除外ディレクトリを指すためスキップ: {} -> {}",
            log_catalog_relative(display_path),
            log_catalog_relative(&relative)
        );
        return Ok(None);
    }
    Ok(Some(relative))
}

fn canonical_cap_relative_dir(base_dir: &Dir, relative: &Path) -> std::io::Result<PathBuf> {
    let canonical_relative = base_dir.canonicalize(relative)?;
    if canonical_relative.is_absolute()
        || canonical_relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "検索対象ディレクトリがベースディレクトリ外を指しています",
        ));
    }
    if exclusion_reason_for_relative_path(&canonical_relative).is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "検索対象ディレクトリが除外パスを指しています",
        ));
    }
    Ok(canonical_relative)
}

fn log_catalog_relative(path: &Path) -> String {
    relative_path_to_slash_string(path)
        .escape_debug()
        .to_string()
}

#[cfg(test)]
pub(super) fn ensure_current_dir_still_canonical(
    current_dir: &Path,
    canonical_base_dir: &Path,
    log_base_dir: &Path,
) -> std::io::Result<()> {
    let current_canonical = current_dir.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] ディレクトリ再検証エラー: {} ({})",
            sanitize_path_for_logging_lexical_escaped(current_dir, log_base_dir),
            error
        );
        error
    })?;

    if current_canonical != current_dir || !current_canonical.starts_with(canonical_base_dir) {
        tracing::warn!(
            "[markdown-view] ディレクトリ再検証で正規化先の差し替えを検出: {} -> {}",
            sanitize_path_for_logging_lexical_escaped(current_dir, log_base_dir),
            sanitize_path_for_logging_escaped(&current_canonical, log_base_dir)
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "ディレクトリの正規化先が走査前検証時と一致しません",
        ));
    }

    Ok(())
}

#[cfg(test)]
pub(super) fn resolve_recursable_directory(
    path: &Path,
    is_symlink: bool,
    canonical_base_dir: &Path,
    visited_dirs: &mut HashSet<PathBuf>,
    log_base_dir: &Path,
) -> std::io::Result<Option<PathBuf>> {
    let label = if is_symlink {
        "シンボリックリンク"
    } else {
        "通常ディレクトリ"
    };
    let Some(resolved) = canonicalize_dir_for_cycle(path, label, log_base_dir) else {
        return Ok(None);
    };

    if !resolved.starts_with(canonical_base_dir) {
        if is_symlink {
            tracing::warn!(
                "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
                sanitize_path_for_logging_lexical_escaped(path, log_base_dir),
                sanitize_path_for_logging_escaped(&resolved, log_base_dir)
            );
            return Ok(None);
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "通常ディレクトリの正規化先がベースディレクトリ外です",
        ));
    }

    if is_symlink {
        if let Ok(relative) = resolved.strip_prefix(canonical_base_dir) {
            if exclusion_reason_for_relative_path(relative).is_some() {
                tracing::debug!(
                    "[markdown-view] シンボリックリンクが除外ディレクトリを指すためスキップ: {} -> {}",
                    sanitize_path_for_logging_lexical_escaped(path, log_base_dir),
                    sanitize_path_for_logging_escaped(&resolved, log_base_dir)
                );
                return Ok(None);
            }
        }
    }

    match std::fs::metadata(&resolved) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) if is_symlink => {
            tracing::debug!(
                "[markdown-view] シンボリックリンクが通常ファイルを指すためスキップ: {} -> {}",
                sanitize_path_for_logging_lexical_escaped(path, log_base_dir),
                sanitize_path_for_logging_escaped(&resolved, log_base_dir)
            );
            return Ok(None);
        }
        Ok(_) => {
            tracing::warn!(
                "[markdown-view] 通常ディレクトリの正規化先がディレクトリではないためスキップ: {} -> {}",
                sanitize_path_for_logging_lexical_escaped(path, log_base_dir),
                sanitize_path_for_logging_escaped(&resolved, log_base_dir)
            );
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "通常ディレクトリの正規化先がディレクトリではありません",
            ));
        }
        Err(error) => {
            if is_symlink {
                tracing::warn!(
                    "[markdown-view] {}のメタデータ取得に失敗（スキップ）: {} ({})",
                    label,
                    sanitize_path_for_logging_escaped(&resolved, log_base_dir),
                    error
                );
                return Ok(None);
            }
            tracing::warn!(
                "[markdown-view] {}のメタデータ取得に失敗: {} ({})",
                label,
                sanitize_path_for_logging_escaped(&resolved, log_base_dir),
                error
            );
            return Err(error);
        }
    }

    if !visited_dirs.insert(resolved.clone()) {
        if is_symlink {
            tracing::warn!(
                "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                sanitize_path_for_logging_lexical_escaped(path, log_base_dir)
            );
        }
        return Ok(None);
    }

    Ok(Some(resolved))
}

#[cfg(test)]
pub(super) fn canonicalize_dir_for_cycle(
    path: &Path,
    label: &str,
    base_dir: &Path,
) -> Option<PathBuf> {
    match path.canonicalize() {
        Ok(canonical) => Some(canonical),
        Err(error) => {
            tracing::warn!(
                "[markdown-view] {}の正規化に失敗（スキップ）: {} ({})",
                label,
                sanitize_path_for_logging_lexical_escaped(path, base_dir),
                error
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::relative_path_to_slash_string;
    use std::path::Path;
    #[cfg(unix)]
    use std::path::PathBuf;

    #[test]
    fn test_relative_path_to_slash_stringはネストしたpathをslash区切りにする() {
        let relative = Path::new("docs").join("guide").join("setup.md");

        assert_eq!(
            relative_path_to_slash_string(&relative),
            "docs/guide/setup.md"
        );
    }

    #[test]
    fn test_relative_path_to_slash_stringは単一componentをそのまま返す() {
        assert_eq!(
            relative_path_to_slash_string(Path::new("README.md")),
            "README.md"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_relative_path_to_slash_stringは非utf8_componentをlossy変換する() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let relative = PathBuf::from("docs").join(OsString::from_vec(b"bad-\xff.md".to_vec()));

        assert_eq!(
            relative_path_to_slash_string(&relative),
            "docs/bad-\u{fffd}.md"
        );
    }
}
