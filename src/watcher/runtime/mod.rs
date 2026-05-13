mod dispatch;
mod error_queue;
mod health;
mod registration;
mod shutdown;
mod thread;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

use self::dispatch::{
    BestEffortFileSender, WatchEventSenders, WatcherDiagnostics, WATCHER_MESSAGE_BUFFER,
};
use self::error_queue::{
    priority_error_channel, PriorityErrorSender, WATCHER_ERROR_QUEUE_CAPACITY,
};
use self::health::WatcherHealthState;
use self::shutdown::{spawn_watch_event_merge_forwarder, MergeForwarderHandle};
use self::thread::{await_watcher_init, spawn_watcher_thread, InitResult, WatchRuntime};
use super::strategy::WatchStrategy;
use super::WatchEvent;
use crate::server::AppMode;

pub use self::health::{WatcherFailureKind, WatcherHealth};
pub(crate) use self::shutdown::WATCH_SHUTDOWN_TIMEOUT_SECS;

pub struct Watcher {
    runtime: Option<WatchRuntime>,
}
impl Watcher {
    /// 監視を開始し、監視イベント受信用チャネルを返す
    pub async fn spawn(mode: AppMode) -> Result<(Self, mpsc::Receiver<WatchEvent>)> {
        let strategy = WatchStrategy::from_mode(&mode)?;
        let watch_plan = strategy.watch_plan()?;
        let (merged_tx, merged_rx) = mpsc::channel::<WatchEvent>(WATCHER_MESSAGE_BUFFER);
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(WATCHER_MESSAGE_BUFFER);
        let (error_tx, error_rx) = priority_error_channel(WATCHER_ERROR_QUEUE_CAPACITY);
        let (init_tx, init_rx) = oneshot::channel::<InitResult>();
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let thread_shutdown_flag = shutdown_flag.clone();
        let health_state = WatcherHealthState::new_starting();
        let diagnostics = WatcherDiagnostics::new(merged_tx.clone(), health_state.clone());
        let file_tx = BestEffortFileSender::new(file_tx, diagnostics.clone());
        let senders = WatchEventSenders {
            file_tx: file_tx.clone(),
            error_tx: error_tx.clone(),
        };
        let merge_forwarder = spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        let thread_health_state = health_state.clone();
        let unexpected_exit = strategy.unexpected_exit_message();
        let watcher_thread = spawn_watcher_thread(
            strategy,
            watch_plan,
            senders,
            init_tx,
            thread_shutdown_flag,
            thread_health_state,
        )?;

        await_watcher_init(init_rx, unexpected_exit).await?;
        health_state.store_alive_if_starting();
        Ok((
            Self::new(
                shutdown_flag,
                watcher_thread,
                merge_forwarder,
                health_state,
                diagnostics,
                error_tx,
            ),
            merged_rx,
        ))
    }

    fn new(
        shutdown_flag: Arc<AtomicBool>,
        watcher_thread: std::thread::JoinHandle<()>,
        merge_forwarder: MergeForwarderHandle,
        health_state: WatcherHealthState,
        diagnostics: WatcherDiagnostics,
        error_tx: PriorityErrorSender,
    ) -> Self {
        Self {
            runtime: Some(WatchRuntime {
                shutdown_flag,
                watcher_thread,
                merge_forwarder,
                health_state,
                diagnostics,
                error_tx,
            }),
        }
    }

    /// watcher の現在状態を返す
    pub fn health(&self) -> WatcherHealth {
        self.runtime
            .as_ref()
            .map(|runtime| runtime.health_state.load())
            .unwrap_or(WatcherHealth::Stopped)
    }

    /// watcher が正常稼働中なら true
    pub fn is_alive(&self) -> bool {
        matches!(self.health(), WatcherHealth::Alive)
    }

    /// 監視スレッドを停止する。
    ///
    /// 停止処理は watcher thread の join と内部転送タスクの完了待ちを含む。
    /// watcher thread の join 待機は blocking pool に隔離して async runtime を塞がない。
    /// 呼び出し側は `.await` して最終 `WatcherHealth` を受け取る。
    pub async fn shutdown(mut self) -> WatcherHealth {
        if let Some(runtime) = self.runtime.take() {
            runtime.stop().await
        } else {
            WatcherHealth::Stopped
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            // Drop は async にできないため、Tokio worker 上で join timeout を待たない。
            tracing::warn!(
                "[markdown-view] Watcherがshutdown().awaitされずにdropされました。watcher thread panic の詳細を観測するには明示shutdownが必要です"
            );
            runtime.request_stop_without_wait();
        }
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::{mpsc, oneshot};

    use super::dispatch::{BestEffortFileSender, WatchEventSenders, WatcherDiagnostics};
    use super::error_queue::{priority_error_channel, PriorityErrorReceiver};
    use super::health::{WatcherHealth, WatcherHealthState};
    use super::thread::{handle_watcher_panic, InitResult};
    use super::Watcher;
    use crate::watcher::WatchEvent;
    pub(super) fn create_markdown_fixture(
        name: &str,
        content: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    pub(super) fn spawn_idle_watcher_thread(
        shutdown_flag: Arc<AtomicBool>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            while !shutdown_flag.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(1));
            }
        })
    }

    pub(super) fn spawn_init_panic_thread_for_test(
        panic_detail: &'static str,
    ) -> (
        oneshot::Receiver<InitResult>,
        PriorityErrorReceiver,
        WatcherHealthState,
        std::thread::JoinHandle<()>,
    ) {
        let health_state = WatcherHealthState::new_starting();
        let thread_health_state = health_state.clone();
        let (error_tx, error_rx) =
            priority_error_channel(super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER);
        let (init_tx, init_rx) = oneshot::channel::<InitResult>();

        let watcher_thread = std::thread::spawn(move || {
            let mut init_tx = Some(init_tx);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                std::panic::panic_any(String::from(panic_detail));
            }));

            if let Err(panic_payload) = result {
                handle_watcher_panic(
                    panic_payload,
                    "監視スレッドの初期化中にパニックを検出",
                    "監視スレッド初期化時パニック",
                    &thread_health_state,
                    &error_tx,
                    &mut init_tx,
                );
            }
        });

        (init_rx, error_rx, health_state, watcher_thread)
    }

    pub(super) fn split_senders_for_test(
        file_buffer: usize,
        error_buffer: usize,
    ) -> (
        WatchEventSenders,
        mpsc::Receiver<PathBuf>,
        PriorityErrorReceiver,
    ) {
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(file_buffer);
        let (error_tx, error_rx) = priority_error_channel(error_buffer.max(1));
        let (merged_tx, _merged_rx) = mpsc::channel::<WatchEvent>(4);
        let diagnostics = WatcherDiagnostics::new(merged_tx, WatcherHealthState::new_alive());
        (
            WatchEventSenders {
                file_tx: BestEffortFileSender::new(file_tx, diagnostics),
                error_tx,
            },
            file_rx,
            error_rx,
        )
    }

    pub(super) fn watcher_for_test(
        shutdown_flag: Arc<AtomicBool>,
        watcher_thread: std::thread::JoinHandle<()>,
        health_state: WatcherHealthState,
    ) -> (Watcher, mpsc::Receiver<WatchEvent>) {
        let (merged_tx, merged_rx) = mpsc::channel::<WatchEvent>(4);
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(4);
        let (error_tx, error_rx) =
            priority_error_channel(super::error_queue::WATCHER_ERROR_QUEUE_CAPACITY);
        let diagnostics = WatcherDiagnostics::new(merged_tx.clone(), health_state.clone());
        let merge_forwarder =
            super::shutdown::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        drop(file_tx);
        let watcher = Watcher::new(
            shutdown_flag,
            watcher_thread,
            merge_forwarder,
            health_state,
            diagnostics,
            error_tx,
        );
        (watcher, merged_rx)
    }

    pub(super) async fn shutdown_watcher_for_test(watcher: Watcher) -> WatcherHealth {
        watcher.shutdown().await
    }

    #[derive(Default)]
    pub(super) struct FakeWatchRegistrar {
        pub(super) fail_on: Option<std::path::PathBuf>,
        pub(super) watched: Vec<(std::path::PathBuf, notify::RecursiveMode)>,
    }

    impl FakeWatchRegistrar {
        pub(super) fn watch(
            &mut self,
            path: &std::path::Path,
            mode: notify::RecursiveMode,
        ) -> notify::Result<()> {
            if self.fail_on.as_deref() == Some(path) {
                return Err(notify::Error::generic("watch registration failed"));
            }
            self.watched.push((path.to_path_buf(), mode));
            Ok(())
        }
    }

    pub(super) async fn expect_note_md_file_changed(
        rx: &mut mpsc::Receiver<WatchEvent>,
        context: &str,
    ) -> std::path::PathBuf {
        const MAX_EVENTS: usize = 10;

        for _ in 0..MAX_EVENTS {
            let event = tokio::time::timeout(Duration::from_secs(3), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("{}: watch eventを受信できる", context))
                .unwrap_or_else(|| panic!("{}: watch event channelが閉じていない", context));

            match event {
                WatchEvent::FileChanged(path) => {
                    if path.file_name() == Some(std::ffi::OsStr::new("note.md")) {
                        return path;
                    }
                }
                WatchEvent::Error(error) => {
                    panic!("{}: FileChangedを期待したがError({})を受信", context, error);
                }
            }
        }

        panic!(
            "{}: 最大{}件のwatch event内にnote.mdのFileChangedがない",
            context, MAX_EVENTS
        );
    }

    #[cfg(unix)]
    pub(super) fn count_open_fds_under(root: &std::path::Path) -> Option<usize> {
        std::fs::read_dir("/proc/self/fd").ok().map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| std::fs::read_link(entry.path()).ok())
                .filter(|target| target.starts_with(root))
                .count()
        })
    }
}
