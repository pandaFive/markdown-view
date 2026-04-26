use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use notify::RecursiveMode;
use notify_debouncer_mini::{DebouncedEvent, DebouncedEventKind};

use crate::server::log_path::sanitize_path_for_logging;
use crate::server::{AppMode, CanonicalPath};

#[derive(Debug, Clone)]
pub(super) enum WatchStrategy {
    SingleFile { target_path: CanonicalPath },
    Directory { base_dir: CanonicalPath },
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

    pub(super) fn watch_dir(&self) -> Result<PathBuf> {
        match self {
            Self::SingleFile { target_path } => target_path
                .as_path()
                .parent()
                .map(Path::to_path_buf)
                .context("親ディレクトリが取得できません"),
            Self::Directory { base_dir } => Ok(base_dir.as_path().to_path_buf()),
        }
    }

    pub(super) fn recursive_mode(&self) -> RecursiveMode {
        match self {
            Self::SingleFile { .. } => RecursiveMode::NonRecursive,
            Self::Directory { .. } => RecursiveMode::Recursive,
        }
    }

    pub(super) fn thread_name(&self) -> &'static str {
        match self {
            Self::SingleFile { .. } => "markdown-view-watcher-file",
            Self::Directory { .. } => "markdown-view-watcher-dir",
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

    pub(super) fn collect_changed_paths(&self, events: &[DebouncedEvent]) -> Vec<PathBuf> {
        match self {
            Self::SingleFile { target_path } => {
                collect_single_file_changes(target_path.as_path(), events)
            }
            Self::Directory { base_dir } => collect_directory_changes(base_dir.as_path(), events),
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

fn collect_directory_changes(base_dir: &Path, events: &[DebouncedEvent]) -> Vec<PathBuf> {
    let mut notified = HashSet::new();
    let mut changed_paths = Vec::new();
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
        if !is_within_base_dir(&event.path, base_dir) {
            tracing::warn!(
                "[markdown-view] ベースディレクトリ外のパスを検出（スキップ）: {}",
                sanitize_path_for_logging(&event.path, base_dir)
            );
            continue;
        }
        if is_hidden_relative(&event.path, base_dir) {
            continue;
        }
        let normalized_event_path = normalize_lexical_path(&event.path);
        if notified.insert(normalized_event_path.clone()) {
            changed_paths.push(normalized_event_path);
        }
    }
    changed_paths
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
/// `try_strip_base` が `None` を返した場合は `true` を返し、
/// 安全側に倒す（隠しファイルとして扱い処理をスキップする）。
fn is_hidden_relative(path: &Path, base: &Path) -> bool {
    match try_strip_base(path, base) {
        Some(relative) => relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.')),
        None => {
            tracing::warn!(
                "[markdown-view] 隠しファイル判定: 相対パス算出不可（安全側で除外）: {}",
                sanitize_path_for_logging(path, base)
            );
            true
        }
    }
}

/// path から base を取り除いた相対 PathBuf を返す。
///
/// `strip_prefix` が直接成功すれば即座に返す。失敗時は path/base を
/// canonicalize して再試行する。canonicalize に失敗した側は元パスを
/// そのまま使い、最終 `strip_prefix` も失敗した場合は `None` を返す。
///
/// 失敗経路では `tracing::warn!` でログを残す。
fn try_strip_base(path: &Path, base: &Path) -> Option<PathBuf> {
    if let Ok(rel) = path.strip_prefix(base) {
        return Some(rel.to_path_buf());
    }
    let canonical_path = path.canonicalize().unwrap_or_else(|e| {
        tracing::warn!(
            "[markdown-view] 隠しファイル判定: パス正規化失敗（元パスで再試行）: {} ({})",
            sanitize_path_for_logging(path, base),
            e
        );
        path.to_path_buf()
    });
    let canonical_base = base.canonicalize().unwrap_or_else(|e| {
        tracing::warn!(
            "[markdown-view] 隠しファイル判定: ベース正規化失敗（元パスで再試行）: {} ({})",
            base.display(),
            e
        );
        base.to_path_buf()
    });
    canonical_path
        .strip_prefix(&canonical_base)
        .ok()
        .map(Path::to_path_buf)
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

fn is_within_base_dir(path: &Path, base: &Path) -> bool {
    match path.canonicalize() {
        Ok(canonical_path) => canonical_path.starts_with(base),
        Err(e) => {
            tracing::warn!(
                "[markdown-view] ベース配下判定: パス正規化失敗（相対化で再試行）: {} ({})",
                sanitize_path_for_logging(path, base),
                e
            );
            let normalized_path = normalize_lexical_path(path);
            let normalized_base = normalize_lexical_path(base);
            normalized_path.starts_with(&normalized_base)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use notify::RecursiveMode;
    use notify_debouncer_mini::{DebouncedEvent, DebouncedEventKind};

    use super::{
        is_content_change_event, is_hidden_relative, is_target_file, is_within_base_dir,
        try_strip_base, WatchStrategy,
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

    #[test]
    fn test_保存完了イベントのみ更新対象に含まれる() {
        assert!(is_content_change_event(&DebouncedEventKind::Any));
        assert!(!is_content_change_event(&DebouncedEventKind::AnyContinuous));
    }

    #[test]
    fn test_strategy_単一ファイルモードのラベルとモードを返す() {
        let (_dir, target) = create_markdown_fixture("target.md", "# target");
        let parent = target.parent().unwrap().to_path_buf();
        let strategy = WatchStrategy::SingleFile {
            target_path: CanonicalPath::try_from_path(&target).unwrap(),
        };

        assert_eq!(strategy.watch_dir().unwrap(), parent);
        assert_eq!(strategy.recursive_mode(), RecursiveMode::NonRecursive);
        assert_eq!(strategy.thread_name(), "markdown-view-watcher-file");
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

        assert_eq!(strategy.watch_dir().unwrap(), dir.path());
        assert_eq!(strategy.recursive_mode(), RecursiveMode::Recursive);
        assert_eq!(strategy.thread_name(), "markdown-view-watcher-dir");
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

        assert!(!is_hidden_relative(visible_file, base));
        assert!(is_hidden_relative(hidden_file, base));
        assert!(is_hidden_relative(hidden_dotfile, base));
    }

    #[test]
    fn test_隠しファイル判定_通常のベースディレクトリ() {
        let base = Path::new("/home/user/docs");
        let visible = Path::new("/home/user/docs/guide.md");
        let hidden = Path::new("/home/user/docs/.draft/wip.md");

        assert!(!is_hidden_relative(visible, base));
        assert!(is_hidden_relative(hidden, base));
    }

    #[test]
    fn test_隠しファイル判定_相対パス算出不可時は安全側で除外() {
        let base = Path::new("/nonexistent/base/dir");
        let unrelated = Path::new("/completely/different/path/file.md");

        assert!(is_hidden_relative(unrelated, base));
    }

    #[test]
    fn test_is_within_base_dir_削除済みパスでもベース配下ならtrue() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("sub/../target.md");
        assert!(is_within_base_dir(&target, dir.path()));
    }

    #[test]
    fn test_is_within_base_dir_ベース外パスはfalse() {
        let base = Path::new("/tmp/base");
        let outside = Path::new("/tmp/other/target.md");
        assert!(!is_within_base_dir(outside, base));
    }

    #[test]
    fn test_try_strip_base_strip_prefix直接成功() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let sub = canonical_dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("guide.md");
        std::fs::write(&file_path, "# guide").unwrap();

        let result = try_strip_base(&file_path, &canonical_dir);

        assert_eq!(result, Some(PathBuf::from("sub/guide.md")));
    }

    #[test]
    fn test_try_strip_base_canonicalize経由成功() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let sub = canonical_dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("guide.md");
        std::fs::write(&file_path, "# guide").unwrap();

        // base を "sub/.." の非正規化形にして直接 strip_prefix を失敗させ、
        // canonicalize fallback 経路で成功することを確認する
        let non_normalized_base = sub.join("..");

        let result = try_strip_base(&file_path, &non_normalized_base);

        assert_eq!(result, Some(PathBuf::from("sub/guide.md")));
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_try_strip_base_完全失敗でNone() {
        let base = Path::new("/nonexistent/base/dir");
        let unrelated = Path::new("/completely/different/path/file.md");

        let result = try_strip_base(unrelated, base);

        assert!(result.is_none());
    }
}
