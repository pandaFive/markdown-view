use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use notify_debouncer_mini::new_debouncer;
use tokio::sync::{mpsc, oneshot};

use super::strategy::WatchStrategy;
use super::{WatchError, WatchEvent};
use crate::server::AppMode;

/// デバウンス間隔（ミリ秒）
const DEBOUNCE_MS: u64 = 300;
/// 監視スレッドのpark待機間隔（ミリ秒）
const WATCHER_THREAD_PARK_MS: u64 = 250;
/// notify から tokio へ橋渡しするチャネル容量
const WATCHER_MESSAGE_BUFFER: usize = 32;
/// watcher error を保持する内部ring queue容量。
///
/// FileChanged burst からは独立させつつ、異常storm時のメモリ上限を持たせる。
/// 64件に達した状態で新規errorをpushする場合は古いerrorからevictし、最新の診断を優先する。
const WATCHER_ERROR_QUEUE_CAPACITY: usize = 64;
/// ring queue の evict 境界を短いテストで固定するための小容量。
#[cfg(test)]
const WATCHER_ERROR_MESSAGE_BUFFER: usize = 8;
/// notify callback から watcher thread へ橋渡しする内部チャネル容量
const WATCHER_INTERNAL_EVENT_BUFFER: usize = 64;
/// shutdown() のグレースフル停止待機秒数
pub(crate) const WATCH_SHUTDOWN_TIMEOUT_SECS: u64 = 2;
/// shutdown診断イベントの送信待機上限（ミリ秒）
const WATCHER_DIAGNOSTIC_SEND_TIMEOUT_MS: u64 = 200;

type InitResult = std::result::Result<(), WatchError>;
type InternalWatchResult =
    std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>;

#[derive(Debug)]
struct WatchRegistrationFailure {
    registered_paths: Vec<PathBuf>,
    source: notify::Error,
}

#[derive(Debug, Default)]
struct WatchDirectoryRegistry {
    entries: HashSet<PathBuf>,
}

impl WatchDirectoryRegistry {
    fn insert(&mut self, path: PathBuf) {
        self.entries.insert(normalize_watch_registry_path(&path));
    }

    fn contains(&self, path: &Path) -> bool {
        self.entries.contains(&normalize_watch_registry_path(path))
    }

    fn remove_subtree(&mut self, root: &Path) -> Vec<PathBuf> {
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

    fn path_set(&self) -> HashSet<PathBuf> {
        self.entries.clone()
    }
}

fn normalize_watch_registry_path(path: &Path) -> PathBuf {
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

/// watcher の稼働状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WatcherHealth {
    /// watcher thread 起動から init 完了まで。先行 failure があれば Alive へは遷移しない
    Starting,
    /// watcher は正常に稼働中
    Alive,
    /// watcher は監視品質が劣化、または panic で停止している
    Failed(WatcherFailureKind),
    /// 停止処理中
    Stopping,
    /// 停止完了
    Stopped,
}

/// watcher failure の分類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WatcherFailureKind {
    /// notify callback がエラーを返した
    Notify,
    /// watcher thread が panic した
    ThreadPanic,
    /// shutdown を隔離する blocking task が panic した
    ShutdownTaskPanic,
    /// watcher thread の停止が timeout した
    ShutdownTimedOut,
    /// merge forwarder task が panic した
    ForwarderTaskPanic,
    /// merge forwarder が予期せず停止した
    ForwarderStopped,
}

#[derive(Debug, Clone)]
struct WatcherHealthState {
    state: Arc<AtomicU8>,
}

impl WatcherHealthState {
    const STARTING: u8 = 0;
    const ALIVE: u8 = 1;
    const FAILED_NOTIFY: u8 = 2;
    const FAILED_THREAD_PANIC: u8 = 3;
    const STOPPING: u8 = 4;
    const STOPPED: u8 = 5;
    const FAILED_SHUTDOWN_TASK_PANIC: u8 = 6;
    const FAILED_SHUTDOWN_TIMED_OUT: u8 = 7;
    const FAILED_FORWARDER_TASK_PANIC: u8 = 8;
    const FAILED_FORWARDER_STOPPED: u8 = 9;

    fn new_starting() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(Self::STARTING)),
        }
    }

    #[cfg(test)]
    fn new_alive() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(Self::ALIVE)),
        }
    }

    fn load(&self) -> WatcherHealth {
        match self.state.load(Ordering::Acquire) {
            Self::STARTING => WatcherHealth::Starting,
            Self::ALIVE => WatcherHealth::Alive,
            Self::FAILED_NOTIFY => WatcherHealth::Failed(WatcherFailureKind::Notify),
            Self::FAILED_THREAD_PANIC => WatcherHealth::Failed(WatcherFailureKind::ThreadPanic),
            Self::FAILED_SHUTDOWN_TASK_PANIC => {
                WatcherHealth::Failed(WatcherFailureKind::ShutdownTaskPanic)
            }
            Self::FAILED_SHUTDOWN_TIMED_OUT => {
                WatcherHealth::Failed(WatcherFailureKind::ShutdownTimedOut)
            }
            Self::FAILED_FORWARDER_TASK_PANIC => {
                WatcherHealth::Failed(WatcherFailureKind::ForwarderTaskPanic)
            }
            Self::FAILED_FORWARDER_STOPPED => {
                WatcherHealth::Failed(WatcherFailureKind::ForwarderStopped)
            }
            Self::STOPPING => WatcherHealth::Stopping,
            Self::STOPPED => WatcherHealth::Stopped,
            invalid => {
                debug_assert!(false, "不正なwatcher health state: {}", invalid);
                WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
            }
        }
    }

    fn store_alive_if_starting(&self) {
        let _ = self.state.compare_exchange(
            Self::STARTING,
            Self::ALIVE,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    fn store_stopping_if_not_failed(&self) {
        self.store_if_not_failed(Self::STOPPING);
    }

    fn store_stopped_if_not_failed(&self) {
        self.store_if_not_failed(Self::STOPPED);
    }

    fn store_failed(&self, kind: WatcherFailureKind) {
        let raw = match kind {
            WatcherFailureKind::Notify => Self::FAILED_NOTIFY,
            WatcherFailureKind::ThreadPanic => Self::FAILED_THREAD_PANIC,
            WatcherFailureKind::ShutdownTaskPanic => Self::FAILED_SHUTDOWN_TASK_PANIC,
            WatcherFailureKind::ShutdownTimedOut => Self::FAILED_SHUTDOWN_TIMED_OUT,
            WatcherFailureKind::ForwarderTaskPanic => Self::FAILED_FORWARDER_TASK_PANIC,
            WatcherFailureKind::ForwarderStopped => Self::FAILED_FORWARDER_STOPPED,
        };
        self.store_if_not_failed(raw);
    }

    fn store_if_not_failed(&self, raw: u8) {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            if Self::is_failed_raw(current) {
                return;
            }
            match self
                .state
                .compare_exchange(current, raw, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    fn is_failed_raw(raw: u8) -> bool {
        matches!(
            raw,
            Self::FAILED_NOTIFY
                | Self::FAILED_THREAD_PANIC
                | Self::FAILED_SHUTDOWN_TASK_PANIC
                | Self::FAILED_SHUTDOWN_TIMED_OUT
                | Self::FAILED_FORWARDER_TASK_PANIC
                | Self::FAILED_FORWARDER_STOPPED
        )
    }
}

/// 監視実行中ランタイム
///
/// 明示停止は `shutdown().await` が正規経路。
/// `Drop` は停止要求と内部転送タスクの abort だけを行い、watcher thread の join は待たない。
#[must_use = "Watcher は明示的に shutdown().await すべき"]
pub struct Watcher {
    runtime: Option<WatchRuntime>,
}

struct WatchRuntime {
    shutdown_flag: Arc<AtomicBool>,
    watcher_thread: std::thread::JoinHandle<()>,
    merge_forwarder: MergeForwarderHandle,
    health_state: WatcherHealthState,
    diagnostics: WatcherDiagnostics,
    error_tx: PriorityErrorSender,
}

#[derive(Clone)]
struct WatchEventSenders {
    file_tx: BestEffortFileSender,
    error_tx: PriorityErrorSender,
}

#[derive(Clone)]
struct BestEffortFileSender {
    tx: mpsc::Sender<PathBuf>,
    diagnostics: WatcherDiagnostics,
}

impl BestEffortFileSender {
    fn new(tx: mpsc::Sender<PathBuf>, diagnostics: WatcherDiagnostics) -> Self {
        Self { tx, diagnostics }
    }

    fn send(&self, path: PathBuf, label: &str) {
        send_file_changed_event(&self.tx, path, label, &self.diagnostics);
    }
}

#[derive(Clone)]
struct WatcherDiagnostics {
    merged_tx: mpsc::Sender<WatchEvent>,
    health_state: WatcherHealthState,
    file_sender_closed_reported: Arc<AtomicBool>,
}

impl WatcherDiagnostics {
    fn new(merged_tx: mpsc::Sender<WatchEvent>, health_state: WatcherHealthState) -> Self {
        Self {
            merged_tx,
            health_state,
            file_sender_closed_reported: Arc::new(AtomicBool::new(false)),
        }
    }

    async fn send_error_with_timeout(&self, error: WatchError, label: &str) {
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

    fn try_send_error(&self, error: WatchError, label: &str) {
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

    fn report_file_sender_closed(&self, label: &str) {
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

struct PriorityErrorSender {
    queue: Arc<PriorityErrorQueue>,
}

impl PriorityErrorSender {
    fn new(queue: Arc<PriorityErrorQueue>) -> Self {
        queue.sender_count.fetch_add(1, Ordering::Relaxed);
        Self { queue }
    }

    fn send(&self, error: WatchError, label: &str) {
        send_error_event(self, error, label);
    }
}

impl Clone for PriorityErrorSender {
    fn clone(&self) -> Self {
        Self::new(self.queue.clone())
    }
}

impl Drop for PriorityErrorSender {
    fn drop(&mut self) {
        if self.queue.sender_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            let mut state = self.queue.state.lock().expect("priority error queue mutex");
            state.closed = true;
            drop(state);
            self.queue.notify.notify_waiters();
        }
    }
}

struct PriorityErrorReceiver {
    queue: Arc<PriorityErrorQueue>,
}

impl PriorityErrorReceiver {
    async fn recv(&mut self) -> Option<WatchError> {
        loop {
            let notified = {
                let mut state = self.queue.state.lock().expect("priority error queue mutex");
                if let Some(error) = state.items.pop_front() {
                    return Some(error);
                }
                if state.closed {
                    return None;
                }
                self.queue.notify.notified()
            };
            notified.await;
        }
    }

    fn try_recv(&mut self) -> std::result::Result<WatchError, mpsc::error::TryRecvError> {
        let mut state = self.queue.state.lock().expect("priority error queue mutex");
        if let Some(error) = state.items.pop_front() {
            Ok(error)
        } else if state.closed {
            Err(mpsc::error::TryRecvError::Disconnected)
        } else {
            Err(mpsc::error::TryRecvError::Empty)
        }
    }
}

impl Drop for PriorityErrorReceiver {
    fn drop(&mut self) {
        let mut state = self.queue.state.lock().expect("priority error queue mutex");
        state.closed = true;
        state.items.clear();
        drop(state);
        self.queue.notify.notify_waiters();
    }
}

struct PriorityErrorQueue {
    state: Mutex<PriorityErrorQueueState>,
    notify: tokio::sync::Notify,
    sender_count: AtomicUsize,
    capacity: usize,
}

struct PriorityErrorQueueState {
    items: VecDeque<WatchError>,
    closed: bool,
}

fn priority_error_channel(capacity: usize) -> (PriorityErrorSender, PriorityErrorReceiver) {
    let queue = Arc::new(PriorityErrorQueue {
        state: Mutex::new(PriorityErrorQueueState {
            items: VecDeque::with_capacity(capacity),
            closed: false,
        }),
        notify: tokio::sync::Notify::new(),
        sender_count: AtomicUsize::new(0),
        capacity,
    });
    (
        PriorityErrorSender::new(queue.clone()),
        PriorityErrorReceiver { queue },
    )
}

enum WatcherThreadStopResult {
    Stopped,
    Panicked(Box<dyn std::any::Any + Send>),
    TimedOut,
}

struct MergeForwarderHandle {
    task: tokio::task::JoinHandle<()>,
    done_rx: oneshot::Receiver<()>,
}

struct ForwarderDoneOnDrop(Option<oneshot::Sender<()>>);

impl Drop for ForwarderDoneOnDrop {
    fn drop(&mut self) {
        if let Some(done_tx) = self.0.take() {
            // abort_without_wait経路では受信側が先にdropされていることがある。
            let _ = done_tx.send(());
        }
    }
}

impl MergeForwarderHandle {
    fn abort_without_wait(self) {
        self.task.abort();
    }

    #[cfg(test)]
    async fn await_completion(self) -> Result<(), tokio::task::JoinError> {
        self.task.await
    }

    async fn stop(self, timeout: Duration, diagnostics: &WatcherDiagnostics) {
        let MergeForwarderHandle { task, done_rx } = self;
        match tokio::time::timeout(timeout, done_rx).await {
            Ok(Ok(())) | Ok(Err(_)) => {
                if let Err(error) = task.await {
                    if error.is_cancelled() {
                        debug_assert!(
                            false,
                            "merge forwarder should not be cancelled before stop observes completion"
                        );
                        tracing::warn!(
                            "[markdown-view] 監視イベント合流タスクはcancel状態で終了しました"
                        );
                    } else {
                        tracing::warn!(
                            "[markdown-view] 監視イベント合流タスクの終了待機に失敗: {}",
                            error
                        );
                        diagnostics
                            .health_state
                            .store_failed(WatcherFailureKind::ForwarderTaskPanic);
                        diagnostics
                            .send_error_with_timeout(
                                WatchError::forwarder_task_panic(format!(
                                    "merge forwarder join failed: {error}"
                                )),
                                "監視イベント合流タスクpanic",
                            )
                            .await;
                    }
                }
            }
            Err(_) => {
                tracing::warn!(
                    timeout_secs = timeout.as_secs(),
                    "[markdown-view] 監視イベント合流タスクの停止がタイムアウトしたためabortします"
                );
                task.abort();
                if let Err(error) = task.await {
                    if error.is_cancelled() {
                        return;
                    }
                    tracing::warn!(
                        "[markdown-view] 監視イベント合流タスクのabort後joinに失敗: {}",
                        error
                    );
                    diagnostics
                        .health_state
                        .store_failed(WatcherFailureKind::ForwarderTaskPanic);
                    diagnostics
                        .send_error_with_timeout(
                            WatchError::forwarder_task_panic(format!(
                                "merge forwarder abort join failed: {error}"
                            )),
                            "監視イベント合流タスクabort後panic",
                        )
                        .await;
                }
            }
        }
    }
}

impl WatchRuntime {
    /// 監視スレッドに停止を通知し、完了を待機する
    ///
    /// `WATCH_SHUTDOWN_TIMEOUT_SECS` 以内にスレッドが終了しない場合はjoin待機を打ち切り、
    /// thread handleをdetachする（プロセス終了時にOSが回収する）。
    async fn stop(self) -> WatcherHealth {
        self.stop_with_timeout(
            Duration::from_secs(WATCH_SHUTDOWN_TIMEOUT_SECS),
            Duration::from_millis(50),
        )
        .await
    }

    async fn stop_with_timeout(self, timeout: Duration, poll_interval: Duration) -> WatcherHealth {
        let WatchRuntime {
            shutdown_flag,
            watcher_thread,
            merge_forwarder,
            health_state,
            diagnostics,
            error_tx,
        } = self;
        let before_stop = health_state.load();
        if matches!(before_stop, WatcherHealth::Failed(_)) {
            tracing::warn!(
                "[markdown-view] 失敗状態の監視スレッドを停止します: {:?}",
                before_stop
            );
        }
        health_state.store_stopping_if_not_failed();
        shutdown_flag.store(true, Ordering::Release);
        watcher_thread.thread().unpark();

        let start = std::time::Instant::now();
        let join_result = tokio::task::spawn_blocking(move || {
            join_watcher_thread_with_timeout(watcher_thread, timeout, poll_interval)
        })
        .await;
        let elapsed = start.elapsed();

        match join_result {
            Ok(WatcherThreadStopResult::TimedOut) => {
                tracing::warn!(
                    elapsed_ms = elapsed.as_millis(),
                    timeout_secs = timeout.as_secs(),
                    "[markdown-view] 監視スレッドの停止がタイムアウトしました"
                );
                health_state.store_failed(WatcherFailureKind::ShutdownTimedOut);
                diagnostics
                    .send_error_with_timeout(
                        WatchError::shutdown_timed_out(format!(
                            "watcher thread stop timed out after {} ms",
                            elapsed.as_millis()
                        )),
                        "監視スレッド停止timeout",
                    )
                    .await;
                merge_forwarder.stop(Duration::ZERO, &diagnostics).await;
                return health_state.load();
            }
            Ok(WatcherThreadStopResult::Panicked(panic_payload)) => {
                handle_watcher_panic(
                    panic_payload,
                    "監視スレッドの停止中にパニックを検出",
                    "監視スレッド停止時パニック",
                    &health_state,
                    &error_tx,
                );
            }
            Ok(WatcherThreadStopResult::Stopped) => {
                health_state.store_stopped_if_not_failed();
            }
            Err(error) => {
                record_shutdown_task_panic(&health_state, &error_tx, error);
            }
        }

        let health = health_state.load();
        drop(error_tx);
        merge_forwarder
            .stop(timeout.saturating_sub(elapsed), &diagnostics)
            .await;
        health
    }

    fn request_stop_without_wait(self) {
        self.health_state.store_stopping_if_not_failed();
        self.shutdown_flag.store(true, Ordering::Release);
        self.watcher_thread.thread().unpark();
        self.merge_forwarder.abort_without_wait();
    }
}

fn record_shutdown_task_panic(
    health_state: &WatcherHealthState,
    error_tx: &PriorityErrorSender,
    error: tokio::task::JoinError,
) {
    health_state.store_failed(WatcherFailureKind::ShutdownTaskPanic);
    error_tx.send(
        WatchError::shutdown_task_panic(format!("shutdown task panic: {error}")),
        "監視スレッド停止処理パニック",
    );
    tracing::warn!(
        "[markdown-view] 監視スレッド停止処理のjoinに失敗: {}",
        error
    );
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

fn join_watcher_thread_with_timeout(
    watcher_thread: std::thread::JoinHandle<()>,
    timeout: Duration,
    poll_interval: Duration,
) -> WatcherThreadStopResult {
    let start = std::time::Instant::now();
    while !watcher_thread.is_finished() {
        if start.elapsed() > timeout {
            return WatcherThreadStopResult::TimedOut;
        }
        std::thread::sleep(poll_interval);
    }
    match watcher_thread.join() {
        Ok(()) => WatcherThreadStopResult::Stopped,
        Err(panic_payload) => WatcherThreadStopResult::Panicked(panic_payload),
    }
}

/// 分離した file/error 入力経路を既存公開APIの `WatchEvent` channel へ再統合する。
///
/// error は FileChanged より優先して drain し、FileChanged は merged channel の
/// 残容量1件を error 用に予約する。
fn spawn_watch_event_merge_forwarder(
    mut file_rx: mpsc::Receiver<PathBuf>,
    mut error_rx: PriorityErrorReceiver,
    merged_tx: mpsc::Sender<WatchEvent>,
) -> MergeForwarderHandle {
    let (done_tx, done_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let _done = ForwarderDoneOnDrop(Some(done_tx));
        loop {
            while let Ok(error) = error_rx.try_recv() {
                if merged_tx.send(WatchEvent::Error(error)).await.is_err() {
                    return;
                }
            }

            tokio::select! {
                biased;
                // Error は FileChanged backlog から分離した優先経路なので、
                // 両方readyなら必ずerrorを先に外部WatchEventへ戻す。

                error = error_rx.recv() => {
                    match error {
                        Some(error) => {
                            if merged_tx.send(WatchEvent::Error(error)).await.is_err() {
                                break;
                            }
                        }
                        None => {
                            while let Some(path) = file_rx.recv().await {
                                if !send_merged_file_changed_event(&merged_tx, path) {
                                    return;
                                }
                            }
                            break;
                        }
                    }
                }
                path = file_rx.recv() => {
                    match path {
                        Some(path) => {
                            if !send_merged_file_changed_event(&merged_tx, path) {
                                break;
                            }
                        }
                        None => {
                            while let Some(error) = error_rx.recv().await {
                                if merged_tx.send(WatchEvent::Error(error)).await.is_err() {
                                    return;
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
    });
    MergeForwarderHandle { task, done_rx }
}

/// FileChanged を外部公開用 merged channel へbest-effort転送する。
///
/// 残容量1件は Error 用に予約し、FileChanged burst が watcher error の合流を
/// 塞がないようにする。
fn send_merged_file_changed_event(tx: &mpsc::Sender<WatchEvent>, path: PathBuf) -> bool {
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
fn send_file_changed_event(
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
fn send_error_event(tx: &PriorityErrorSender, error: WatchError, label: &str) {
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

fn spawn_watcher_thread(
    strategy: WatchStrategy,
    watch_plan: super::strategy::WatchPlan,
    senders: WatchEventSenders,
    init_tx: oneshot::Sender<InitResult>,
    thread_shutdown_flag: Arc<AtomicBool>,
    health_state: WatcherHealthState,
) -> Result<std::thread::JoinHandle<()>> {
    let thread_name = strategy.thread_name().to_string();
    let spawn_context = format!("監視スレッド {} の起動に失敗", strategy.thread_name());
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let rt_senders = senders;
            let panic_error_tx = rt_senders.error_tx.clone();
            let mut init_tx = Some(init_tx);
            let start_error_prefix = strategy.start_error_prefix();
            let panic_message = strategy.panic_message();
            let error_label = strategy.error_label();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let (internal_tx, internal_rx) =
                    std::sync::mpsc::sync_channel::<InternalWatchResult>(
                        WATCHER_INTERNAL_EVENT_BUFFER,
                );
                let callback_strategy = strategy.clone();
                let callback_senders = rt_senders.clone();
                let callback_health_state = health_state.clone();
                let debouncer = new_debouncer(
                    Duration::from_millis(DEBOUNCE_MS),
                    move |res: InternalWatchResult| {
                        send_internal_watch_result(
                            &internal_tx,
                            res,
                            &callback_strategy,
                            &callback_senders,
                            &callback_health_state,
                        );
                    },
                );

                let mut debouncer = match debouncer {
                    Ok(d) => d,
                    Err(e) => {
                        send_init_result(
                            &mut init_tx,
                            Err(WatchError::init(format!("debouncerの初期化に失敗: {}", e))),
                        );
                        return;
                    }
                };

                let mut registered_paths = WatchDirectoryRegistry::default();
                if let Err(failure) =
                    register_watch_plan_with(&watch_plan, &mut registered_paths, |path, mode| {
                        debouncer.watcher().watch(path, mode)
                    })
                {
                    send_init_result(
                        &mut init_tx,
                        Err(WatchError::from_watch_init_error(
                            start_error_prefix,
                            &failure.source,
                        )),
                    );
                    return;
                }

                send_init_result(&mut init_tx, Ok(()));
                run_watcher_event_loop(
                    &mut debouncer,
                    internal_rx,
                    &strategy,
                    &rt_senders,
                    &health_state,
                    &thread_shutdown_flag,
                    &mut registered_paths,
                );
            }));

            if let Err(panic_payload) = result {
                handle_watcher_panic(
                    panic_payload,
                    panic_message,
                    error_label,
                    &health_state,
                    &panic_error_tx,
                );
            }
        })
        .context(spawn_context)
}

fn register_watch_plan_with<F>(
    plan: &super::strategy::WatchPlan,
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

fn send_internal_watch_result(
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

fn handle_debounced_watch_result(
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
fn process_debounced_events_with_watch<F>(
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

fn process_debounced_events_with_watch_and_unwatch<F, U>(
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
        let plan =
            match super::strategy::WatchPlan::for_new_subtree(&candidate, &registered_path_set) {
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
        for markdown in super::strategy::collect_markdown_files_for_recovery_under_watched_dirs(
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

fn handle_watcher_panic(
    panic_payload: Box<dyn std::any::Any + Send>,
    panic_message: &str,
    error_label: &str,
    health_state: &WatcherHealthState,
    error_tx: &PriorityErrorSender,
) {
    let panic_detail = if let Some(s) = panic_payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(error) = panic_payload.downcast_ref::<anyhow::Error>() {
        error.to_string()
    } else {
        "不明なパニック".to_string()
    };
    health_state.store_failed(WatcherFailureKind::ThreadPanic);
    let watch_error = WatchError::thread_panic(panic_detail.clone());
    tracing::error!("[markdown-view] {}: {}", panic_message, panic_detail);
    error_tx.send(watch_error, error_label);
}

fn send_init_result(init_tx: &mut Option<oneshot::Sender<InitResult>>, result: InitResult) {
    if let Some(tx) = init_tx.take() {
        if tx.send(result).is_err() {
            tracing::warn!("[markdown-view] 初期化通知先が既に閉じています");
        }
    } else {
        tracing::warn!(
            "[markdown-view] send_init_resultが二重に呼び出されました（結果を破棄）: {:?}",
            result.err()
        );
    }
}

fn run_watcher_event_loop(
    debouncer: &mut notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
    internal_rx: std::sync::mpsc::Receiver<InternalWatchResult>,
    strategy: &WatchStrategy,
    senders: &WatchEventSenders,
    health_state: &WatcherHealthState,
    shutdown_flag: &AtomicBool,
    registered_paths: &mut WatchDirectoryRegistry,
) {
    while !shutdown_flag.load(Ordering::Acquire) {
        match internal_rx.recv_timeout(Duration::from_millis(WATCHER_THREAD_PARK_MS)) {
            Ok(Ok(events)) => {
                let watcher = std::cell::RefCell::new(debouncer.watcher());
                process_debounced_events_with_watch_and_unwatch(
                    events,
                    strategy,
                    senders,
                    health_state,
                    registered_paths,
                    |path, mode| watcher.borrow_mut().watch(path, mode),
                    |path| watcher.borrow_mut().unwatch(path),
                );
            }
            Ok(Err(error)) => {
                handle_debounced_watch_result(Err(error), strategy, senders, health_state);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                handle_internal_channel_disconnected(
                    strategy,
                    senders,
                    health_state,
                    shutdown_flag,
                );
                break;
            }
        }
    }
}

fn handle_internal_channel_disconnected(
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

async fn await_watcher_init(
    init_rx: oneshot::Receiver<InitResult>,
    unexpected_exit: &'static str,
) -> Result<()> {
    match init_rx.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(anyhow::Error::new(e)),
        Err(_) => Err(anyhow::Error::new(WatchError::init(unexpected_exit))),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::{mpsc, oneshot};
    use tracing_test::traced_test;

    use super::{
        handle_debounced_watch_result, handle_watcher_panic, priority_error_channel,
        process_debounced_events_with_watch, process_debounced_events_with_watch_and_unwatch,
        register_watch_plan_with, BestEffortFileSender, PriorityErrorReceiver,
        WatchDirectoryRegistry, WatchEventSenders, Watcher, WatcherDiagnostics, WatcherFailureKind,
        WatcherHealth, WatcherHealthState,
    };
    use crate::server::AppMode;
    use crate::watcher::strategy::WatchStrategy;
    use crate::watcher::{WatchError, WatchErrorKind, WatchEvent};

    fn create_markdown_fixture(
        name: &str,
        content: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    fn spawn_idle_watcher_thread(shutdown_flag: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            while !shutdown_flag.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(1));
            }
        })
    }

    fn split_senders_for_test(
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

    fn watcher_for_test(
        shutdown_flag: Arc<AtomicBool>,
        watcher_thread: std::thread::JoinHandle<()>,
        health_state: WatcherHealthState,
    ) -> (Watcher, mpsc::Receiver<WatchEvent>) {
        let (merged_tx, merged_rx) = mpsc::channel::<WatchEvent>(4);
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(4);
        let (error_tx, error_rx) = priority_error_channel(super::WATCHER_ERROR_QUEUE_CAPACITY);
        let diagnostics = WatcherDiagnostics::new(merged_tx.clone(), health_state.clone());
        let merge_forwarder =
            super::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
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

    async fn shutdown_watcher_for_test(watcher: Watcher) -> WatcherHealth {
        watcher.shutdown().await
    }

    #[derive(Default)]
    struct FakeWatchRegistrar {
        fail_on: Option<std::path::PathBuf>,
        watched: Vec<(std::path::PathBuf, notify::RecursiveMode)>,
    }

    impl FakeWatchRegistrar {
        fn watch(
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

    async fn expect_note_md_file_changed(
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
    fn count_open_fds_under(root: &std::path::Path) -> Option<usize> {
        std::fs::read_dir("/proc/self/fd").ok().map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| std::fs::read_link(entry.path()).ok())
                .filter(|target| target.starts_with(root))
                .count()
        })
    }

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
    async fn test_error_eventはfile_channel満杯時もmerged_rxに届く() {
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(1);
        let (error_tx, error_rx) = priority_error_channel(super::WATCHER_ERROR_QUEUE_CAPACITY);
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        file_tx
            .send(first)
            .await
            .expect("file channelを満杯にできる");

        let send_error_tx = error_tx.clone();
        tokio::task::spawn_blocking(move || {
            send_error_tx.send(
                WatchError::notify("file channelが満杯でも送達する"),
                "error分離テスト",
            );
        })
        .await
        .expect("error event送信taskが正常終了する");

        let forwarder = super::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        drop(file_tx);
        drop(error_tx);

        let mut delivered_error = None;
        for _ in 0..2 {
            let event = tokio::time::timeout(Duration::from_secs(1), merged_rx.recv())
                .await
                .expect("merged eventを待てる")
                .expect("merged eventを受信できる");
            if let WatchEvent::Error(error) = event {
                delivered_error = Some(error);
                break;
            }
        }

        let error = delivered_error.expect("Error eventが送達される");
        assert_eq!(error.kind(), WatchErrorKind::Notify);
        assert_eq!(error.detail(), "file channelが満杯でも送達する");

        forwarder
            .await_completion()
            .await
            .expect("merge forwarderが正常終了する");
    }

    #[tokio::test]
    async fn test_merge_forwarderはfileよりerrorを優先する() {
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(4);
        let (error_tx, error_rx) = priority_error_channel(super::WATCHER_ERROR_QUEUE_CAPACITY);
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);
        let (_dir, changed) = create_markdown_fixture("changed.md", "# changed");
        file_tx.send(changed).await.expect("file eventを送信できる");
        error_tx.send(WatchError::notify("優先されるerror"), "優先テスト");

        let forwarder = super::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        drop(file_tx);
        drop(error_tx);

        match tokio::time::timeout(Duration::from_secs(1), merged_rx.recv())
            .await
            .expect("最初のmerged eventを待てる")
            .expect("最初のmerged eventを受信できる")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.detail(), "優先されるerror");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Error優先を期待したがFileChanged({path:?})を受信")
            }
        }

        match tokio::time::timeout(Duration::from_secs(1), merged_rx.recv())
            .await
            .expect("2つ目のmerged eventを待てる")
            .expect("2つ目のmerged eventを受信できる")
        {
            WatchEvent::FileChanged(_) => {}
            WatchEvent::Error(error) => {
                panic!("2つ目はFileChangedを期待したがError({error})を受信")
            }
        }

        forwarder
            .await_completion()
            .await
            .expect("merge forwarderが正常終了する");
    }

    #[tokio::test]
    async fn test_merge_forwarderはmerged_backlogでerror_drainを止めない() {
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(4);
        let (error_tx, error_rx) = priority_error_channel(super::WATCHER_ERROR_QUEUE_CAPACITY);
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(1);
        let (_dir, prefilled) = create_markdown_fixture("prefilled.md", "# prefilled");
        let (_dir2, changed) = create_markdown_fixture("changed.md", "# changed");
        merged_tx
            .send(WatchEvent::FileChanged(prefilled.clone()))
            .await
            .expect("merged channelを満杯にできる");
        file_tx.send(changed).await.expect("file eventを送信できる");

        let forwarder = super::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        tokio::task::yield_now().await;
        error_tx.send(
            WatchError::notify("merged backlogでも優先されるerror"),
            "merged backlogテスト",
        );
        drop(file_tx);
        drop(error_tx);

        match tokio::time::timeout(Duration::from_secs(1), merged_rx.recv())
            .await
            .expect("merged eventを待てる")
            .expect("merged eventを受信できる")
        {
            WatchEvent::FileChanged(path) => assert_eq!(path, prefilled),
            WatchEvent::Error(error) => {
                panic!("先行FileChangedを期待したがError({error})を受信")
            }
        }

        match tokio::time::timeout(Duration::from_secs(1), merged_rx.recv())
            .await
            .expect("error eventを待てる")
            .expect("error eventを受信できる")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.detail(), "merged backlogでも優先されるerror");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }

        forwarder
            .await_completion()
            .await
            .expect("merge forwarderが正常終了する");
    }

    #[tokio::test]
    async fn test_error_eventはfile_burst中にbuffer件数を超えても破棄されない() {
        const ERROR_COUNT: usize = super::WATCHER_ERROR_MESSAGE_BUFFER + 4;

        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(super::WATCHER_MESSAGE_BUFFER);
        let (error_tx, error_rx) = priority_error_channel(ERROR_COUNT);
        let error_sender = error_tx.clone();
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(1);
        let (_dir, prefilled) = create_markdown_fixture("prefilled.md", "# prefilled");
        merged_tx
            .send(WatchEvent::FileChanged(prefilled))
            .await
            .expect("merged channelを満杯にできる");

        for index in 0..super::WATCHER_MESSAGE_BUFFER {
            file_tx
                .send(PathBuf::from(format!("burst-{index}.md")))
                .await
                .expect("file burstを送信できる");
        }
        for index in 0..ERROR_COUNT {
            error_sender.send(
                WatchError::notify(format!("error-{index}")),
                "error burstテスト",
            );
        }
        drop(error_sender);

        let forwarder = super::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        drop(file_tx);
        drop(error_tx);

        let mut delivered_errors = Vec::new();
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_secs(1), merged_rx.recv()).await
        {
            if let WatchEvent::Error(error) = event {
                delivered_errors.push(error.detail().to_string());
                if delivered_errors.len() == ERROR_COUNT {
                    break;
                }
            }
        }

        assert_eq!(
            delivered_errors.len(),
            ERROR_COUNT,
            "file burst中でもerror eventを内部buffer件数で破棄しない"
        );
        forwarder
            .await_completion()
            .await
            .expect("merge forwarderが正常終了する");
    }

    #[tokio::test]
    async fn test_merge_forwarderはfile側close後もerrorをdrainする() {
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(4);
        let (error_tx, error_rx) = priority_error_channel(super::WATCHER_ERROR_QUEUE_CAPACITY);
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);

        error_tx.send(
            WatchError::notify("file close後のerror-1"),
            "片側closeテスト",
        );
        error_tx.send(
            WatchError::notify("file close後のerror-2"),
            "片側closeテスト",
        );
        drop(file_tx);

        let forwarder = super::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        drop(error_tx);

        let mut details = Vec::new();
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_secs(1), merged_rx.recv()).await
        {
            if let WatchEvent::Error(error) = event {
                details.push(error.detail().to_string());
            }
        }

        assert_eq!(
            details,
            vec!["file close後のerror-1", "file close後のerror-2"]
        );
        forwarder
            .await_completion()
            .await
            .expect("merge forwarderが正常終了する");
    }

    #[tokio::test]
    async fn test_merge_forwarderはerror側close後もfileをdrainする() {
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(4);
        let (error_tx, error_rx) = priority_error_channel(super::WATCHER_ERROR_QUEUE_CAPACITY);
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);
        let (_dir, first) = create_markdown_fixture("error-close-first.md", "# first");
        let (_dir2, second) = create_markdown_fixture("error-close-second.md", "# second");

        file_tx
            .send(first.clone())
            .await
            .expect("file eventを送信できる");
        file_tx
            .send(second.clone())
            .await
            .expect("file eventを送信できる");
        drop(error_tx);

        let forwarder = super::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        drop(file_tx);

        let mut paths = Vec::new();
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_secs(1), merged_rx.recv()).await
        {
            if let WatchEvent::FileChanged(path) = event {
                paths.push(path);
            }
        }

        assert_eq!(paths, vec![first, second]);
        forwarder
            .await_completion()
            .await
            .expect("merge forwarderが正常終了する");
    }

    #[traced_test]
    #[tokio::test]
    async fn test_merge_forwarder_stopはtimeout時にabort後joinする() {
        let (done_tx, done_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _done_tx = done_tx;
            std::future::pending::<()>().await;
        });
        let forwarder = super::MergeForwarderHandle { task, done_rx };
        let (merged_tx, _merged_rx) = mpsc::channel::<WatchEvent>(4);
        let diagnostics = WatcherDiagnostics::new(merged_tx, WatcherHealthState::new_alive());

        forwarder.stop(Duration::ZERO, &diagnostics).await;

        assert!(logs_contain(
            "監視イベント合流タスクの停止がタイムアウトしたためabortします"
        ));
    }

    #[tokio::test]
    async fn test_merge_forwarder_stopはpanicをhealthとerror_eventに記録する() {
        let (done_tx, done_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _done = super::ForwarderDoneOnDrop(Some(done_tx));
            panic!("forwarder panic detail");
        });
        let forwarder = super::MergeForwarderHandle { task, done_rx };
        let health_state = WatcherHealthState::new_alive();
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);
        let diagnostics = WatcherDiagnostics::new(merged_tx, health_state.clone());

        forwarder.stop(Duration::from_secs(1), &diagnostics).await;

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::ForwarderTaskPanic)
        );
        match merged_rx
            .recv()
            .await
            .expect("forwarder panic error eventを期待")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::ForwarderTaskPanic);
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }
    }

    #[tokio::test]
    async fn test_send_merged_file_changed_eventはerror用capacityを残す() {
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(1);
        let (_dir, changed) = create_markdown_fixture("changed.md", "# changed");

        assert!(super::send_merged_file_changed_event(&merged_tx, changed));

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

        assert!(!super::send_merged_file_changed_event(
            &merged_tx,
            PathBuf::from("closed.md")
        ));
        assert!(logs_contain(
            "監視イベント転送チャネルが閉じているためFileChangedを破棄しました"
        ));
    }

    #[traced_test]
    #[test]
    fn test_send_error_eventはreceiver_drop後にclosedとして扱い未配送errorを保持しない() {
        let (error_tx, error_rx) = priority_error_channel(super::WATCHER_ERROR_MESSAGE_BUFFER);

        error_tx.send(
            WatchError::notify("queued before receiver drop"),
            "error receiver closedテスト",
        );
        assert_eq!(
            error_tx
                .queue
                .state
                .lock()
                .expect("priority error queue mutex")
                .items
                .len(),
            1
        );
        drop(error_rx);

        error_tx.send(
            WatchError::notify("secret receiver closed detail"),
            "error receiver closedテスト",
        );

        let state = error_tx
            .queue
            .state
            .lock()
            .expect("priority error queue mutex");
        assert!(state.closed);
        assert!(state.items.is_empty());
        drop(state);
        assert!(logs_contain(
            "watcher error channel が閉じているため異常通知を破棄しました"
        ));
        assert!(logs_contain("error_kind=Notify"));
        assert!(!logs_contain("secret receiver closed detail"));
        assert!(!logs_contain("queued before receiver drop"));
    }

    #[test]
    fn test_send_error_eventはcapacity超過時に最古をevictして最新を保持する() {
        let (error_tx, mut error_rx) = priority_error_channel(super::WATCHER_ERROR_MESSAGE_BUFFER);

        for index in 0..super::WATCHER_ERROR_MESSAGE_BUFFER + 4 {
            error_tx.send(
                WatchError::notify(format!("error-{index}")),
                "error ring queueテスト",
            );
        }

        let mut received = Vec::new();
        while let Ok(error) = error_rx.try_recv() {
            received.push(error.detail().to_string());
        }

        assert_eq!(received.len(), super::WATCHER_ERROR_MESSAGE_BUFFER);
        assert_eq!(received.first().map(String::as_str), Some("error-4"));
        assert_eq!(received.last().map(String::as_str), Some("error-11"));
    }

    #[tokio::test]
    async fn test_watcher_spawn_単一ファイルモードでイベント受信できる() {
        let (_dir, file_path) = create_markdown_fixture("watch.md", "# before");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (watcher, mut rx) = Watcher::spawn(mode).await.unwrap();

        tokio::fs::write(&file_path, "# after").await.unwrap();

        let received = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .unwrap();
        match received {
            WatchEvent::FileChanged(changed_path) => {
                assert_eq!(
                    changed_path.file_name(),
                    Some(std::ffi::OsStr::new("watch.md"))
                );
            }
            WatchEvent::Error(error) => {
                panic!("FileChangedを期待したが Error({}) を受信", error)
            }
        }

        shutdown_watcher_for_test(watcher).await;
    }

    #[tokio::test]
    async fn test_watcher_spawn_ディレクトリモードでイベント受信できる() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("watch.md");
        tokio::fs::write(&file_path, "# before").await.unwrap();
        let mode = AppMode::new_directory(dir.path()).unwrap();
        let (watcher, mut rx) = Watcher::spawn(mode).await.unwrap();

        tokio::fs::write(&file_path, "# after").await.unwrap();

        let received = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, WatchEvent::FileChanged(file_path.clone()));

        shutdown_watcher_for_test(watcher).await;
    }

    #[tokio::test]
    async fn test_watcher_spawn_ディレクトリモードで起動後新規サブディレクトリを監視する() {
        let dir = tempfile::tempdir().unwrap();
        let initial = dir.path().join("initial.md");
        tokio::fs::write(&initial, "# initial").await.unwrap();
        let mode = AppMode::new_directory(dir.path()).unwrap();
        let (watcher, mut rx) = Watcher::spawn(mode).await.unwrap();

        let new_dir = dir.path().join("new-section");
        tokio::fs::create_dir_all(&new_dir).await.unwrap();
        let new_file = new_dir.join("note.md");
        tokio::fs::write(&new_file, "# before").await.unwrap();

        let first =
            expect_note_md_file_changed(&mut rx, "新規ディレクトリ作成後の回復通知または更新通知")
                .await;
        assert_eq!(first.file_name(), Some(std::ffi::OsStr::new("note.md")));

        tokio::fs::write(&new_file, "# after").await.unwrap();

        let second =
            expect_note_md_file_changed(&mut rx, "新規サブディレクトリ配下の更新通知").await;
        assert_eq!(second.file_name(), Some(std::ffi::OsStr::new("note.md")));

        shutdown_watcher_for_test(watcher).await;
    }

    #[tokio::test]
    async fn test_watcher_health_生成直後はaliveを返す() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let (watcher, _rx) = watcher_for_test(shutdown_flag, watcher_thread, health_state);

        assert_eq!(watcher.health(), WatcherHealth::Alive);
        assert!(watcher.is_alive());

        shutdown_watcher_for_test(watcher).await;
    }

    #[test]
    fn test_watcher_panic経路はhealth_failedとerror_eventを記録する() {
        let health_state = WatcherHealthState::new_starting();
        let (error_tx, mut error_rx) = priority_error_channel(super::WATCHER_ERROR_MESSAGE_BUFFER);

        handle_watcher_panic(
            Box::new(String::from("panic detail")),
            "panic message",
            "panic label",
            &health_state,
            &error_tx,
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
        );
        assert!(!matches!(health_state.load(), WatcherHealth::Alive));
        let error = error_rx.try_recv().expect("panic error eventを期待");
        assert_eq!(error.kind(), WatchErrorKind::ThreadPanic);
        assert_eq!(error.detail(), "panic detail");
    }

    #[test]
    fn test_watcher_panic経路はanyhow_payload_detailを保持する() {
        let health_state = WatcherHealthState::new_starting();
        let (error_tx, mut error_rx) = priority_error_channel(super::WATCHER_ERROR_MESSAGE_BUFFER);

        handle_watcher_panic(
            Box::new(anyhow::anyhow!("anyhow panic detail")),
            "panic message",
            "panic label",
            &health_state,
            &error_tx,
        );

        let error = error_rx.try_recv().expect("panic error eventを期待");
        assert_eq!(error.kind(), WatchErrorKind::ThreadPanic);
        assert_eq!(error.detail(), "anyhow panic detail");
    }

    #[test]
    fn test_store_alive_if_startingは先行failedを上書きしない() {
        let health_state = WatcherHealthState::new_starting();

        health_state.store_failed(WatcherFailureKind::Notify);
        health_state.store_alive_if_starting();

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
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

    #[tokio::test]
    async fn test_failed_notifyはshutdown後もstoppedで上書きされない() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let (watcher, _rx) = watcher_for_test(shutdown_flag, watcher_thread, health_state.clone());

        health_state.store_failed(WatcherFailureKind::Notify);

        assert_eq!(
            shutdown_watcher_for_test(watcher).await,
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
    }

    #[tokio::test]
    async fn test_shutdown中のjoin_panicはhealthとerror_eventに記録する() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = std::thread::spawn(|| panic!("join panic detail"));
        let (watcher, mut rx) =
            watcher_for_test(shutdown_flag, watcher_thread, health_state.clone());

        assert_eq!(
            shutdown_watcher_for_test(watcher).await,
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
        );
        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
        );
        match tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("join panic error eventを待てる")
            .expect("join panic error eventを期待")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::ThreadPanic);
                assert_eq!(error.detail(), "join panic detail");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({:?})を受信", path)
            }
        }
    }

    #[tokio::test]
    async fn test_shutdown_task_panicはhealthとerror_eventに記録する() {
        let health_state = WatcherHealthState::new_alive();
        let (error_tx, mut error_rx) = priority_error_channel(super::WATCHER_ERROR_MESSAGE_BUFFER);
        let join_error = tokio::spawn(async {
            panic!("shutdown task panic detail");
        })
        .await
        .expect_err("shutdown task panicのJoinErrorを生成する");

        super::record_shutdown_task_panic(&health_state, &error_tx, join_error);

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::ShutdownTaskPanic)
        );
        let error = error_rx
            .try_recv()
            .expect("shutdown task panic error eventを期待");
        assert_eq!(error.kind(), WatchErrorKind::ShutdownTaskPanic);
        assert!(error.detail().contains("shutdown task panic"));
    }

    #[tokio::test]
    async fn test_watcher_shutdown後はstoppedを記録する() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let (watcher, _rx) = watcher_for_test(shutdown_flag, watcher_thread, health_state.clone());

        let shutdown_health = shutdown_watcher_for_test(watcher).await;

        assert_eq!(health_state.load(), WatcherHealth::Stopped);
        assert_eq!(shutdown_health, WatcherHealth::Stopped);
    }

    #[traced_test]
    #[tokio::test]
    async fn test_watch_runtime_stop_timeoutログに診断情報を含める() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let release_flag = Arc::new(AtomicBool::new(false));
        let thread_release_flag = release_flag.clone();
        let health_state = WatcherHealthState::new_alive();
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);
        let diagnostics = WatcherDiagnostics::new(merged_tx.clone(), health_state.clone());
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(4);
        let (error_tx, error_rx) = priority_error_channel(super::WATCHER_ERROR_MESSAGE_BUFFER);
        let merge_forwarder =
            super::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        drop(file_tx);
        let watcher_thread = std::thread::spawn(move || {
            while !thread_release_flag.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(1));
            }
        });
        let runtime = super::WatchRuntime {
            shutdown_flag,
            watcher_thread,
            merge_forwarder,
            health_state,
            diagnostics,
            error_tx,
        };

        let shutdown_health = runtime
            .stop_with_timeout(Duration::from_secs(0), Duration::from_millis(1))
            .await;
        release_flag.store(true, Ordering::Release);

        assert_eq!(
            shutdown_health,
            WatcherHealth::Failed(WatcherFailureKind::ShutdownTimedOut)
        );
        assert!(logs_contain("監視スレッドの停止がタイムアウトしました"));
        assert!(logs_contain(
            "監視イベント合流タスクの停止がタイムアウトしたためabortします"
        ));
        assert!(logs_contain("elapsed_ms="));
        assert!(logs_contain("timeout_secs=0"));
        match merged_rx
            .try_recv()
            .expect("shutdown timeout error eventを期待")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::ShutdownTimedOut);
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }
    }

    #[tokio::test]
    async fn test_watcher_shutdownで監視スレッドを停止できる() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let (watcher, _rx) = watcher_for_test(shutdown_flag.clone(), watcher_thread, health_state);
        assert_eq!(watcher.shutdown().await, WatcherHealth::Stopped);
        assert!(shutdown_flag.load(Ordering::Acquire));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_watcher_shutdown中もtokio_timerが進む() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let thread_shutdown_flag = shutdown_flag.clone();
        let watcher_thread = std::thread::spawn(move || {
            while !thread_shutdown_flag.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(1));
            }
            std::thread::sleep(Duration::from_millis(150));
        });
        let (watcher, _rx) = watcher_for_test(shutdown_flag, watcher_thread, health_state);

        let mut shutdown = Box::pin(watcher.shutdown());
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            health = &mut shutdown => {
                panic!("shutdownが早すぎてruntime非blocking性を検証できない: {health:?}");
            }
        }

        assert_eq!(shutdown.await, WatcherHealth::Stopped);
    }

    #[tokio::test]
    async fn test_watcher_dropはフォールバック停止を行う() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let (watcher, _rx) = watcher_for_test(shutdown_flag.clone(), watcher_thread, health_state);

        tokio::task::spawn_blocking(move || drop(watcher))
            .await
            .expect("watcher drop taskが正常終了する");
        assert!(shutdown_flag.load(Ordering::Acquire));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_watcher_dropはtokio_workerをブロックしない() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let release_flag = Arc::new(AtomicBool::new(false));
        let thread_release_flag = release_flag.clone();
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = std::thread::spawn(move || {
            while !thread_release_flag.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(10));
            }
        });
        let (watcher, _rx) = watcher_for_test(shutdown_flag.clone(), watcher_thread, health_state);

        let start = std::time::Instant::now();
        drop(watcher);
        let elapsed = start.elapsed();
        release_flag.store(true, Ordering::Release);

        assert!(
            elapsed < Duration::from_millis(200),
            "DropはTokio worker上で同期join timeoutを待たない: elapsed={elapsed:?}"
        );
        assert!(shutdown_flag.load(Ordering::Acquire));
    }

    #[traced_test]
    #[tokio::test(flavor = "current_thread")]
    async fn test_watcher_dropは未shutdown警告を残す() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let (watcher, _rx) = watcher_for_test(shutdown_flag, watcher_thread, health_state);

        drop(watcher);

        assert!(logs_contain(
            "Watcherがshutdown().awaitされずにdropされました"
        ));
    }
}
