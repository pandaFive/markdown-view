use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use notify::RecursiveMode;
use notify_debouncer_mini::{DebouncedEvent, DebouncedEventKind};

use crate::server::log_path::sanitize_path_for_logging;
use crate::server::{AppMode, CanonicalPath};
use crate::workspace_exclusion::{
    exclusion_reason_for_name, exclusion_reason_for_relative_path, WorkspaceExclusionReason,
};

#[derive(Debug, Clone)]
pub(super) enum WatchStrategy {
    SingleFile { target_path: CanonicalPath },
    Directory { base_dir: CanonicalPath },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct WatchPlan {
    entries: Vec<WatchPlanEntry>,
    diagnostics: WatchPlanDiagnostics,
}

#[allow(dead_code)]
impl WatchPlan {
    #[cfg(test)]
    pub(super) fn from_entries_for_test(entries: Vec<WatchPlanEntry>) -> Self {
        Self {
            diagnostics: WatchPlanDiagnostics {
                registered_candidates: entries.len(),
                excluded_subtrees: 0,
                excluded_by_reason: BTreeMap::new(),
            },
            entries,
        }
    }

    pub(super) fn for_new_subtree(
        subtree: &Path,
        registered_paths: &HashSet<PathBuf>,
    ) -> Result<Self> {
        if let Some(reason) = exclude_reason_for_dir(subtree) {
            let mut diagnostics = WatchPlanDiagnostics::default();
            diagnostics.record_excluded(reason);
            return Ok(Self {
                entries: Vec::new(),
                diagnostics,
            });
        }

        let mut plan = build_directory_watch_plan(subtree)?;
        let normalized_registered_paths = registered_paths
            .iter()
            .map(|path| normalize_lexical_path(path))
            .collect::<HashSet<_>>();

        plan.entries
            .retain(|entry| !normalized_registered_paths.contains(entry.path()));
        plan.diagnostics.registered_candidates = plan.entries.len();
        Ok(plan)
    }

    pub(super) fn entries(&self) -> &[WatchPlanEntry] {
        &self.entries
    }

    pub(super) fn diagnostics(&self) -> &WatchPlanDiagnostics {
        &self.diagnostics
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WatchPlanEntry {
    path: PathBuf,
    recursive_mode: RecursiveMode,
}

#[allow(dead_code)]
impl WatchPlanEntry {
    pub(super) fn new(path: PathBuf, recursive_mode: RecursiveMode) -> Self {
        Self {
            path,
            recursive_mode,
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn recursive_mode(&self) -> RecursiveMode {
        self.recursive_mode
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WatchPlanDiagnostics {
    registered_candidates: usize,
    excluded_subtrees: usize,
    excluded_by_reason: BTreeMap<ExcludeReason, usize>,
}

#[allow(dead_code)]
impl WatchPlanDiagnostics {
    fn record_registered(&mut self) {
        self.registered_candidates += 1;
    }

    fn record_excluded(&mut self, reason: ExcludeReason) {
        self.excluded_subtrees += 1;
        *self.excluded_by_reason.entry(reason).or_insert(0) += 1;
    }

    pub(super) fn registered_candidates(&self) -> usize {
        self.registered_candidates
    }

    pub(super) fn excluded_subtrees(&self) -> usize {
        self.excluded_subtrees
    }

    pub(super) fn excluded_by_reason(&self) -> &BTreeMap<ExcludeReason, usize> {
        &self.excluded_by_reason
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ExcludeReason {
    Git,
    NodeModules,
    Target,
    Hidden,
    Symlink,
    MetadataError,
}

impl WatchStrategy {
    pub(super) fn from_mode(mode: &AppMode) -> Result<Self> {
        if let Some(file_path) = mode.single_file_canonical() {
            Ok(Self::SingleFile {
                target_path: file_path.clone(),
            })
        } else if let Some(dir_path) = mode.directory_canonical() {
            Ok(Self::Directory {
                base_dir: dir_path.clone(),
            })
        } else {
            anyhow::bail!("未知のAppModeです")
        }
    }

    #[allow(dead_code)]
    pub(super) fn watch_plan(&self) -> Result<WatchPlan> {
        match self {
            Self::SingleFile { target_path } => {
                let watch_dir = target_path
                    .as_path()
                    .parent()
                    .map(Path::to_path_buf)
                    .context("親ディレクトリが取得できません")?;
                let mut diagnostics = WatchPlanDiagnostics::default();
                diagnostics.record_registered();
                Ok(WatchPlan {
                    entries: vec![WatchPlanEntry::new(watch_dir, RecursiveMode::NonRecursive)],
                    diagnostics,
                })
            }
            Self::Directory { base_dir } => build_directory_watch_plan(base_dir.as_path()),
        }
    }

    pub(super) fn thread_name(&self) -> &'static str {
        match self {
            Self::SingleFile { .. } => "markdown-view-watcher-file",
            Self::Directory { .. } => "markdown-view-watcher-dir",
        }
    }

    pub(super) fn error_delivery_thread_name(&self) -> &'static str {
        match self {
            Self::SingleFile { .. } => "markdown-view-watch-error-delivery-file",
            Self::Directory { .. } => "markdown-view-watch-error-delivery-dir",
        }
    }

    pub(super) fn unexpected_exit_message(&self) -> &'static str {
        match self {
            Self::SingleFile { .. } => "ファイル監視スレッドが予期せず終了しました",
            Self::Directory { .. } => "ディレクトリ監視スレッドが予期せず終了しました",
        }
    }

    pub(super) fn start_error_prefix(&self) -> &'static str {
        match self {
            Self::SingleFile { .. } => "ファイル監視の開始に失敗",
            Self::Directory { .. } => "ディレクトリ監視の開始に失敗",
        }
    }

    pub(super) fn watch_error_prefix(&self) -> &'static str {
        match self {
            Self::SingleFile { .. } => "ファイル監視エラー",
            Self::Directory { .. } => "ディレクトリ監視エラー",
        }
    }

    pub(super) fn panic_message(&self) -> &'static str {
        match self {
            Self::SingleFile { .. } => "単一ファイル監視スレッドがパニックで停止しました",
            Self::Directory { .. } => "ディレクトリ監視スレッドがパニックで停止しました",
        }
    }

    pub(super) fn change_label(&self) -> &'static str {
        match self {
            Self::SingleFile { .. } => "単一ファイル更新",
            Self::Directory { .. } => "ディレクトリ更新",
        }
    }

    pub(super) fn error_label(&self) -> &'static str {
        match self {
            Self::SingleFile { .. } => "単一ファイル監視エラー",
            Self::Directory { .. } => "ディレクトリ監視エラー",
        }
    }

    pub(super) fn path_for_log(&self, path: &Path) -> String {
        match self {
            Self::SingleFile { target_path } => {
                let base = target_path
                    .as_path()
                    .parent()
                    .unwrap_or_else(|| target_path.as_path());
                sanitize_path_for_logging(path, base).into_owned()
            }
            Self::Directory { base_dir } => {
                sanitize_path_for_logging(path, base_dir.as_path()).into_owned()
            }
        }
    }

    pub(super) fn collect_changed_paths(&self, events: &[DebouncedEvent]) -> Vec<PathBuf> {
        match self {
            Self::SingleFile { target_path } => {
                collect_single_file_changes(target_path.as_path(), events)
            }
            Self::Directory { base_dir } => collect_directory_changes(base_dir, events),
        }
    }

    pub(super) fn collect_new_directory_candidates(
        &self,
        events: &[DebouncedEvent],
    ) -> Vec<PathBuf> {
        match self {
            Self::SingleFile { .. } => Vec::new(),
            Self::Directory { base_dir } => collect_directory_candidates(base_dir, events),
        }
    }
}

fn collect_single_file_changes(target_path: &Path, events: &[DebouncedEvent]) -> Vec<PathBuf> {
    for event in events {
        if is_content_change_event(&event.kind) && is_target_file(&event.path, target_path) {
            return vec![event.path.clone()];
        }
    }
    Vec::new()
}

fn collect_directory_changes(base_dir: &CanonicalPath, events: &[DebouncedEvent]) -> Vec<PathBuf> {
    let mut notified = HashSet::new();
    let mut changed_paths = Vec::new();
    let base_path = base_dir.as_path();
    for event in events {
        if !is_content_change_event(&event.kind) {
            continue;
        }
        let is_md = event
            .path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if !is_md {
            continue;
        }
        let Some(base_relative_check) = path_for_base_relative_checks(&event.path, base_path)
        else {
            tracing::warn!(
                "[markdown-view] ベースディレクトリ外のパスを検出（スキップ）: {}",
                sanitize_path_for_logging(&event.path, base_path)
            );
            continue;
        };
        if base_relative_check.is_hidden {
            continue;
        }
        let normalized_event_path = base_relative_check.normalized_event_path;
        if notified.insert(normalized_event_path.clone()) {
            changed_paths.push(normalized_event_path);
        }
    }
    changed_paths
}

fn collect_directory_candidates(
    base_dir: &CanonicalPath,
    events: &[DebouncedEvent],
) -> Vec<PathBuf> {
    let mut candidates = HashSet::new();
    let base_path = base_dir.as_path();
    for event in events {
        if !is_content_change_event(&event.kind) {
            continue;
        }

        let path = normalize_lexical_path(&event.path);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        "[markdown-view] 新規ディレクトリ候補のメタデータ取得に失敗（スキップ）: {} ({})",
                        sanitize_path_for_logging(&path, base_path),
                        error
                    );
                }
                continue;
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }

        let Some(base_relative_check) = path_for_base_relative_checks(&path, base_path) else {
            continue;
        };
        if base_relative_check.is_hidden {
            continue;
        }
        candidates.insert(base_relative_check.normalized_event_path);
    }

    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort();
    candidates
}

#[allow(dead_code)]
pub(super) fn collect_markdown_files_for_recovery(root: &Path) -> Vec<PathBuf> {
    let mut markdown_files = Vec::new();
    collect_markdown_files_for_recovery_inner(root, root, &mut markdown_files, None);
    markdown_files
}

#[allow(dead_code)]
pub(super) fn collect_markdown_files_for_recovery_under_watched_dirs(
    root: &Path,
    watched_dirs: &HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let mut markdown_files = Vec::new();
    collect_markdown_files_for_recovery_inner(root, root, &mut markdown_files, Some(watched_dirs));
    markdown_files
}

#[allow(dead_code)]
fn collect_markdown_files_for_recovery_inner(
    path: &Path,
    log_base: &Path,
    markdown_files: &mut Vec<PathBuf>,
    watched_dirs: Option<&HashSet<PathBuf>>,
) {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            tracing::warn!(
                "[markdown-view] watcher回復列挙: メタデータ取得失敗（スキップ）: {} ({})",
                sanitize_path_for_logging(path, log_base),
                error
            );
            return;
        }
    };

    if metadata.file_type().is_symlink() {
        tracing::warn!(
            "[markdown-view] watcher回復列挙: シンボリックリンクをスキップ: {}",
            sanitize_path_for_logging(path, log_base)
        );
        return;
    }

    if metadata.is_dir() {
        if exclude_reason_for_dir(path).is_some() {
            return;
        }

        let canonical_dir = match path.canonicalize() {
            Ok(canonical_dir) => canonical_dir,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] watcher回復列挙: ディレクトリ正規化失敗（スキップ）: {} ({})",
                    sanitize_path_for_logging(path, log_base),
                    error
                );
                return;
            }
        };
        if watched_dirs.is_some_and(|watched_dirs| !watched_dirs.contains(&canonical_dir)) {
            return;
        }

        let children = match std::fs::read_dir(path) {
            Ok(children) => children,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] watcher回復列挙: ディレクトリ読み取り失敗（スキップ）: {} ({})",
                    sanitize_path_for_logging(path, log_base),
                    error
                );
                return;
            }
        };

        for child in children {
            match child {
                Ok(child) => collect_markdown_files_for_recovery_inner(
                    &child.path(),
                    log_base,
                    markdown_files,
                    watched_dirs,
                ),
                Err(error) => {
                    tracing::warn!(
                        "[markdown-view] watcher回復列挙: ディレクトリエントリ読み取り失敗（スキップ）: {} ({})",
                        sanitize_path_for_logging(path, log_base),
                        error
                    );
                }
            }
        }
        return;
    }

    if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
    {
        return;
    }

    let is_markdown = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
    if !is_markdown {
        return;
    }

    match path.canonicalize() {
        Ok(canonical_path) => markdown_files.push(canonical_path),
        Err(error) => {
            tracing::warn!(
                "[markdown-view] watcher回復列挙: Markdownパス正規化失敗（スキップ）: {} ({})",
                sanitize_path_for_logging(path, log_base),
                error
            );
        }
    }
}

/// レンダリング更新が必要なイベント種別か判定する
fn is_content_change_event(kind: &DebouncedEventKind) -> bool {
    matches!(kind, DebouncedEventKind::Any)
}

/// ベースディレクトリからの相対パスに隠しコンポーネントが含まれるか判定する
///
/// ベースディレクトリ自体が`.`で始まるパスに含まれる場合でも
/// 正しく動作するよう、相対パス部分のみをチェックする。
///
/// ## Fail-safe動作
/// production経路ではbase配下判定後のパスを受け取る想定だが、テストや将来の呼び出し
/// 変更で `try_strip_canonical_base_lexical` が `None` を返した場合は `true` を返し、
/// 安全側に倒す（隠しファイルとして扱い処理をスキップする）。
#[cfg(test)]
fn is_hidden_relative_to_canonical_base(path: &Path, canonical_base: &Path) -> bool {
    match try_strip_canonical_base_lexical(path, canonical_base) {
        Some(relative) => relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.')),
        None => {
            tracing::warn!(
                "[markdown-view] 隠しファイル判定: 相対パス算出不可（安全側で除外）: {}",
                sanitize_path_for_logging(path, canonical_base)
            );
            true
        }
    }
}

#[cfg(test)]
fn try_strip_canonical_base_lexical(path: &Path, canonical_base: &Path) -> Option<PathBuf> {
    let normalized_path = normalize_lexical_path(path);
    let normalized_base = normalize_lexical_path(canonical_base);
    normalized_path
        .strip_prefix(&normalized_base)
        .ok()
        .map(Path::to_path_buf)
}

#[cfg(test)]
fn is_within_canonical_base_lexical(path: &Path, canonical_base: &Path) -> bool {
    try_strip_canonical_base_lexical(path, canonical_base).is_some()
}

struct BaseRelativeCheckPath {
    normalized_event_path: PathBuf,
    is_hidden: bool,
}

/// base相対の追加検査に使うパスを返す。
///
/// 存在するパスはcanonical targetでbase配下を確認する。削除済みなどNotFoundの場合は
/// lexicalなbase配下判定にfallbackし、それ以外のI/O失敗は通知対象から除外する。
/// 存在するパスではevent名とcanonical先の両方で隠しcomponentを確認し、隠しsymlink名と
/// 隠しsymlink先のどちらも通知しない。
/// event名のcase差分を落とさないため、canonicalでbase配下を証明した後だけ
/// component数によるsuffix fallbackを使う。
fn path_for_base_relative_checks(
    path: &Path,
    canonical_base: &Path,
) -> Option<BaseRelativeCheckPath> {
    let normalized_event_path = normalize_lexical_path(path);
    let normalized_base = normalize_lexical_path(canonical_base);

    match path.canonicalize() {
        Ok(canonical_path) => {
            let relative_canonical = canonical_path.strip_prefix(&normalized_base).ok()?;
            let relative_event = normalized_event_path
                .strip_prefix(&normalized_base)
                .ok()
                .map(Path::to_path_buf)
                .or_else(|| {
                    path_suffix_by_component_count(
                        &normalized_event_path,
                        relative_canonical.components().count(),
                    )
                })?;
            Some(BaseRelativeCheckPath {
                normalized_event_path,
                is_hidden: has_hidden_component(&relative_event)
                    || has_hidden_component(relative_canonical),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let relative_event = normalized_event_path
                .strip_prefix(&normalized_base)
                .ok()?
                .to_path_buf();
            Some(BaseRelativeCheckPath {
                normalized_event_path,
                is_hidden: has_hidden_component(&relative_event),
            })
        }
        Err(error) => {
            tracing::warn!(
                "[markdown-view] ベース配下判定: パス正規化失敗（スキップ）: {} ({})",
                sanitize_path_for_logging(path, canonical_base),
                error
            );
            None
        }
    }
}

fn path_suffix_by_component_count(path: &Path, component_count: usize) -> Option<PathBuf> {
    let components = path.components().collect::<Vec<_>>();
    if component_count > components.len() {
        return None;
    }
    Some(
        components[components.len().saturating_sub(component_count)..]
            .iter()
            .map(|component| component.as_os_str())
            .collect(),
    )
}

fn has_hidden_component(relative: &Path) -> bool {
    exclusion_reason_for_relative_path(relative).is_some()
}

#[allow(dead_code)]
fn build_directory_watch_plan(base_dir: &Path) -> Result<WatchPlan> {
    let mut entries = Vec::new();
    let mut diagnostics = WatchPlanDiagnostics::default();
    collect_watch_plan_entries(base_dir, base_dir, true, &mut entries, &mut diagnostics)?;
    Ok(WatchPlan {
        entries,
        diagnostics,
    })
}

#[allow(dead_code)]
fn collect_watch_plan_entries(
    dir: &Path,
    log_base: &Path,
    is_root: bool,
    entries: &mut Vec<WatchPlanEntry>,
    diagnostics: &mut WatchPlanDiagnostics,
) -> Result<()> {
    if !is_root {
        if let Some(reason) = exclude_reason_for_dir(dir) {
            diagnostics.record_excluded(reason);
            return Ok(());
        }
    }

    let metadata = match std::fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) if is_root => {
            return Err(error).with_context(|| {
                format!(
                    "監視対象ディレクトリのメタデータ取得に失敗: {}",
                    sanitize_path_for_logging(dir, log_base)
                )
            });
        }
        Err(error) => {
            diagnostics.record_excluded(ExcludeReason::MetadataError);
            tracing::warn!(
                "[markdown-view] watcher監視計画: メタデータ取得失敗（除外）: {} ({})",
                sanitize_path_for_logging(dir, log_base),
                error
            );
            return Ok(());
        }
    };

    if metadata.file_type().is_symlink() && !is_root {
        diagnostics.record_excluded(ExcludeReason::Symlink);
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }

    let children = match std::fs::read_dir(dir) {
        Ok(children) => children,
        Err(error) if is_root => {
            return Err(error).with_context(|| {
                format!(
                    "監視対象ディレクトリの読み取りに失敗: {}",
                    sanitize_path_for_logging(dir, log_base)
                )
            });
        }
        Err(error) => {
            diagnostics.record_excluded(ExcludeReason::MetadataError);
            tracing::warn!(
                "[markdown-view] watcher監視計画: ディレクトリ読み取り失敗（除外）: {} ({})",
                sanitize_path_for_logging(dir, log_base),
                error
            );
            return Ok(());
        }
    };

    let mut child_paths = Vec::new();
    for child in children {
        match child {
            Ok(child) => child_paths.push(child.path()),
            Err(error) if is_root => {
                return Err(error).with_context(|| {
                    format!(
                        "監視対象ディレクトリのエントリ読み取りに失敗: {}",
                        sanitize_path_for_logging(dir, log_base)
                    )
                });
            }
            Err(error) => {
                diagnostics.record_excluded(ExcludeReason::MetadataError);
                tracing::warn!(
                    "[markdown-view] watcher監視計画: ディレクトリエントリ読み取り失敗（除外）: {} ({})",
                    sanitize_path_for_logging(dir, log_base),
                    error
                );
                return Ok(());
            }
        }
    }

    if let Some(entry_path) = validate_watch_plan_entry_dir(dir, log_base, is_root, diagnostics)? {
        entries.push(WatchPlanEntry::new(entry_path, RecursiveMode::NonRecursive));
        diagnostics.record_registered();
    }

    for child_path in child_paths {
        collect_watch_plan_entries(&child_path, log_base, false, entries, diagnostics)?;
    }

    Ok(())
}

#[allow(dead_code)]
fn validate_watch_plan_entry_dir(
    dir: &Path,
    log_base: &Path,
    is_root: bool,
    diagnostics: &mut WatchPlanDiagnostics,
) -> Result<Option<PathBuf>> {
    let metadata = match std::fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) if is_root => {
            return Err(error).with_context(|| {
                format!(
                    "監視対象ディレクトリの登録前メタデータ取得に失敗: {}",
                    sanitize_path_for_logging(dir, log_base)
                )
            });
        }
        Err(error) => {
            diagnostics.record_excluded(ExcludeReason::MetadataError);
            tracing::warn!(
                "[markdown-view] watcher監視計画: 登録前メタデータ取得失敗（除外）: {} ({})",
                sanitize_path_for_logging(dir, log_base),
                error
            );
            return Ok(None);
        }
    };

    if metadata.file_type().is_symlink() && !is_root {
        diagnostics.record_excluded(ExcludeReason::Symlink);
        return Ok(None);
    }
    if !metadata.is_dir() {
        return Ok(None);
    }

    let canonical = match dir.canonicalize() {
        Ok(canonical) => canonical,
        Err(error) if is_root => {
            return Err(error).with_context(|| {
                format!(
                    "監視対象ディレクトリの登録前正規化に失敗: {}",
                    sanitize_path_for_logging(dir, log_base)
                )
            });
        }
        Err(error) => {
            diagnostics.record_excluded(ExcludeReason::MetadataError);
            tracing::warn!(
                "[markdown-view] watcher監視計画: 登録前正規化失敗（除外）: {} ({})",
                sanitize_path_for_logging(dir, log_base),
                error
            );
            return Ok(None);
        }
    };

    match std::fs::metadata(&canonical) {
        Ok(metadata) if metadata.is_dir() => Ok(Some(canonical)),
        Ok(_) if is_root => Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "監視対象rootが登録前にディレクトリではなくなりました",
        )
        .into()),
        Ok(_) => Ok(None),
        Err(error) if is_root => Err(error).with_context(|| {
            format!(
                "監視対象ディレクトリの登録前メタデータ再取得に失敗: {}",
                sanitize_path_for_logging(&canonical, log_base)
            )
        }),
        Err(error) => {
            diagnostics.record_excluded(ExcludeReason::MetadataError);
            tracing::warn!(
                "[markdown-view] watcher監視計画: 登録前メタデータ再取得失敗（除外）: {} ({})",
                sanitize_path_for_logging(&canonical, log_base),
                error
            );
            Ok(None)
        }
    }
}

#[allow(dead_code)]
fn exclude_reason_for_dir(path: &Path) -> Option<ExcludeReason> {
    exclusion_reason_for_name(path.file_name()?).map(ExcludeReason::from)
}

impl From<WorkspaceExclusionReason> for ExcludeReason {
    fn from(reason: WorkspaceExclusionReason) -> Self {
        match reason {
            WorkspaceExclusionReason::Git => Self::Git,
            WorkspaceExclusionReason::Hidden => Self::Hidden,
            WorkspaceExclusionReason::NodeModules => Self::NodeModules,
            WorkspaceExclusionReason::Target => Self::Target,
        }
    }
}

/// パスが監視対象ファイルと一致するか判定する
///
/// target_pathは起動時にcanonicalize済みの絶対パス。
/// event_pathもcanonicalizeして比較し、失敗時はファイル名と親ディレクトリの両方で比較する。
fn is_target_file(event_path: &Path, target_path: &Path) -> bool {
    match event_path.canonicalize() {
        Ok(canonical) => canonical == *target_path,
        Err(e) => {
            let log_base: &Path = target_path.parent().unwrap_or_else(|| Path::new(""));
            tracing::warn!(
                "[markdown-view] パス正規化に失敗（ファイル名比較にフォールバック）: {} ({})",
                sanitize_path_for_logging(event_path, log_base),
                e
            );
            if event_path.file_name() != target_path.file_name() {
                return false;
            }
            let Some(event_parent) = event_path.parent() else {
                return false;
            };
            let Some(target_parent) = target_path.parent() else {
                return false;
            };

            let event_parent_normalized = event_parent
                .canonicalize()
                .unwrap_or_else(|_| normalize_lexical_path(event_parent));
            let target_parent_normalized = target_parent
                .canonicalize()
                .unwrap_or_else(|_| normalize_lexical_path(target_parent));

            event_parent_normalized == target_parent_normalized
        }
    }
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use notify::RecursiveMode;
    use notify_debouncer_mini::{DebouncedEvent, DebouncedEventKind};

    use super::{
        is_content_change_event, is_hidden_relative_to_canonical_base, is_target_file,
        is_within_canonical_base_lexical, normalize_lexical_path, path_for_base_relative_checks,
        try_strip_canonical_base_lexical, WatchStrategy,
    };
    use crate::server::CanonicalPath;

    fn debounced_event(path: impl Into<PathBuf>, kind: DebouncedEventKind) -> DebouncedEvent {
        DebouncedEvent::new(path.into(), kind)
    }

    fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    fn plan_paths(plan: &super::WatchPlan) -> Vec<PathBuf> {
        let mut paths = plan
            .entries()
            .iter()
            .map(|entry| entry.path().to_path_buf())
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    fn sorted_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
        paths.sort();
        paths
    }

    #[test]
    fn test_watch_plan_単一ファイルは親ディレクトリをnonrecursiveで登録する() {
        let (_dir, target) = create_markdown_fixture("target.md", "# target");
        let strategy = WatchStrategy::SingleFile {
            target_path: CanonicalPath::try_from_path(&target).unwrap(),
        };

        let plan = strategy.watch_plan().unwrap();

        assert_eq!(plan.entries().len(), 1);
        assert_eq!(plan.entries()[0].path(), target.parent().unwrap());
        assert_eq!(
            plan.entries()[0].recursive_mode(),
            RecursiveMode::NonRecursive
        );
        assert_eq!(plan.diagnostics().registered_candidates(), 1);
        assert_eq!(plan.diagnostics().excluded_subtrees(), 0);
    }

    #[test]
    fn test_watch_plan_ディレクトリは除外対象を登録しない() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "docs",
            "docs/nested",
            ".git",
            ".git/objects",
            "node_modules",
            "node_modules/pkg",
            "target",
            "target/debug",
            ".draft",
            ".draft/notes",
        ] {
            std::fs::create_dir_all(dir.path().join(path)).unwrap();
        }
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };

        let plan = strategy.watch_plan().unwrap();
        let paths = plan_paths(&plan);

        assert!(paths.contains(&dir.path().canonicalize().unwrap()));
        assert!(paths.contains(&dir.path().join("docs").canonicalize().unwrap()));
        assert!(paths.contains(&dir.path().join("docs/nested").canonicalize().unwrap()));
        assert!(!paths.iter().any(|path| path.ends_with(".git")));
        assert!(!paths.iter().any(|path| path.ends_with("objects")));
        assert!(!paths.iter().any(|path| path.ends_with("node_modules")));
        assert!(!paths.iter().any(|path| path.ends_with("pkg")));
        assert!(!paths.iter().any(|path| path.ends_with("target")));
        assert!(!paths.iter().any(|path| path.ends_with("debug")));
        assert!(!paths.iter().any(|path| path.ends_with(".draft")));
        assert!(!paths.iter().any(|path| path.ends_with("notes")));
        assert_eq!(plan.diagnostics().registered_candidates(), 3);
        assert_eq!(plan.diagnostics().excluded_subtrees(), 4);
        assert_eq!(
            plan.diagnostics()
                .excluded_by_reason()
                .get(&super::ExcludeReason::Git),
            Some(&1)
        );
        assert_eq!(
            plan.diagnostics()
                .excluded_by_reason()
                .get(&super::ExcludeReason::NodeModules),
            Some(&1)
        );
        assert_eq!(
            plan.diagnostics()
                .excluded_by_reason()
                .get(&super::ExcludeReason::Target),
            Some(&1)
        );
        assert_eq!(
            plan.diagnostics()
                .excluded_by_reason()
                .get(&super::ExcludeReason::Hidden),
            Some(&1)
        );
    }

    #[test]
    fn test_watch_plan_隠しbase自体は登録する() {
        let parent = tempfile::tempdir().unwrap();
        let hidden_base = parent.path().join(".workspace");
        std::fs::create_dir_all(hidden_base.join("docs")).unwrap();
        std::fs::create_dir_all(hidden_base.join(".draft")).unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(&hidden_base).unwrap(),
        };

        let plan = strategy.watch_plan().unwrap();
        let paths = plan_paths(&plan);

        assert!(paths.contains(&hidden_base.canonicalize().unwrap()));
        assert!(paths.contains(&hidden_base.join("docs").canonicalize().unwrap()));
        assert!(!paths.iter().any(|path| path.ends_with(".draft")));
    }

    #[test]
    #[cfg(unix)]
    fn test_watch_plan_シンボリックリンクディレクトリは辿らない() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("docs")).unwrap();
        symlink(outside.path(), dir.path().join("linked")).unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };

        let plan = strategy.watch_plan().unwrap();
        let paths = plan_paths(&plan);

        assert!(paths.contains(&dir.path().canonicalize().unwrap()));
        assert!(!paths.iter().any(|path| path.ends_with("linked")));
        assert!(!paths.iter().any(|path| path.ends_with("docs")));
        assert_eq!(
            plan.diagnostics()
                .excluded_by_reason()
                .get(&super::ExcludeReason::Symlink),
            Some(&1)
        );
    }

    #[test]
    fn test_watch_plan_new_subtreeは既存登録済みpathを除外する() {
        let dir = tempfile::tempdir().unwrap();
        let subtree = dir.path().join("docs");
        let nested = subtree.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let registered_paths = HashSet::from([subtree.canonicalize().unwrap()]);

        let plan = super::WatchPlan::for_new_subtree(&subtree, &registered_paths).unwrap();
        let paths = plan_paths(&plan);

        assert_eq!(paths, vec![nested.canonicalize().unwrap()]);
        assert_eq!(plan.diagnostics().registered_candidates(), 1);
    }

    #[test]
    fn test_watch_plan_new_subtreeは除外対象を含めない() {
        let dir = tempfile::tempdir().unwrap();
        let subtree = dir.path().join("new");
        for path in [
            "docs",
            "docs/nested",
            ".git",
            "node_modules",
            "target",
            ".draft",
        ] {
            std::fs::create_dir_all(subtree.join(path)).unwrap();
        }
        let registered_paths = HashSet::new();

        let plan = super::WatchPlan::for_new_subtree(&subtree, &registered_paths).unwrap();
        let paths = plan_paths(&plan);

        assert!(paths.contains(&subtree.canonicalize().unwrap()));
        assert!(paths.contains(&subtree.join("docs").canonicalize().unwrap()));
        assert!(paths.contains(&subtree.join("docs/nested").canonicalize().unwrap()));
        assert!(!paths.iter().any(|path| path.ends_with(".git")));
        assert!(!paths.iter().any(|path| path.ends_with("node_modules")));
        assert!(!paths.iter().any(|path| path.ends_with("target")));
        assert!(!paths.iter().any(|path| path.ends_with(".draft")));
        assert_eq!(plan.diagnostics().registered_candidates(), 3);
        assert_eq!(plan.diagnostics().excluded_subtrees(), 4);
    }

    #[test]
    fn test_watch_plan_new_subtreeはrootが除外対象なら登録しない() {
        let dir = tempfile::tempdir().unwrap();
        let subtree = dir.path().join("node_modules");
        std::fs::create_dir_all(subtree.join("pkg")).unwrap();
        let registered_paths = HashSet::new();

        let plan = super::WatchPlan::for_new_subtree(&subtree, &registered_paths).unwrap();

        assert!(plan.entries().is_empty());
        assert_eq!(plan.diagnostics().registered_candidates(), 0);
        assert_eq!(plan.diagnostics().excluded_subtrees(), 1);
        assert_eq!(
            plan.diagnostics()
                .excluded_by_reason()
                .get(&super::ExcludeReason::NodeModules),
            Some(&1)
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_watch_plan_読み取りエラーはmetadata_errorとして診断に記録する() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let unreadable = dir.path().join("unreadable");
        std::fs::create_dir_all(unreadable.join("nested")).unwrap();
        let original_permissions = std::fs::metadata(&unreadable).unwrap().permissions();
        let mut permissions = original_permissions.clone();
        permissions.set_mode(0o000);
        std::fs::set_permissions(&unreadable, permissions).unwrap();
        if std::fs::read_dir(&unreadable).is_ok() {
            std::fs::set_permissions(&unreadable, original_permissions).unwrap();
            return;
        }
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };

        let plan = strategy.watch_plan().unwrap();
        let paths = plan_paths(&plan);

        std::fs::set_permissions(&unreadable, original_permissions).unwrap();
        assert!(!paths.iter().any(|path| path.ends_with("unreadable")));
        assert_eq!(
            plan.diagnostics()
                .excluded_by_reason()
                .get(&super::ExcludeReason::MetadataError),
            Some(&1)
        );
    }

    #[test]
    fn test_validate_watch_plan_entry_dirは登録直前にcanonical_dirを返す() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("docs");
        std::fs::create_dir_all(&target).unwrap();
        let mut diagnostics = super::WatchPlanDiagnostics::default();

        let entry =
            super::validate_watch_plan_entry_dir(&target, dir.path(), false, &mut diagnostics)
                .expect("登録前検証は成功する")
                .expect("通常ディレクトリは登録対象になる");

        assert_eq!(entry, target.canonicalize().unwrap());
        assert_eq!(diagnostics.excluded_subtrees(), 0);
    }

    #[test]
    #[cfg(unix)]
    fn test_validate_watch_plan_entry_dirは登録直前のsymlinkを除外する() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = dir.path().join("linked");
        symlink(outside.path(), &link).unwrap();
        let mut diagnostics = super::WatchPlanDiagnostics::default();

        let entry =
            super::validate_watch_plan_entry_dir(&link, dir.path(), false, &mut diagnostics)
                .expect("symlink除外は計画生成エラーにしない");

        assert!(entry.is_none());
        assert_eq!(
            diagnostics
                .excluded_by_reason()
                .get(&super::ExcludeReason::Symlink),
            Some(&1)
        );
    }

    #[test]
    fn test_保存完了イベントのみ更新対象に含まれる() {
        assert!(is_content_change_event(&DebouncedEventKind::Any));
        assert!(!is_content_change_event(&DebouncedEventKind::AnyContinuous));
    }

    #[test]
    fn test_strategy_単一ファイルモードのラベルとモードを返す() {
        let (_dir, target) = create_markdown_fixture("target.md", "# target");
        let strategy = WatchStrategy::SingleFile {
            target_path: CanonicalPath::try_from_path(&target).unwrap(),
        };

        assert_eq!(strategy.thread_name(), "markdown-view-watcher-file");
        assert_eq!(
            strategy.error_delivery_thread_name(),
            "markdown-view-watch-error-delivery-file"
        );
        assert_eq!(
            strategy.unexpected_exit_message(),
            "ファイル監視スレッドが予期せず終了しました"
        );
        assert_eq!(strategy.start_error_prefix(), "ファイル監視の開始に失敗");
        assert_eq!(strategy.watch_error_prefix(), "ファイル監視エラー");
        assert_eq!(
            strategy.panic_message(),
            "単一ファイル監視スレッドがパニックで停止しました"
        );
        assert_eq!(strategy.change_label(), "単一ファイル更新");
        assert_eq!(strategy.error_label(), "単一ファイル監視エラー");
    }

    #[test]
    fn test_strategy_ディレクトリモードのラベルとモードを返す() {
        let dir = tempfile::tempdir().unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };

        assert_eq!(strategy.thread_name(), "markdown-view-watcher-dir");
        assert_eq!(
            strategy.error_delivery_thread_name(),
            "markdown-view-watch-error-delivery-dir"
        );
        assert_eq!(
            strategy.unexpected_exit_message(),
            "ディレクトリ監視スレッドが予期せず終了しました"
        );
        assert_eq!(
            strategy.start_error_prefix(),
            "ディレクトリ監視の開始に失敗"
        );
        assert_eq!(strategy.watch_error_prefix(), "ディレクトリ監視エラー");
        assert_eq!(
            strategy.panic_message(),
            "ディレクトリ監視スレッドがパニックで停止しました"
        );
        assert_eq!(strategy.change_label(), "ディレクトリ更新");
        assert_eq!(strategy.error_label(), "ディレクトリ監視エラー");
    }

    #[test]
    fn test_collect_changed_paths_単一ファイル対象のみ通知する() {
        let (_dir, target) = create_markdown_fixture("target.md", "# target");
        let sibling = target.parent().unwrap().join("other.md");
        std::fs::write(&sibling, "# other").unwrap();
        let strategy = WatchStrategy::SingleFile {
            target_path: CanonicalPath::try_from_path(&target).unwrap(),
        };

        let received = strategy.collect_changed_paths(&[
            debounced_event(sibling.clone(), DebouncedEventKind::Any),
            debounced_event(target.clone(), DebouncedEventKind::Any),
        ]);

        assert_eq!(received, vec![target]);
    }

    #[test]
    fn test_collect_changed_paths_ディレクトリモードで非mdを無視する() {
        let dir = tempfile::tempdir().unwrap();
        let text_file = dir.path().join("notes.txt");
        std::fs::write(&text_file, "memo").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };

        let received =
            strategy.collect_changed_paths(&[debounced_event(text_file, DebouncedEventKind::Any)]);

        assert!(received.is_empty(), "非mdファイルは通知されないはず");
    }

    #[test]
    fn test_collect_changed_paths_ディレクトリモードで隠しパスを無視する() {
        let dir = tempfile::tempdir().unwrap();
        let hidden_dir = dir.path().join(".draft");
        std::fs::create_dir_all(&hidden_dir).unwrap();
        let hidden_file = hidden_dir.join("note.md");
        std::fs::write(&hidden_file, "# hidden").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };

        let received = strategy
            .collect_changed_paths(&[debounced_event(hidden_file, DebouncedEventKind::Any)]);

        assert!(received.is_empty(), "隠しパスは通知されないはず");
    }

    #[test]
    fn test_collect_changed_paths_ディレクトリモードで生成物ディレクトリ配下を無視する() {
        let dir = tempfile::tempdir().unwrap();
        let node_file = dir.path().join("node_modules/pkg/readme.md");
        let target_file = dir.path().join("target/debug/build.md");
        std::fs::create_dir_all(node_file.parent().unwrap()).unwrap();
        std::fs::create_dir_all(target_file.parent().unwrap()).unwrap();
        std::fs::write(&node_file, "# generated").unwrap();
        std::fs::write(&target_file, "# generated").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };

        let received = strategy.collect_changed_paths(&[
            debounced_event(node_file, DebouncedEventKind::Any),
            debounced_event(target_file, DebouncedEventKind::Any),
        ]);

        assert!(
            received.is_empty(),
            "生成物ディレクトリ配下は通知されないはず"
        );
    }

    #[test]
    fn test_collect_changed_paths_ディレクトリモードで重複通知を排除する() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("guide.md");
        std::fs::write(&file_path, "# guide").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };

        let received = strategy.collect_changed_paths(&[
            debounced_event(file_path.clone(), DebouncedEventKind::Any),
            debounced_event(
                dir.path().join("./guide.md"),
                DebouncedEventKind::AnyContinuous,
            ),
        ]);

        assert_eq!(received, vec![file_path]);
    }

    #[test]
    fn test_collect_changed_paths_ディレクトリモードで連続更新イベントのみは通知しない() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("guide.md");
        std::fs::write(&file_path, "# guide").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };

        let received = strategy.collect_changed_paths(&[debounced_event(
            file_path,
            DebouncedEventKind::AnyContinuous,
        )]);

        assert!(
            received.is_empty(),
            "連続更新イベントのみでは通知されないはず"
        );
    }

    #[test]
    fn test_collect_new_directory_candidates_単一ファイルは空を返す() {
        let (_dir, target) = create_markdown_fixture("target.md", "# target");
        let strategy = WatchStrategy::SingleFile {
            target_path: CanonicalPath::try_from_path(&target).unwrap(),
        };
        let events = vec![debounced_event(
            target.parent().unwrap().to_path_buf(),
            DebouncedEventKind::Any,
        )];

        let candidates = strategy.collect_new_directory_candidates(&events);

        assert!(candidates.is_empty());
    }

    #[test]
    fn test_collect_new_directory_candidates_通常ディレクトリだけ候補にする() {
        let dir = tempfile::tempdir().unwrap();
        let new_dir = dir.path().join("new");
        std::fs::create_dir_all(&new_dir).unwrap();
        let markdown = new_dir.join("note.md");
        std::fs::write(&markdown, "# note").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![
            debounced_event(markdown, DebouncedEventKind::Any),
            debounced_event(new_dir.clone(), DebouncedEventKind::Any),
        ];

        let candidates = strategy.collect_new_directory_candidates(&events);

        assert_eq!(candidates, vec![new_dir]);
    }

    #[test]
    fn test_collect_new_directory_candidates_hiddenとbase外を候補にしない() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let hidden = dir.path().join(".hidden");
        std::fs::create_dir_all(&hidden).unwrap();
        let outside_dir = outside.path().join("new");
        std::fs::create_dir_all(&outside_dir).unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![
            debounced_event(hidden, DebouncedEventKind::Any),
            debounced_event(outside_dir, DebouncedEventKind::Any),
        ];

        let candidates = strategy.collect_new_directory_candidates(&events);

        assert!(candidates.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn test_collect_new_directory_candidates_シンボリックリンクディレクトリは候補にしない() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let linked = dir.path().join("linked");
        symlink(outside.path(), &linked).unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![debounced_event(linked, DebouncedEventKind::Any)];

        let candidates = strategy.collect_new_directory_candidates(&events);

        assert!(candidates.is_empty());
    }

    #[test]
    fn test_collect_markdown_files_for_recoveryは除外対象を読まない() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        for path in ["docs", ".git", "node_modules", "target", ".draft"] {
            std::fs::create_dir_all(root.join(path)).unwrap();
        }
        let visible = root.join("docs/guide.MD");
        std::fs::write(&visible, "# guide").unwrap();
        for path in [
            ".git/secret.md",
            "node_modules/pkg.md",
            "target/generated.md",
            ".draft/wip.md",
        ] {
            std::fs::write(root.join(path), "# excluded").unwrap();
        }
        std::fs::write(root.join("notes.txt"), "not markdown").unwrap();

        let paths = sorted_paths(super::collect_markdown_files_for_recovery(&root));

        assert_eq!(paths, vec![visible.canonicalize().unwrap()]);
    }

    #[test]
    fn test_collect_markdown_files_for_recoveryはrootが除外対象なら空を返す() {
        let dir = tempfile::tempdir().unwrap();
        let hidden_root = dir.path().join(".hidden");
        std::fs::create_dir_all(&hidden_root).unwrap();
        std::fs::write(hidden_root.join("wip.md"), "# hidden").unwrap();

        let paths = super::collect_markdown_files_for_recovery(&hidden_root);

        assert!(paths.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn test_collect_markdown_files_for_recoveryはシンボリックリンクディレクトリを辿らない() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("secret.md");
        std::fs::write(&outside_file, "# secret").unwrap();
        let visible = root.join("visible.md");
        std::fs::write(&visible, "# visible").unwrap();
        symlink(outside.path(), root.join("linked")).unwrap();

        let paths = sorted_paths(super::collect_markdown_files_for_recovery(&root));

        assert_eq!(paths, vec![visible.canonicalize().unwrap()]);
    }

    #[test]
    fn test_collect_directory_changes_削除済みbase配下markdownを通知する() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_base = CanonicalPath::try_from_path(dir.path()).unwrap();
        let deleted = canonical_base.as_path().join("docs").join("deleted.md");
        let events = vec![debounced_event(deleted.clone(), DebouncedEventKind::Any)];

        let changes = WatchStrategy::Directory {
            base_dir: canonical_base,
        }
        .collect_changed_paths(&events);

        assert_eq!(changes, vec![normalize_lexical_path(&deleted)]);
    }

    #[test]
    fn test_collect_directory_changes_base外markdownを除外する() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let canonical_base = CanonicalPath::try_from_path(dir.path()).unwrap();
        let outside_file = outside.path().join("outside.md");
        let events = vec![debounced_event(outside_file, DebouncedEventKind::Any)];

        let changes = WatchStrategy::Directory {
            base_dir: canonical_base,
        }
        .collect_changed_paths(&events);

        assert!(changes.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn test_collect_directory_changes_base外symlink先markdownを除外する() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let canonical_base = CanonicalPath::try_from_path(dir.path()).unwrap();
        let outside_file = outside.path().join("secret.md");
        std::fs::write(&outside_file, "# secret").unwrap();
        let link = canonical_base.as_path().join("link.md");
        symlink(&outside_file, &link).unwrap();
        let events = vec![debounced_event(link, DebouncedEventKind::Any)];

        let changes = WatchStrategy::Directory {
            base_dir: canonical_base,
        }
        .collect_changed_paths(&events);

        assert!(changes.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn test_collect_directory_changes_base外symlinkディレクトリ配下markdownを除外する() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let canonical_base = CanonicalPath::try_from_path(dir.path()).unwrap();
        let outside_file = outside.path().join("out.md");
        std::fs::write(&outside_file, "# outside").unwrap();
        let link_dir = canonical_base.as_path().join("link");
        symlink(outside.path(), &link_dir).unwrap();
        let events = vec![debounced_event(
            link_dir.join("out.md"),
            DebouncedEventKind::Any,
        )];

        let changes = WatchStrategy::Directory {
            base_dir: canonical_base,
        }
        .collect_changed_paths(&events);

        assert!(changes.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn test_collect_directory_changes_hidden_symlink先markdownを除外する() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let canonical_base = CanonicalPath::try_from_path(dir.path()).unwrap();
        let hidden_dir = canonical_base.as_path().join(".secret");
        std::fs::create_dir_all(&hidden_dir).unwrap();
        let hidden_file = hidden_dir.join("hidden.md");
        std::fs::write(&hidden_file, "# secret").unwrap();
        let link = canonical_base.as_path().join("link.md");
        symlink(&hidden_file, &link).unwrap();
        let events = vec![debounced_event(link, DebouncedEventKind::Any)];

        let changes = WatchStrategy::Directory {
            base_dir: canonical_base,
        }
        .collect_changed_paths(&events);

        assert!(changes.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn test_collect_directory_changes_hidden_symlink名markdownを除外する() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let canonical_base = CanonicalPath::try_from_path(dir.path()).unwrap();
        let visible_file = canonical_base.as_path().join("visible.md");
        std::fs::write(&visible_file, "# visible").unwrap();
        let hidden_link = canonical_base.as_path().join(".secret.md");
        symlink(&visible_file, &hidden_link).unwrap();
        let events = vec![debounced_event(hidden_link, DebouncedEventKind::Any)];

        let changes = WatchStrategy::Directory {
            base_dir: canonical_base,
        }
        .collect_changed_paths(&events);

        assert!(changes.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn test_collect_directory_changes_broken_hidden_symlink名markdownを除外する() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let canonical_base = CanonicalPath::try_from_path(dir.path()).unwrap();
        let missing_target = canonical_base.as_path().join("missing.md");
        let hidden_link = canonical_base.as_path().join(".missing.md");
        symlink(&missing_target, &hidden_link).unwrap();
        let events = vec![debounced_event(hidden_link, DebouncedEventKind::Any)];

        let changes = WatchStrategy::Directory {
            base_dir: canonical_base,
        }
        .collect_changed_paths(&events);

        assert!(changes.is_empty());
    }

    #[test]
    fn test_collect_changed_paths_ディレクトリモードでベース外を無視する() {
        let base_dir = tempfile::tempdir().unwrap();
        let outside_dir = tempfile::tempdir().unwrap();
        let outside_file = outside_dir.path().join("outside.md");
        std::fs::write(&outside_file, "# outside").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(base_dir.path()).unwrap(),
        };

        let received = strategy
            .collect_changed_paths(&[debounced_event(outside_file, DebouncedEventKind::Any)]);

        assert!(received.is_empty(), "ベース外パスは通知されないはず");
    }

    #[test]
    fn test_is_target_file_正規化成功時は完全一致のみtrue() {
        let (dir, target) = create_markdown_fixture("target.md", "# target");
        let other = dir.path().join("other.md");
        std::fs::write(&other, "# other").unwrap();

        let canonical_target = target.canonicalize().unwrap();
        assert!(is_target_file(&target, &canonical_target));
        assert!(!is_target_file(&other, &canonical_target));
    }

    #[test]
    fn test_is_target_file_正規化失敗時は同名かつ同一親ディレクトリでフォールバック一致() {
        let (_dir, target) = create_markdown_fixture("target.md", "# target");
        let canonical_target = target.canonicalize().unwrap();

        std::fs::remove_file(&target).unwrap();
        assert!(is_target_file(&target, &canonical_target));
    }

    #[test]
    fn test_is_target_file_正規化失敗時は非正規化親パスでも一致判定できる() {
        let (dir, target) = create_markdown_fixture("target.md", "# target");
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        let canonical_target = target.canonicalize().unwrap();

        std::fs::remove_file(&target).unwrap();
        let non_normalized = dir.path().join("sub/../target.md");

        assert!(is_target_file(&non_normalized, &canonical_target));
    }

    #[test]
    fn test_is_target_file_正規化失敗フォールバックでも親ディレクトリ不一致はfalse() {
        let (_dir, target) = create_markdown_fixture("target.md", "# target");
        let canonical_target = target.canonicalize().unwrap();

        let other_dir = tempfile::tempdir().unwrap();
        let other_path_same_name = other_dir.path().join("target.md");
        assert!(!is_target_file(&other_path_same_name, &canonical_target));
    }

    #[test]
    fn test_隠しファイル判定_相対パスのみチェック() {
        let base = Path::new("/home/user/.config/docs");
        let visible_file = Path::new("/home/user/.config/docs/README.md");
        let hidden_file = Path::new("/home/user/.config/docs/.secret/notes.md");
        let hidden_dotfile = Path::new("/home/user/.config/docs/.hidden.md");

        assert!(!is_hidden_relative_to_canonical_base(visible_file, base));
        assert!(is_hidden_relative_to_canonical_base(hidden_file, base));
        assert!(is_hidden_relative_to_canonical_base(hidden_dotfile, base));
    }

    #[test]
    fn test_隠しファイル判定_通常のベースディレクトリ() {
        let base = Path::new("/home/user/docs");
        let visible = Path::new("/home/user/docs/guide.md");
        let hidden = Path::new("/home/user/docs/.draft/wip.md");

        assert!(!is_hidden_relative_to_canonical_base(visible, base));
        assert!(is_hidden_relative_to_canonical_base(hidden, base));
    }

    #[test]
    fn test_隠しファイル判定_相対パス算出不可時は安全側で除外() {
        let base = Path::new("/nonexistent/base/dir");
        let unrelated = Path::new("/completely/different/path/file.md");

        assert!(is_hidden_relative_to_canonical_base(unrelated, base));
    }

    #[test]
    fn test_is_within_canonical_base_lexical_削除済みパスでもベース配下ならtrue() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("sub/../target.md");
        assert!(is_within_canonical_base_lexical(&target, dir.path()));
    }

    #[test]
    fn test_is_within_canonical_base_lexical_ベース外パスはfalse() {
        let base = Path::new("/tmp/base");
        let outside = Path::new("/tmp/other/target.md");
        assert!(!is_within_canonical_base_lexical(outside, base));
    }

    #[test]
    fn test_try_strip_canonical_base_lexical_直接成功() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let sub = canonical_dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("guide.md");
        std::fs::write(&file_path, "# guide").unwrap();

        let result = try_strip_canonical_base_lexical(&file_path, &canonical_dir);

        assert_eq!(result, Some(PathBuf::from("sub").join("guide.md")));
    }

    #[test]
    fn test_try_strip_canonical_base_lexical_非正規化baseをlexicalに処理する() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let sub = canonical_dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("guide.md");
        std::fs::write(&file_path, "# guide").unwrap();

        let non_normalized_base = sub.join("..");

        let result = try_strip_canonical_base_lexical(&file_path, &non_normalized_base);

        assert_eq!(result, Some(PathBuf::from("sub").join("guide.md")));
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_try_strip_canonical_base_lexical_完全失敗でNone() {
        let base = Path::new("/nonexistent/base/dir");
        let unrelated = Path::new("/completely/different/path/file.md");

        let result = try_strip_canonical_base_lexical(unrelated, base);

        assert!(result.is_none());
    }

    #[test]
    fn test_path_for_base_relative_checks_正規化でbase配下確定後はlexical_prefix失敗でも許可する() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_base = dir.path().canonicalize().unwrap();
        let file_path = canonical_base.join("guide.md");
        std::fs::write(&file_path, "# guide").unwrap();
        let lexical_mismatch_base = canonical_base.join("child").join("..");

        let result = path_for_base_relative_checks(&file_path, &lexical_mismatch_base)
            .expect("canonical in-base path should be accepted");

        assert_eq!(
            result.normalized_event_path,
            normalize_lexical_path(&file_path)
        );
        assert!(!result.is_hidden);
    }
}
