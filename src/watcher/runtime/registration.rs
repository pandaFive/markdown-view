use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::super::strategy::WatchPlan;

#[derive(Debug)]
pub(super) struct WatchRegistrationFailure {
    pub(super) registered_paths: Vec<PathBuf>,
    pub(super) source: notify::Error,
}

#[derive(Debug, Default)]
pub(super) struct WatchDirectoryRegistry {
    entries: HashSet<PathBuf>,
}

impl WatchDirectoryRegistry {
    pub(super) fn insert(&mut self, path: PathBuf) {
        self.entries.insert(normalize_watch_registry_path(&path));
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        self.entries.contains(&normalize_watch_registry_path(path))
    }

    pub(super) fn remove_subtree(&mut self, root: &Path) -> Vec<PathBuf> {
        let normalized_root = normalize_watch_registry_path(root);
        let mut removed = self
            .entries
            .iter()
            .filter(|path| normalize_watch_registry_path(path).starts_with(&normalized_root))
            .cloned()
            .collect::<Vec<_>>();
        removed.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        self.entries
            .retain(|path| !normalize_watch_registry_path(path).starts_with(&normalized_root));
        removed
    }

    pub(super) fn path_set(&self) -> HashSet<PathBuf> {
        self.entries.clone()
    }
}

pub(super) fn normalize_watch_registry_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub(super) fn register_watch_plan_with<F>(
    plan: &WatchPlan,
    registered_paths: &mut WatchDirectoryRegistry,
    mut watch: F,
) -> std::result::Result<Vec<PathBuf>, WatchRegistrationFailure>
where
    F: FnMut(&Path, notify::RecursiveMode) -> notify::Result<()>,
{
    let mut newly_registered = Vec::new();
    for entry in plan.entries() {
        if let Err(source) = watch(entry.path(), entry.recursive_mode()) {
            return Err(WatchRegistrationFailure {
                registered_paths: newly_registered,
                source,
            });
        }
        let path = entry.path().to_path_buf();
        registered_paths.insert(path.clone());
        newly_registered.push(path);
    }
    tracing::debug!(
        registered_candidates = plan.diagnostics().registered_candidates(),
        excluded_subtrees = plan.diagnostics().excluded_subtrees(),
        excluded_by_reason = ?plan.diagnostics().excluded_by_reason(),
        "[markdown-view] watcher監視計画を登録しました"
    );
    Ok(newly_registered)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    #[test]
    fn test_register_watch_plan_全entryをnonrecursiveで登録する() {
        let dir = tempfile::tempdir().unwrap();
        let plan = crate::watcher::strategy::WatchPlan::from_entries_for_test(vec![
            crate::watcher::strategy::WatchPlanEntry::new(
                dir.path().join("a"),
                notify::RecursiveMode::NonRecursive,
            ),
            crate::watcher::strategy::WatchPlanEntry::new(
                dir.path().join("b"),
                notify::RecursiveMode::NonRecursive,
            ),
        ]);
        let mut registrar = FakeWatchRegistrar::default();
        let mut registered = WatchDirectoryRegistry::default();

        register_watch_plan_with(&plan, &mut registered, |path, mode| {
            registrar.watch(path, mode)
        })
        .expect("watch plan registration should succeed");

        assert_eq!(registrar.watched.len(), 2);
        assert!(registered.contains(&dir.path().join("a")));
        assert!(registered.contains(&dir.path().join("b")));
    }

    #[test]
    fn test_register_watch_plan_一部失敗ならerrorを返す() {
        let dir = tempfile::tempdir().unwrap();
        let fail_path = dir.path().join("b");
        let plan = crate::watcher::strategy::WatchPlan::from_entries_for_test(vec![
            crate::watcher::strategy::WatchPlanEntry::new(
                dir.path().join("a"),
                notify::RecursiveMode::NonRecursive,
            ),
            crate::watcher::strategy::WatchPlanEntry::new(
                fail_path.clone(),
                notify::RecursiveMode::NonRecursive,
            ),
        ]);
        let mut registrar = FakeWatchRegistrar {
            fail_on: Some(fail_path),
            watched: Vec::new(),
        };
        let mut registered = WatchDirectoryRegistry::default();

        let error = register_watch_plan_with(&plan, &mut registered, |path, mode| {
            registrar.watch(path, mode)
        })
        .expect_err("partial registration should fail");

        assert!(error
            .source
            .to_string()
            .contains("watch registration failed"));
        assert_eq!(error.registered_paths, vec![dir.path().join("a")]);
        assert!(registered.contains(&dir.path().join("a")));
        assert!(!registered.contains(&dir.path().join("b")));
        assert_eq!(registrar.watched.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_watch_directory_registry_登録時にfdを保持し続けない() {
        let dir = tempfile::tempdir().unwrap();
        let Some(before) = count_open_fds_under(dir.path()) else {
            return;
        };
        let mut registered = WatchDirectoryRegistry::default();
        for index in 0..64 {
            let child = dir.path().join(format!("dir-{index}"));
            std::fs::create_dir(&child).unwrap();
            registered.insert(child.canonicalize().unwrap());
        }
        let Some(after) = count_open_fds_under(dir.path()) else {
            return;
        };

        assert_eq!(
            after, before,
            "registry should not retain fds for registered directories: before={before}, after={after}"
        );
        assert_eq!(registered.path_set().len(), 64);
    }
}
