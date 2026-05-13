use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use super::super::strategy::{self, WatchPlan, WatchStrategy};
use super::super::{WatchError, WatchEvent};
use super::error_queue::PriorityErrorSender;
use super::health::{WatcherFailureKind, WatcherHealthState};
use super::registration::{register_watch_plan_with, WatchDirectoryRegistry};
use super::shutdown::WATCHER_DIAGNOSTIC_SEND_TIMEOUT_MS;

pub(super) const WATCHER_MESSAGE_BUFFER: usize = 32;
pub(super) const WATCHER_INTERNAL_EVENT_BUFFER: usize = 64;
pub(super) type InternalWatchResult =
    std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>;
#[derive(Clone)]
pub(super) struct WatchEventSenders {
    pub(super) file_tx: BestEffortFileSender,
    pub(super) error_tx: PriorityErrorSender,
}

#[derive(Clone)]
pub(super) struct BestEffortFileSender {
    pub(super) tx: mpsc::Sender<PathBuf>,
    diagnostics: WatcherDiagnostics,
}

impl BestEffortFileSender {
    pub(super) fn new(tx: mpsc::Sender<PathBuf>, diagnostics: WatcherDiagnostics) -> Self {
        Self { tx, diagnostics }
    }

    pub(super) fn send(&self, path: PathBuf, label: &str) {
        send_file_changed_event(&self.tx, path, label, &self.diagnostics);
    }
}

#[derive(Clone)]
pub(super) struct WatcherDiagnostics {
    merged_tx: mpsc::Sender<WatchEvent>,
    pub(super) health_state: WatcherHealthState,
    file_sender_closed_reported: Arc<AtomicBool>,
}

impl WatcherDiagnostics {
    pub(super) fn new(
        merged_tx: mpsc::Sender<WatchEvent>,
        health_state: WatcherHealthState,
    ) -> Self {
        Self {
            merged_tx,
            health_state,
            file_sender_closed_reported: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) async fn send_error_with_timeout(&self, error: WatchError, label: &str) {
        let kind = error.kind();
        match tokio::time::timeout(
            Duration::from_millis(WATCHER_DIAGNOSTIC_SEND_TIMEOUT_MS),
            self.merged_tx.send(WatchEvent::Error(error)),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(
                    error_kind = ?kind,
                    "[markdown-view] shutdown診断イベントの送信先が閉じています: {} ({})",
                    label,
                    error
                );
            }
            Err(_) => {
                tracing::warn!(
                    error_kind = ?kind,
                    timeout_ms = WATCHER_DIAGNOSTIC_SEND_TIMEOUT_MS,
                    "[markdown-view] shutdown診断イベントの送信がタイムアウトしました: {}",
                    label
                );
            }
        }
    }

    pub(super) fn try_send_error(&self, error: WatchError, label: &str) {
        let kind = error.kind();
        match self.merged_tx.try_send(WatchEvent::Error(error)) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(
                    error_kind = ?kind,
                    "[markdown-view] 診断イベント転送チャネルが満杯のため異常通知を破棄しました: {}",
                    label
                );
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!(
                    error_kind = ?kind,
                    "[markdown-view] 診断イベント転送チャネルが閉じているため異常通知を破棄しました: {}",
                    label
                );
            }
        }
    }

    pub(super) fn report_file_sender_closed(&self, label: &str) {
        if self
            .file_sender_closed_reported
            .swap(true, Ordering::AcqRel)
        {
            return;
        }
        self.health_state
            .store_failed(WatcherFailureKind::ForwarderStopped);
        self.try_send_error(
            WatchError::forwarder_stopped(format!("file change channel が閉じています: {label}")),
            "file change channel closed",
        );
    }
}
pub(super) fn send_merged_file_changed_event(tx: &mpsc::Sender<WatchEvent>, path: PathBuf) -> bool {
    if tx.is_closed() {
        tracing::warn!(
            "[markdown-view] 監視イベント転送チャネルが閉じているためFileChangedを破棄しました"
        );
        return false;
    }

    if tx.capacity() <= 1 {
        tracing::warn!(
            "[markdown-view] 監視イベント転送チャネルが混雑しているためError用の余白を残してFileChangedを破棄しました"
        );
        return !tx.is_closed();
    }

    match tx.try_send(WatchEvent::FileChanged(path)) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            debug_assert!(
                false,
                "FileChanged should be dropped before try_send when merged capacity is reserved"
            );
            tracing::warn!(
                "[markdown-view] 監視イベント転送チャネルが満杯のためFileChangedを破棄しました"
            );
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::warn!(
                "[markdown-view] 監視イベント転送チャネルが閉じているためFileChangedを破棄しました"
            );
            false
        }
    }
}

/// notifyコールバックからtokioチャネルへFileChangedを転送する（non-blocking）。
/// チャネル満杯時・クローズ時はイベントを破棄しwarnログを出力する。
pub(super) fn send_file_changed_event(
    tx: &mpsc::Sender<PathBuf>,
    path: PathBuf,
    label: &str,
    diagnostics: &WatcherDiagnostics,
) {
    match tx.try_send(path) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::warn!(
                "[markdown-view] 監視イベントが多すぎるため通知を破棄しました: {}",
                label
            );
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::warn!(
                "[markdown-view] 通知チャネルが閉じているため監視イベントを破棄しました: {}",
                label
            );
            diagnostics.report_file_sender_closed(label);
        }
    }
}

/// watcher error を専用チャネルへ転送する（non-blocking）。
///
/// error は FileChanged backlog から独立した bounded ring queue に積む。
/// capacity 超過時は最新の異常を残すため最古の error を破棄し、watcher thread を
/// 停止不能にしない。
pub(super) fn send_error_event(tx: &PriorityErrorSender, error: WatchError, label: &str) {
    let mut state = tx.queue.state.lock().expect("priority error queue mutex");
    if state.closed {
        tracing::warn!(
            error_kind = ?error.kind(),
            "[markdown-view] watcher error channel が閉じているため異常通知を破棄しました: {}",
            label
        );
        return;
    }

    if state.items.len() == tx.queue.capacity {
        let evicted = state.items.pop_front();
        if let Some(evicted) = evicted {
            tracing::warn!(
                evicted_kind = ?evicted.kind(),
                capacity = tx.queue.capacity,
                "[markdown-view] watcher error channel が満杯のため最古の異常通知を破棄しました"
            );
        }
    }
    state.items.push_back(error);
    drop(state);
    // PriorityErrorReceiver は単一consumer前提。複数consumer化する場合はnotify_waitersを検討する。
    tx.queue.notify.notify_one();
}
pub(super) fn send_internal_watch_result(
    internal_tx: &std::sync::mpsc::SyncSender<InternalWatchResult>,
    result: InternalWatchResult,
    strategy: &WatchStrategy,
    senders: &WatchEventSenders,
    health_state: &WatcherHealthState,
) {
    match internal_tx.try_send(result) {
        Ok(()) => {}
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            tracing::warn!("[markdown-view] watcher internal channel が満杯です");
            health_state.store_failed(WatcherFailureKind::Notify);
            senders.error_tx.send(
                WatchError::notify("watcher internal channel が満杯です"),
                strategy.error_label(),
            );
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            tracing::warn!("[markdown-view] watcher internal channel が閉じています");
            health_state.store_failed(WatcherFailureKind::Notify);
            senders.error_tx.send(
                WatchError::notify("watcher internal channel が閉じています"),
                strategy.error_label(),
            );
        }
    }
}

pub(super) fn handle_debounced_watch_result(
    result: std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>,
    strategy: &WatchStrategy,
    senders: &WatchEventSenders,
    health_state: &WatcherHealthState,
) {
    match result {
        Ok(events) => {
            for changed_path in strategy.collect_changed_paths(&events) {
                senders.file_tx.send(changed_path, strategy.change_label());
            }
        }
        Err(e) => {
            health_state.store_failed(WatcherFailureKind::Notify);
            let watch_error = WatchError::notify(e.to_string());
            tracing::warn!(
                "[markdown-view] {}: {}",
                strategy.watch_error_prefix(),
                watch_error.detail()
            );
            senders.error_tx.send(watch_error, strategy.error_label());
        }
    }
}

#[cfg(test)]
pub(super) fn process_debounced_events_with_watch<F>(
    events: Vec<notify_debouncer_mini::DebouncedEvent>,
    strategy: &WatchStrategy,
    senders: &WatchEventSenders,
    health_state: &WatcherHealthState,
    registered_paths: &mut WatchDirectoryRegistry,
    watch: F,
) where
    F: FnMut(&Path, notify::RecursiveMode) -> notify::Result<()>,
{
    process_debounced_events_with_watch_and_unwatch(
        events,
        strategy,
        senders,
        health_state,
        registered_paths,
        watch,
        |_path| Ok(()),
    );
}

pub(super) fn process_debounced_events_with_watch_and_unwatch<F, U>(
    events: Vec<notify_debouncer_mini::DebouncedEvent>,
    strategy: &WatchStrategy,
    senders: &WatchEventSenders,
    health_state: &WatcherHealthState,
    registered_paths: &mut WatchDirectoryRegistry,
    mut watch: F,
    mut unwatch: U,
) where
    F: FnMut(&Path, notify::RecursiveMode) -> notify::Result<()>,
    U: FnMut(&Path) -> notify::Result<()>,
{
    for changed_path in strategy.collect_changed_paths(&events) {
        senders.file_tx.send(changed_path, strategy.change_label());
    }

    for candidate in strategy.collect_new_directory_candidates(&events) {
        if registered_paths.contains(&candidate) {
            for stale_path in registered_paths.remove_subtree(&candidate) {
                if let Err(error) = unwatch(&stale_path) {
                    tracing::warn!(
                        "[markdown-view] 登録済みディレクトリの監視解除に失敗（再登録は継続）: path={}, {}",
                        strategy.path_for_log(&stale_path),
                        error
                    );
                }
            }
        }

        let registered_path_set = registered_paths.path_set();
        let plan = match WatchPlan::for_new_subtree(&candidate, &registered_path_set) {
            Ok(plan) => plan,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] 新規ディレクトリの監視計画生成に失敗: {}",
                    error
                );
                continue;
            }
        };

        if plan.entries().is_empty() {
            continue;
        }

        let (registered_now, registration_error) =
            match register_watch_plan_with(&plan, registered_paths, |path, mode| watch(path, mode))
            {
                Ok(registered_now) => (registered_now, None),
                Err(failure) => {
                    health_state.store_failed(WatcherFailureKind::Notify);
                    let watch_error = WatchError::from_watch_registration_error(
                        "新規ディレクトリの監視追加に失敗",
                        &failure.source,
                    );
                    tracing::warn!(
                        "[markdown-view] 新規ディレクトリの監視追加に失敗: candidate={}, {}",
                        strategy.path_for_log(&candidate),
                        watch_error.detail()
                    );
                    (failure.registered_paths, Some(watch_error))
                }
            };

        let mut recovered = HashSet::new();
        let registered_now = registered_now.into_iter().collect::<HashSet<_>>();
        for markdown in strategy::collect_markdown_files_for_recovery_under_watched_dirs(
            &candidate,
            &registered_now,
        ) {
            if recovered.insert(markdown.clone()) {
                senders.file_tx.send(markdown, strategy.change_label());
            }
        }

        if let Some(watch_error) = registration_error {
            senders.error_tx.send(watch_error, strategy.error_label());
        }
    }
}
pub(super) fn handle_internal_channel_disconnected(
    strategy: &WatchStrategy,
    senders: &WatchEventSenders,
    health_state: &WatcherHealthState,
    shutdown_flag: &AtomicBool,
) {
    tracing::warn!("[markdown-view] watcher internal channel が切断されました");
    if shutdown_flag.load(Ordering::Acquire) {
        return;
    }
    health_state.store_failed(WatcherFailureKind::Notify);
    senders.error_tx.send(
        WatchError::notify("watcher internal channel が切断されました"),
        strategy.error_label(),
    );
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use tokio::sync::mpsc;
    use tracing_test::traced_test;

    use crate::server::AppMode;
    use crate::watcher::strategy::WatchStrategy;
    use crate::watcher::{WatchErrorKind, WatchEvent, WatcherHealth};

    #[test]
    fn test_process_internal_events_新規ディレクトリをwatch追加して既存markdownを通知する() {
        let dir = tempfile::tempdir().unwrap();
        let new_dir = dir.path().join("new");
        std::fs::create_dir_all(&new_dir).unwrap();
        let md = new_dir.join("created.md");
        std::fs::write(&md, "# created").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![notify_debouncer_mini::DebouncedEvent::new(
            new_dir.clone(),
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let health_state = WatcherHealthState::new_alive();
        let (senders, mut file_rx, _error_rx) = split_senders_for_test(4, 4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());
        let mut watched = Vec::new();

        process_debounced_events_with_watch(
            events,
            &strategy,
            &senders,
            &health_state,
            &mut registered,
            |path, mode| {
                watched.push((path.to_path_buf(), mode));
                Ok(())
            },
        );

        assert!(watched.iter().any(|(path, mode)| {
            path == &new_dir.canonicalize().unwrap() && *mode == notify::RecursiveMode::NonRecursive
        }));
        assert_eq!(
            file_rx
                .try_recv()
                .expect("recovery markdown notificationを期待"),
            md.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_process_internal_events_追加watch失敗はhealth_failedとerror_eventを送る() {
        let dir = tempfile::tempdir().unwrap();
        let new_dir = dir.path().join("new");
        std::fs::create_dir_all(&new_dir).unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![notify_debouncer_mini::DebouncedEvent::new(
            new_dir,
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let health_state = WatcherHealthState::new_alive();
        let (senders, _file_rx, mut error_rx) = split_senders_for_test(4, 4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());

        process_debounced_events_with_watch(
            events,
            &strategy,
            &senders,
            &health_state,
            &mut registered,
            |_path, _mode| Err(notify::Error::generic("dynamic watch failed")),
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        let error = error_rx
            .try_recv()
            .expect("dynamic watch error eventを期待");
        assert_eq!(error.kind(), WatchErrorKind::Notify);
        assert!(error.detail().contains("dynamic watch failed"));
    }

    #[test]
    fn test_process_internal_events_追加watch部分失敗は成功済みrootだけrecovery通知する() {
        let dir = tempfile::tempdir().unwrap();
        let new_dir = dir.path().join("new");
        let nested = new_dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let root_md = new_dir.join("root.md");
        let nested_md = nested.join("child.md");
        std::fs::write(&root_md, "# root").unwrap();
        std::fs::write(&nested_md, "# child").unwrap();
        let nested_canonical = nested.canonicalize().unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![notify_debouncer_mini::DebouncedEvent::new(
            new_dir.clone(),
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let health_state = WatcherHealthState::new_alive();
        let (senders, mut file_rx, mut error_rx) = split_senders_for_test(4, 4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());

        process_debounced_events_with_watch(
            events,
            &strategy,
            &senders,
            &health_state,
            &mut registered,
            |path, _mode| {
                if path == nested_canonical {
                    Err(notify::Error::generic("nested watch failed"))
                } else {
                    Ok(())
                }
            },
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        assert!(registered.contains(&new_dir.canonicalize().unwrap()));
        assert!(!registered.contains(&nested_canonical));
        assert_eq!(
            file_rx
                .try_recv()
                .expect("root recovery notificationを期待"),
            root_md.canonicalize().unwrap()
        );
        let error = error_rx
            .try_recv()
            .expect("partial watch error eventを期待");
        assert_eq!(error.kind(), WatchErrorKind::Notify);
        assert!(error.detail().contains("nested watch failed"));
        assert!(
            file_rx.try_recv().is_err(),
            "未登録nested配下のMarkdownは通知しない"
        );
    }

    #[test]
    fn test_process_internal_events_追加watchのmax_files_watchはresource_exhaustedを送る() {
        let dir = tempfile::tempdir().unwrap();
        let new_dir = dir.path().join("new");
        std::fs::create_dir_all(&new_dir).unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![notify_debouncer_mini::DebouncedEvent::new(
            new_dir,
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let health_state = WatcherHealthState::new_alive();
        let (senders, _file_rx, mut error_rx) = split_senders_for_test(4, 4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());

        process_debounced_events_with_watch(
            events,
            &strategy,
            &senders,
            &health_state,
            &mut registered,
            |_path, _mode| Err(notify::Error::new(notify::ErrorKind::MaxFilesWatch)),
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        let error = error_rx
            .try_recv()
            .expect("dynamic watch resource exhausted eventを期待");
        assert_eq!(error.kind(), WatchErrorKind::ResourceExhausted);
        assert!(error.detail().contains("新規ディレクトリの監視追加に失敗"));
    }

    #[test]
    fn test_process_internal_events_登録済みディレクトリでもrefresh通知する() {
        let dir = tempfile::tempdir().unwrap();
        let existing_dir = dir.path().join("existing");
        std::fs::create_dir_all(&existing_dir).unwrap();
        let md = existing_dir.join("created.md");
        std::fs::write(&md, "# created").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![notify_debouncer_mini::DebouncedEvent::new(
            existing_dir.clone(),
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let health_state = WatcherHealthState::new_alive();
        let (senders, mut file_rx, _error_rx) = split_senders_for_test(4, 4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());
        registered.insert(existing_dir.canonicalize().unwrap());
        let mut watched = Vec::new();

        process_debounced_events_with_watch(
            events,
            &strategy,
            &senders,
            &health_state,
            &mut registered,
            |path, mode| {
                watched.push((path.to_path_buf(), mode));
                Ok(())
            },
        );

        let existing_canonical = existing_dir.canonicalize().unwrap();
        assert!(watched.iter().any(|(path, mode)| {
            path == &existing_canonical && *mode == notify::RecursiveMode::NonRecursive
        }));
        assert_eq!(
            file_rx
                .try_recv()
                .expect("refresh recovery notificationを期待"),
            md.canonicalize().unwrap()
        );
        assert_eq!(health_state.load(), WatcherHealth::Alive);
    }

    #[test]
    fn test_process_internal_events_登録済みディレクトリイベントでもwatchをrefreshする() {
        let dir = tempfile::tempdir().unwrap();
        let existing_dir = dir.path().join("existing");
        std::fs::create_dir_all(&existing_dir).unwrap();
        let md = existing_dir.join("created.md");
        std::fs::write(&md, "# created").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![notify_debouncer_mini::DebouncedEvent::new(
            existing_dir.clone(),
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let health_state = WatcherHealthState::new_alive();
        let (senders, mut file_rx, _error_rx) = split_senders_for_test(4, 4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());
        registered.insert(existing_dir.canonicalize().unwrap());
        let mut watched = Vec::new();
        let mut unwatched = Vec::new();

        process_debounced_events_with_watch_and_unwatch(
            events,
            &strategy,
            &senders,
            &health_state,
            &mut registered,
            |path, mode| {
                watched.push((path.to_path_buf(), mode));
                Ok(())
            },
            |path| {
                unwatched.push(path.to_path_buf());
                Ok(())
            },
        );

        let existing_canonical = existing_dir.canonicalize().unwrap();
        assert!(unwatched.iter().any(|path| path == &existing_canonical));
        assert!(watched.iter().any(|(path, mode)| {
            path == &existing_canonical && *mode == notify::RecursiveMode::NonRecursive
        }));
        assert_eq!(
            file_rx
                .try_recv()
                .expect("refresh recovery notificationを期待"),
            md.canonicalize().unwrap()
        );
        assert_eq!(health_state.load(), WatcherHealth::Alive);
    }

    #[cfg(unix)]
    #[test]
    fn test_process_internal_events_登録済みディレクトリのctime変化でもrefreshは正常終了する() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let dir = tempfile::tempdir().unwrap();
        let existing_dir = dir.path().join("existing");
        std::fs::create_dir_all(&existing_dir).unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());
        registered.insert(existing_dir.canonicalize().unwrap());
        let before = std::fs::metadata(&existing_dir).unwrap();
        let before_ctime = (before.ctime(), before.ctime_nsec());
        std::thread::sleep(Duration::from_millis(10));
        let mut permissions = before.permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&existing_dir, permissions).unwrap();
        let after = std::fs::metadata(&existing_dir).unwrap();
        assert_ne!(
            before_ctime,
            (after.ctime(), after.ctime_nsec()),
            "test setup should change directory ctime"
        );
        let events = vec![notify_debouncer_mini::DebouncedEvent::new(
            existing_dir.clone(),
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let health_state = WatcherHealthState::new_alive();
        let (senders, mut file_rx, _error_rx) = split_senders_for_test(4, 4);
        let mut watched = Vec::new();

        process_debounced_events_with_watch(
            events,
            &strategy,
            &senders,
            &health_state,
            &mut registered,
            |path, mode| {
                watched.push((path.to_path_buf(), mode));
                Ok(())
            },
        );

        assert!(watched.iter().any(|(path, mode)| {
            path == &existing_dir.canonicalize().unwrap()
                && *mode == notify::RecursiveMode::NonRecursive
        }));
        assert!(
            file_rx.try_recv().is_err(),
            "Markdownがなければrefreshしてもrecovery通知しない"
        );
        assert_eq!(health_state.load(), WatcherHealth::Alive);
    }

    #[test]
    fn test_process_internal_events_再作成された登録済みディレクトリは再watchする() {
        let dir = tempfile::tempdir().unwrap();
        let recreated_dir = dir.path().join("recreated");
        std::fs::create_dir(&recreated_dir).unwrap();
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());
        registered.insert(recreated_dir.canonicalize().unwrap());
        std::fs::remove_dir(&recreated_dir).unwrap();
        std::fs::create_dir(&recreated_dir).unwrap();
        let md = recreated_dir.join("created.md");
        std::fs::write(&md, "# recreated").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![notify_debouncer_mini::DebouncedEvent::new(
            recreated_dir.clone(),
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let health_state = WatcherHealthState::new_alive();
        let (senders, mut file_rx, _error_rx) = split_senders_for_test(4, 4);
        let mut watched = Vec::new();

        process_debounced_events_with_watch(
            events,
            &strategy,
            &senders,
            &health_state,
            &mut registered,
            |path, mode| {
                watched.push((path.to_path_buf(), mode));
                Ok(())
            },
        );

        let recreated_canonical = recreated_dir.canonicalize().unwrap();
        assert!(watched.iter().any(|(path, mode)| {
            path == &recreated_canonical && *mode == notify::RecursiveMode::NonRecursive
        }));
        assert_eq!(
            file_rx
                .try_recv()
                .expect("recreated recovery notificationを期待"),
            md.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_process_internal_events_recoveryは新規登録rootだけを通知する() {
        let dir = tempfile::tempdir().unwrap();
        let existing_dir = dir.path().join("existing");
        let new_dir = existing_dir.join("new");
        std::fs::create_dir_all(&new_dir).unwrap();
        let old_md = existing_dir.join("old.md");
        let new_md = new_dir.join("created.md");
        std::fs::write(&old_md, "# old").unwrap();
        std::fs::write(&new_md, "# created").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![notify_debouncer_mini::DebouncedEvent::new(
            new_dir.clone(),
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let health_state = WatcherHealthState::new_alive();
        let (senders, mut file_rx, _error_rx) = split_senders_for_test(4, 4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());
        registered.insert(existing_dir.canonicalize().unwrap());

        process_debounced_events_with_watch(
            events,
            &strategy,
            &senders,
            &health_state,
            &mut registered,
            |_path, _mode| Ok(()),
        );

        assert_eq!(
            file_rx
                .try_recv()
                .expect("new root recovery notificationを期待"),
            new_md.canonicalize().unwrap()
        );
        assert!(
            file_rx.try_recv().is_err(),
            "登録済み親ディレクトリ直下のMarkdownは再通知しない"
        );
    }

    #[test]
    fn test_process_internal_events_recoveryは新規subtreeを一度だけ走査する() {
        let dir = tempfile::tempdir().unwrap();
        let new_dir = dir.path().join("new");
        let nested = new_dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let md = nested.join("created.md");
        std::fs::write(&md, "# created").unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let events = vec![notify_debouncer_mini::DebouncedEvent::new(
            new_dir.clone(),
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let health_state = WatcherHealthState::new_alive();
        let (senders, mut file_rx, _error_rx) = split_senders_for_test(4, 4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());

        process_debounced_events_with_watch(
            events,
            &strategy,
            &senders,
            &health_state,
            &mut registered,
            |_path, _mode| Ok(()),
        );

        assert_eq!(
            file_rx
                .try_recv()
                .expect("nested recovery notificationを期待"),
            md.canonicalize().unwrap()
        );
        assert!(file_rx.try_recv().is_err(), "重複recovery通知は不要");
    }

    #[test]
    fn test_run_watcher_event_loop_internal_channel切断はhealth_failedとerror_eventを送る() {
        let (_dir, file_path) = create_markdown_fixture("watch.md", "# before");
        let strategy = WatchStrategy::from_mode(&AppMode::new_single_file(&file_path).unwrap())
            .expect("watch strategyを作成できる");
        let (internal_tx, internal_rx) = std::sync::mpsc::channel::<super::InternalWatchResult>();
        drop(internal_tx);
        let health_state = WatcherHealthState::new_alive();
        let shutdown_flag = AtomicBool::new(false);
        let (senders, _file_rx, mut error_rx) = split_senders_for_test(1, 1);

        assert!(matches!(
            internal_rx.recv_timeout(Duration::from_millis(1)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ));
        super::handle_internal_channel_disconnected(
            &strategy,
            &senders,
            &health_state,
            &shutdown_flag,
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        let error = error_rx
            .try_recv()
            .expect("internal channel error eventを期待");
        assert_eq!(error.kind(), WatchErrorKind::Notify);
        assert!(error.detail().contains("internal channel"));
    }

    #[test]
    fn test_send_internal_watch_result_満杯時はhealth_failedとerror_eventを送る() {
        let dir = tempfile::tempdir().unwrap();
        let strategy = WatchStrategy::Directory {
            base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
        let (internal_tx, _internal_rx) =
            std::sync::mpsc::sync_channel::<super::InternalWatchResult>(1);
        internal_tx
            .send(Ok(Vec::new()))
            .expect("internal channelを満杯にできる");
        let health_state = WatcherHealthState::new_alive();
        let (senders, _file_rx, mut error_rx) = split_senders_for_test(1, 1);

        super::send_internal_watch_result(
            &internal_tx,
            Ok(Vec::new()),
            &strategy,
            &senders,
            &health_state,
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        let error = error_rx
            .try_recv()
            .expect("internal channel full error eventを期待");
        assert_eq!(error.kind(), WatchErrorKind::Notify);
        assert!(error.detail().contains("internal channel が満杯"));
    }

    #[test]
    fn test_send_file_changed_eventはチャネル満杯時に破棄してブロックしない() {
        let (file_tx, mut file_rx) = mpsc::channel::<PathBuf>(1);
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        let (_dir2, second) = create_markdown_fixture("second.md", "# second");
        file_tx
            .blocking_send(first.clone())
            .expect("file channelを満杯にできる");
        let (merged_tx, _merged_rx) = mpsc::channel::<WatchEvent>(4);
        let diagnostics = WatcherDiagnostics::new(merged_tx, WatcherHealthState::new_alive());

        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let sender = std::thread::spawn(move || {
            super::send_file_changed_event(
                &file_tx,
                second,
                "filechanged満杯時テスト",
                &diagnostics,
            );
            done_tx.send(()).expect("完了通知を送信できる");
        });
        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("FileChanged満杯時の送信はブロックしない");
        sender.join().expect("filechanged送信threadが正常終了する");

        assert_eq!(
            file_rx.blocking_recv().expect("先行file eventを受信できる"),
            first
        );
        assert!(
            file_rx.try_recv().is_err(),
            "満杯時のFileChangedは破棄されているはず"
        );
    }

    #[test]
    fn test_send_file_changed_eventはclosed時にhealth_failedとerror_eventを記録する() {
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(1);
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);
        let health_state = WatcherHealthState::new_alive();
        let diagnostics = WatcherDiagnostics::new(merged_tx, health_state.clone());
        drop(file_rx);

        super::send_file_changed_event(
            &file_tx,
            PathBuf::from("closed.md"),
            "file channel closedテスト",
            &diagnostics,
        );
        super::send_file_changed_event(
            &file_tx,
            PathBuf::from("closed-again.md"),
            "file channel closedテスト",
            &diagnostics,
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::ForwarderStopped)
        );
        match merged_rx
            .try_recv()
            .expect("forwarder stopped error eventを期待")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::ForwarderStopped);
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }
        assert!(merged_rx.try_recv().is_err(), "closed診断は初回だけ送る");
    }

    #[tokio::test]
    async fn test_send_merged_file_changed_eventはerror用capacityを残す() {
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(1);
        let (_dir, changed) = create_markdown_fixture("changed.md", "# changed");

        assert!(send_merged_file_changed_event(&merged_tx, changed));

        assert!(
            merged_rx.try_recv().is_err(),
            "残容量が1以下ならFileChangedはmerged channelへ積まない"
        );
        assert_eq!(merged_tx.capacity(), 1);
    }

    #[traced_test]
    #[test]
    fn test_send_merged_file_changed_eventはclosed時にwarnを残す() {
        let (merged_tx, merged_rx) = mpsc::channel::<WatchEvent>(1);
        drop(merged_rx);

        assert!(!send_merged_file_changed_event(
            &merged_tx,
            PathBuf::from("closed.md")
        ));
        assert!(logs_contain(
            "監視イベント転送チャネルが閉じているためFileChangedを破棄しました"
        ));
    }
    #[test]
    fn test_notify_error経路はhealth_failedとerror_eventを記録する() {
        let (_dir, file_path) = create_markdown_fixture("watch.md", "# before");
        let strategy = WatchStrategy::from_mode(&AppMode::new_single_file(&file_path).unwrap())
            .expect("watch strategyを作成できる");
        let health_state = WatcherHealthState::new_starting();
        let (senders, _file_rx, mut error_rx) = split_senders_for_test(1, 1);

        handle_debounced_watch_result(
            Err(notify::Error::generic("notify detail")),
            &strategy,
            &senders,
            &health_state,
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        assert!(!matches!(health_state.load(), WatcherHealth::Alive));
        let error = error_rx.try_recv().expect("notify error eventを期待");
        assert_eq!(error.kind(), WatchErrorKind::Notify);
        assert_eq!(error.detail(), "notify detail");
    }

    #[test]
    fn test_notify_failed後もfilechanged通知は継続する() {
        let (_dir, file_path) = create_markdown_fixture("watch.md", "# before");
        let strategy = WatchStrategy::from_mode(&AppMode::new_single_file(&file_path).unwrap())
            .expect("watch strategyを作成できる");
        let health_state = WatcherHealthState::new_starting();
        let (senders, mut file_rx, mut error_rx) = split_senders_for_test(2, 2);

        handle_debounced_watch_result(
            Err(notify::Error::generic("notify detail")),
            &strategy,
            &senders,
            &health_state,
        );
        handle_debounced_watch_result(
            Ok(vec![notify_debouncer_mini::DebouncedEvent::new(
                file_path.clone(),
                notify_debouncer_mini::DebouncedEventKind::Any,
            )]),
            &strategy,
            &senders,
            &health_state,
        );

        assert_eq!(
            error_rx
                .try_recv()
                .expect("notify error eventを期待")
                .kind(),
            WatchErrorKind::Notify
        );
        assert_eq!(
            file_rx.try_recv().expect("file changed eventを期待"),
            file_path
        );
        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
    }
}
