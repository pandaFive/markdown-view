use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
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
/// notify callback から watcher thread へ橋渡しする内部チャネル容量
const WATCHER_INTERNAL_EVENT_BUFFER: usize = 64;
/// shutdown() のグレースフル停止待機秒数
pub(crate) const WATCH_SHUTDOWN_TIMEOUT_SECS: u64 = 2;
const DEFAULT_ERROR_DELIVERY_THREAD_NAME: &str = "markdown-view-watch-error-delivery";

type InitResult = std::result::Result<(), WatchError>;
type InternalWatchResult =
    std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>;

#[derive(Debug, Clone)]
struct ErrorDeliveryState {
    inner: Arc<Mutex<ErrorDeliveryInner>>,
    poison_recovered: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
enum ErrorDeliveryInner {
    #[default]
    Idle,
    InFlight {
        coalesced_count: u64,
    },
}

impl ErrorDeliveryState {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ErrorDeliveryInner::default())),
            poison_recovered: Arc::new(AtomicBool::new(false)),
        }
    }

    fn lock_inner(&self) -> MutexGuard<'_, ErrorDeliveryInner> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = ErrorDeliveryInner::Idle;
                self.inner.clear_poison();
                self.poison_recovered.store(true, Ordering::Release);
                guard
            }
        }
    }

    fn take_poison_recovered(&self) -> bool {
        self.poison_recovered.swap(false, Ordering::AcqRel)
    }

    fn try_mark_in_flight(&self) -> bool {
        let mut inner = self.lock_inner();
        match *inner {
            ErrorDeliveryInner::Idle => {
                *inner = ErrorDeliveryInner::InFlight { coalesced_count: 0 };
                true
            }
            ErrorDeliveryInner::InFlight { .. } => false,
        }
    }

    fn coalesce_if_in_flight(&self) -> Option<u64> {
        let mut inner = self.lock_inner();
        match &mut *inner {
            ErrorDeliveryInner::Idle => None,
            ErrorDeliveryInner::InFlight { coalesced_count } => {
                *coalesced_count += 1;
                Some(*coalesced_count)
            }
        }
    }

    fn complete_in_flight(&self) -> u64 {
        let mut inner = self.lock_inner();
        match std::mem::take(&mut *inner) {
            ErrorDeliveryInner::Idle => 0,
            ErrorDeliveryInner::InFlight { coalesced_count } => coalesced_count,
        }
    }

    #[cfg(test)]
    fn is_in_flight(&self) -> bool {
        matches!(*self.lock_inner(), ErrorDeliveryInner::InFlight { .. })
    }

    #[cfg(test)]
    fn coalesced_count(&self) -> u64 {
        match *self.lock_inner() {
            ErrorDeliveryInner::Idle => 0,
            ErrorDeliveryInner::InFlight { coalesced_count } => coalesced_count,
        }
    }
}

#[derive(Clone, Copy)]
struct WatchEventDelivery<'a> {
    strategy: &'a WatchStrategy,
    tx: &'a mpsc::Sender<WatchEvent>,
    health_state: &'a WatcherHealthState,
    error_delivery_state: &'a ErrorDeliveryState,
    error_delivery_thread_name: &'static str,
}

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
        };
        self.store_if_not_failed(raw);
    }

    fn store_thread_panic_unconditionally(&self) {
        self.state
            .store(Self::FAILED_THREAD_PANIC, Ordering::Release);
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
        matches!(raw, Self::FAILED_NOTIFY | Self::FAILED_THREAD_PANIC)
    }
}

/// 監視実行中ランタイム
///
/// `shutdown()` および `Drop` は同じ停止処理（タイムアウト付き待機）を行う。
/// `shutdown()` は `self` を消費するため二重停止を防止する。
pub struct Watcher {
    runtime: Option<WatchRuntime>,
}

struct WatchRuntime {
    shutdown_flag: Arc<AtomicBool>,
    watcher_thread: std::thread::JoinHandle<()>,
    health_state: WatcherHealthState,
    error_delivery_state: ErrorDeliveryState,
    error_tx: mpsc::Sender<WatchEvent>,
}

impl WatchRuntime {
    /// 監視スレッドに停止を通知し、完了を待機する
    ///
    /// `WATCH_SHUTDOWN_TIMEOUT_SECS` 以内にスレッドが終了しない場合はリークさせる
    /// （プロセス終了時にOSが回収する）。
    fn stop(self) -> WatcherHealth {
        self.stop_with_timeout(
            Duration::from_secs(WATCH_SHUTDOWN_TIMEOUT_SECS),
            Duration::from_millis(50),
        )
    }

    fn stop_with_timeout(self, timeout: Duration, poll_interval: Duration) -> WatcherHealth {
        let before_stop = self.health_state.load();
        if matches!(before_stop, WatcherHealth::Failed(_)) {
            tracing::warn!(
                "[markdown-view] 失敗状態の監視スレッドを停止します: {:?}",
                before_stop
            );
        }
        self.health_state.store_stopping_if_not_failed();
        self.shutdown_flag.store(true, Ordering::Release);
        self.watcher_thread.thread().unpark();

        let start = std::time::Instant::now();
        while !self.watcher_thread.is_finished() {
            if start.elapsed() > timeout {
                let elapsed_ms = start.elapsed().as_millis();
                tracing::warn!(
                    elapsed_ms,
                    timeout_secs = timeout.as_secs(),
                    "[markdown-view] 監視スレッドの停止がタイムアウトしました"
                );
                return self.health_state.load();
            }
            std::thread::sleep(poll_interval);
        }
        if let Err(panic_payload) = self.watcher_thread.join() {
            handle_watcher_panic(
                panic_payload,
                "監視スレッドの停止中にパニックを検出",
                "監視スレッド停止時パニック",
                &self.health_state,
                &self.error_delivery_state,
                &self.error_tx,
            );
            return self.health_state.load();
        }
        self.health_state.store_stopped_if_not_failed();
        self.health_state.load()
    }
}

impl Watcher {
    /// 監視を開始し、監視イベント受信用チャネルを返す
    pub async fn spawn(mode: AppMode) -> Result<(Self, mpsc::Receiver<WatchEvent>)> {
        let strategy = WatchStrategy::from_mode(&mode)?;
        let watch_plan = strategy.watch_plan()?;
        let (tx, rx) = mpsc::channel::<WatchEvent>(WATCHER_MESSAGE_BUFFER);
        let error_tx = tx.clone();
        let (init_tx, init_rx) = oneshot::channel::<InitResult>();
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let thread_shutdown_flag = shutdown_flag.clone();
        let health_state = WatcherHealthState::new_starting();
        let thread_health_state = health_state.clone();
        let error_delivery_state = ErrorDeliveryState::new();
        let thread_error_delivery_state = error_delivery_state.clone();
        let unexpected_exit = strategy.unexpected_exit_message();
        let watcher_thread = spawn_watcher_thread(
            strategy,
            watch_plan,
            tx,
            init_tx,
            thread_shutdown_flag,
            thread_health_state,
            thread_error_delivery_state,
        )?;

        await_watcher_init(init_rx, unexpected_exit).await?;
        health_state.store_alive_if_starting();
        Ok((
            Self::new(
                shutdown_flag,
                watcher_thread,
                health_state,
                error_delivery_state,
                error_tx,
            ),
            rx,
        ))
    }

    fn new(
        shutdown_flag: Arc<AtomicBool>,
        watcher_thread: std::thread::JoinHandle<()>,
        health_state: WatcherHealthState,
        error_delivery_state: ErrorDeliveryState,
        error_tx: mpsc::Sender<WatchEvent>,
    ) -> Self {
        Self {
            runtime: Some(WatchRuntime {
                shutdown_flag,
                watcher_thread,
                health_state,
                error_delivery_state,
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

    /// 監視スレッドを停止する
    pub fn shutdown(mut self) -> WatcherHealth {
        if let Some(runtime) = self.runtime.take() {
            runtime.stop()
        } else {
            WatcherHealth::Stopped
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.stop();
        }
    }
}

/// notifyコールバックからtokioチャネルへイベントを転送する。
/// FileChangedは過負荷時に破棄し、Errorは満杯時にbounded helperへ委譲する。
fn send_watch_event(
    tx: &mpsc::Sender<WatchEvent>,
    event: WatchEvent,
    label: &str,
    error_delivery_state: &ErrorDeliveryState,
) {
    send_watch_event_with_context(
        tx,
        event,
        label,
        error_delivery_state,
        DEFAULT_ERROR_DELIVERY_THREAD_NAME,
        None,
    );
}

fn send_watch_event_with_context(
    tx: &mpsc::Sender<WatchEvent>,
    event: WatchEvent,
    label: &str,
    error_delivery_state: &ErrorDeliveryState,
    error_delivery_thread_name: &'static str,
    health_state: Option<&WatcherHealthState>,
) {
    match event {
        WatchEvent::FileChanged(path) => send_file_changed_event(tx, path, label),
        WatchEvent::Error(error) => send_error_event(
            tx,
            error,
            label,
            error_delivery_state,
            error_delivery_thread_name,
            health_state,
        ),
    }
}

fn send_file_changed_event(tx: &mpsc::Sender<WatchEvent>, path: PathBuf, label: &str) {
    match tx.try_send(WatchEvent::FileChanged(path)) {
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
        }
    }
}

fn send_error_event(
    tx: &mpsc::Sender<WatchEvent>,
    error: WatchError,
    label: &str,
    error_delivery_state: &ErrorDeliveryState,
    error_delivery_thread_name: &'static str,
    health_state: Option<&WatcherHealthState>,
) {
    // Production の通常送信元は watcher thread だが、shutdown panic 経路など
    // Tokio runtime 内から呼ばれる場合もあるため fallback は runtime-aware にする。
    latch_health_for_watch_error(health_state, error.kind());
    if let Some(coalesced) = error_delivery_state.coalesce_if_in_flight() {
        latch_error_delivery_poison_if_recovered(health_state, error_delivery_state);
        tracing::warn!(
            coalesced,
            error_kind = ?error.kind(),
            "[markdown-view] 監視エラー送達が保留中のため追加エラーを集約しました: {}",
            label
        );
        return;
    }
    latch_error_delivery_poison_if_recovered(health_state, error_delivery_state);

    match tx.try_send(WatchEvent::Error(error)) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Closed(WatchEvent::Error(error))) => {
            tracing::warn!(
                error_kind = ?error.kind(),
                "[markdown-view] 通知チャネルが閉じているため監視エラーを送達できませんでした: {}",
                label,
            );
        }
        Err(mpsc::error::TrySendError::Full(WatchEvent::Error(error))) => {
            enqueue_blocking_error_delivery(
                tx.clone(),
                error,
                label,
                error_delivery_state,
                error_delivery_thread_name,
                health_state.cloned(),
            );
        }
        Err(mpsc::error::TrySendError::Closed(WatchEvent::FileChanged(_)))
        | Err(mpsc::error::TrySendError::Full(WatchEvent::FileChanged(_))) => {
            if let Some(health_state) = health_state {
                health_state.store_thread_panic_unconditionally();
            }
            debug_assert!(false, "WatchEvent::Error送信でFileChangedが返ることはない");
            tracing::error!(
                "[markdown-view] 監視エラー送達で予期しないイベント種別を受け取りました: {}",
                label
            );
        }
    }
}

fn enqueue_blocking_error_delivery(
    tx: mpsc::Sender<WatchEvent>,
    error: WatchError,
    label: &str,
    error_delivery_state: &ErrorDeliveryState,
    thread_name: &'static str,
    health_state: Option<WatcherHealthState>,
) {
    enqueue_blocking_error_delivery_with_spawner(
        tx,
        error,
        label,
        error_delivery_state,
        thread_name,
        health_state,
        |name, job| {
            std::thread::Builder::new()
                .name(name.to_string())
                .spawn(job)
                .map(|_| ())
        },
    );
}

fn enqueue_blocking_error_delivery_with_spawner<F>(
    tx: mpsc::Sender<WatchEvent>,
    error: WatchError,
    label: &str,
    error_delivery_state: &ErrorDeliveryState,
    thread_name: &'static str,
    health_state: Option<WatcherHealthState>,
    spawn_delivery: F,
) where
    F: FnOnce(&'static str, Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<()>,
{
    if !error_delivery_state.try_mark_in_flight() {
        latch_error_delivery_poison_if_recovered(health_state.as_ref(), error_delivery_state);
        let coalesced = error_delivery_state.coalesce_if_in_flight().unwrap_or(0);
        latch_error_delivery_poison_if_recovered(health_state.as_ref(), error_delivery_state);
        tracing::warn!(
            coalesced,
            error_kind = ?error.kind(),
            "[markdown-view] 監視エラー送達が保留中のため追加エラーを集約しました: {}",
            label
        );
        return;
    }

    let label = label.to_string();
    let delivery_label = label.clone();
    let fallback_label = label.clone();
    let error_kind = error.kind();
    let state = error_delivery_state.clone();
    let delivery_health_state = health_state.clone();
    let fallback_tx = tx.clone();
    let fallback_error = error.clone();
    let fallback_state = state.clone();
    let fallback_health_state = health_state.clone();
    match spawn_delivery(
        thread_name,
        Box::new(move || {
            deliver_blocking_error_event(
                tx,
                error,
                &delivery_label,
                error_kind,
                &state,
                delivery_health_state,
                thread_name,
            );
        }),
    ) {
        Ok(_) => {}
        Err(spawn_error) => {
            tracing::warn!(
                error_kind = ?error_kind,
                "[markdown-view] 監視エラー送達スレッドの起動に失敗しました。fallback送達を試みます: {}, {}",
                label,
                spawn_error
            );
            deliver_fallback_error_event(
                fallback_tx,
                fallback_error,
                fallback_label,
                error_kind,
                fallback_state,
                fallback_health_state,
                thread_name,
            );
        }
    }
}

fn deliver_fallback_error_event(
    tx: mpsc::Sender<WatchEvent>,
    error: WatchError,
    label: String,
    error_kind: super::WatchErrorKind,
    error_delivery_state: ErrorDeliveryState,
    health_state: Option<WatcherHealthState>,
    thread_name: &'static str,
) {
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::spawn(async move {
            deliver_async_error_event(
                tx,
                error,
                label,
                error_kind,
                error_delivery_state,
                health_state,
                thread_name,
            )
            .await;
        });
    } else {
        deliver_blocking_error_event(
            tx,
            error,
            &label,
            error_kind,
            &error_delivery_state,
            health_state,
            thread_name,
        );
    }
}

async fn deliver_async_error_event(
    tx: mpsc::Sender<WatchEvent>,
    error: WatchError,
    label: String,
    error_kind: super::WatchErrorKind,
    error_delivery_state: ErrorDeliveryState,
    health_state: Option<WatcherHealthState>,
    thread_name: &'static str,
) {
    if tx.send(WatchEvent::Error(error)).await.is_err() {
        tracing::warn!(
            error_kind = ?error_kind,
            "[markdown-view] 通知チャネルが閉じているため監視エラーを送達できませんでした: {}",
            label
        );
    }
    complete_error_delivery(
        &label,
        &tx,
        &error_delivery_state,
        health_state.as_ref(),
        thread_name,
    );
}

/// Tokio runtime 内で呼ぶと `blocking_send` が panic するため、runtime 外の
/// helper thread または同期 fallback 経路からだけ呼び出す。
fn deliver_blocking_error_event(
    tx: mpsc::Sender<WatchEvent>,
    error: WatchError,
    label: &str,
    error_kind: super::WatchErrorKind,
    error_delivery_state: &ErrorDeliveryState,
    health_state: Option<WatcherHealthState>,
    thread_name: &'static str,
) {
    debug_assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "deliver_blocking_error_event must not run inside a Tokio runtime"
    );
    if tx.blocking_send(WatchEvent::Error(error)).is_err() {
        tracing::warn!(
            error_kind = ?error_kind,
            "[markdown-view] 通知チャネルが閉じているため監視エラーを送達できませんでした: {}",
            label
        );
    }
    complete_error_delivery(
        label,
        &tx,
        error_delivery_state,
        health_state.as_ref(),
        thread_name,
    );
}

fn complete_error_delivery(
    label: &str,
    tx: &mpsc::Sender<WatchEvent>,
    error_delivery_state: &ErrorDeliveryState,
    health_state: Option<&WatcherHealthState>,
    thread_name: &'static str,
) {
    let coalesced = error_delivery_state.complete_in_flight();
    latch_error_delivery_poison_if_recovered(health_state, error_delivery_state);
    if coalesced > 0 {
        tracing::warn!(
            coalesced,
            "[markdown-view] 送達待機中に発生した監視エラーを集約しました: {}",
            label
        );
        send_watch_event_with_context(
            tx,
            WatchEvent::Error(WatchError::notify(format!(
                "送達待機中に発生した監視エラーを{}件集約しました",
                coalesced
            ))),
            label,
            error_delivery_state,
            thread_name,
            health_state,
        );
    }
}

fn latch_health_for_watch_error(
    health_state: Option<&WatcherHealthState>,
    error_kind: super::WatchErrorKind,
) {
    let Some(health_state) = health_state else {
        return;
    };
    match error_kind {
        super::WatchErrorKind::ThreadPanic => {
            health_state.store_failed(WatcherFailureKind::ThreadPanic);
        }
        super::WatchErrorKind::Init
        | super::WatchErrorKind::Notify
        | super::WatchErrorKind::ResourceExhausted => {
            health_state.store_failed(WatcherFailureKind::Notify);
        }
    }
}

fn latch_error_delivery_poison_if_recovered(
    health_state: Option<&WatcherHealthState>,
    error_delivery_state: &ErrorDeliveryState,
) {
    if error_delivery_state.take_poison_recovered() {
        tracing::error!(
            "[markdown-view] 監視エラー送達状態のpoisonを検出したため状態をリセットしました"
        );
        if let Some(health_state) = health_state {
            health_state.store_thread_panic_unconditionally();
        }
    }
}

fn spawn_watcher_thread(
    strategy: WatchStrategy,
    watch_plan: super::strategy::WatchPlan,
    tx: mpsc::Sender<WatchEvent>,
    init_tx: oneshot::Sender<InitResult>,
    thread_shutdown_flag: Arc<AtomicBool>,
    health_state: WatcherHealthState,
    error_delivery_state: ErrorDeliveryState,
) -> Result<std::thread::JoinHandle<()>> {
    let thread_name = strategy.thread_name().to_string();
    let spawn_context = format!("監視スレッド {} の起動に失敗", strategy.thread_name());
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let rt_tx = tx;
            let panic_tx = rt_tx.clone();
            let panic_error_delivery_state = error_delivery_state.clone();
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
                let callback_tx = rt_tx.clone();
                let callback_health_state = health_state.clone();
                let callback_error_delivery_state = error_delivery_state.clone();
                let debouncer = new_debouncer(
                    Duration::from_millis(DEBOUNCE_MS),
                    move |res: InternalWatchResult| {
                        send_internal_watch_result(
                            &internal_tx,
                            res,
                            &callback_strategy,
                            &callback_tx,
                            &callback_health_state,
                            &callback_error_delivery_state,
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
                let delivery = WatchEventDelivery {
                    strategy: &strategy,
                    tx: &rt_tx,
                    health_state: &health_state,
                    error_delivery_state: &error_delivery_state,
                    error_delivery_thread_name: strategy.error_delivery_thread_name(),
                };
                run_watcher_event_loop(
                    &mut debouncer,
                    internal_rx,
                    delivery,
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
                    &panic_error_delivery_state,
                    &panic_tx,
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
    tx: &mpsc::Sender<WatchEvent>,
    health_state: &WatcherHealthState,
    error_delivery_state: &ErrorDeliveryState,
) {
    match internal_tx.try_send(result) {
        Ok(()) => {}
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            tracing::warn!("[markdown-view] watcher internal channel が満杯です");
            health_state.store_failed(WatcherFailureKind::Notify);
            send_watch_event_with_context(
                tx,
                WatchEvent::Error(WatchError::notify("watcher internal channel が満杯です")),
                strategy.error_label(),
                error_delivery_state,
                strategy.error_delivery_thread_name(),
                Some(health_state),
            );
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            tracing::warn!("[markdown-view] watcher internal channel が閉じています");
            health_state.store_failed(WatcherFailureKind::Notify);
            send_watch_event_with_context(
                tx,
                WatchEvent::Error(WatchError::notify(
                    "watcher internal channel が閉じています",
                )),
                strategy.error_label(),
                error_delivery_state,
                strategy.error_delivery_thread_name(),
                Some(health_state),
            );
        }
    }
}

fn handle_debounced_watch_result(
    result: std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>,
    strategy: &WatchStrategy,
    tx: &mpsc::Sender<WatchEvent>,
    health_state: &WatcherHealthState,
    error_delivery_state: &ErrorDeliveryState,
) {
    match result {
        Ok(events) => {
            for changed_path in strategy.collect_changed_paths(&events) {
                send_watch_event(
                    tx,
                    WatchEvent::FileChanged(changed_path),
                    strategy.change_label(),
                    error_delivery_state,
                );
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
            send_watch_event_with_context(
                tx,
                WatchEvent::Error(watch_error),
                strategy.error_label(),
                error_delivery_state,
                strategy.error_delivery_thread_name(),
                Some(health_state),
            );
        }
    }
}

#[cfg(test)]
fn process_debounced_events_with_watch<F>(
    events: Vec<notify_debouncer_mini::DebouncedEvent>,
    strategy: &WatchStrategy,
    tx: &mpsc::Sender<WatchEvent>,
    health_state: &WatcherHealthState,
    error_delivery_state: &ErrorDeliveryState,
    registered_paths: &mut WatchDirectoryRegistry,
    watch: F,
) where
    F: FnMut(&Path, notify::RecursiveMode) -> notify::Result<()>,
{
    let delivery = WatchEventDelivery {
        strategy,
        tx,
        health_state,
        error_delivery_state,
        error_delivery_thread_name: strategy.error_delivery_thread_name(),
    };
    process_debounced_events_with_watch_and_unwatch(
        events,
        delivery,
        registered_paths,
        watch,
        |_path| Ok(()),
    );
}

fn process_debounced_events_with_watch_and_unwatch<F, U>(
    events: Vec<notify_debouncer_mini::DebouncedEvent>,
    delivery: WatchEventDelivery<'_>,
    registered_paths: &mut WatchDirectoryRegistry,
    mut watch: F,
    mut unwatch: U,
) where
    F: FnMut(&Path, notify::RecursiveMode) -> notify::Result<()>,
    U: FnMut(&Path) -> notify::Result<()>,
{
    for changed_path in delivery.strategy.collect_changed_paths(&events) {
        send_watch_event(
            delivery.tx,
            WatchEvent::FileChanged(changed_path),
            delivery.strategy.change_label(),
            delivery.error_delivery_state,
        );
    }

    for candidate in delivery.strategy.collect_new_directory_candidates(&events) {
        if registered_paths.contains(&candidate) {
            for stale_path in registered_paths.remove_subtree(&candidate) {
                if let Err(error) = unwatch(&stale_path) {
                    tracing::warn!(
                        "[markdown-view] 登録済みディレクトリの監視解除に失敗（再登録は継続）: path={}, {}",
                        delivery.strategy.path_for_log(&stale_path),
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
                    delivery
                        .health_state
                        .store_failed(WatcherFailureKind::Notify);
                    let watch_error = WatchError::from_watch_registration_error(
                        "新規ディレクトリの監視追加に失敗",
                        &failure.source,
                    );
                    tracing::warn!(
                        "[markdown-view] 新規ディレクトリの監視追加に失敗: candidate={}, {}",
                        delivery.strategy.path_for_log(&candidate),
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
                send_watch_event(
                    delivery.tx,
                    WatchEvent::FileChanged(markdown),
                    delivery.strategy.change_label(),
                    delivery.error_delivery_state,
                );
            }
        }

        if let Some(watch_error) = registration_error {
            send_watch_event_with_context(
                delivery.tx,
                WatchEvent::Error(watch_error),
                delivery.strategy.error_label(),
                delivery.error_delivery_state,
                delivery.error_delivery_thread_name,
                Some(delivery.health_state),
            );
        }
    }
}

fn handle_watcher_panic(
    panic_payload: Box<dyn std::any::Any + Send>,
    panic_message: &str,
    error_label: &str,
    health_state: &WatcherHealthState,
    error_delivery_state: &ErrorDeliveryState,
    tx: &mpsc::Sender<WatchEvent>,
) {
    let panic_detail = if let Some(s) = panic_payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "不明なパニック".to_string()
    };
    health_state.store_failed(WatcherFailureKind::ThreadPanic);
    let watch_error = WatchError::thread_panic(panic_detail.clone());
    tracing::error!("[markdown-view] {}: {}", panic_message, panic_detail);
    send_watch_event_with_context(
        tx,
        WatchEvent::Error(watch_error),
        error_label,
        error_delivery_state,
        DEFAULT_ERROR_DELIVERY_THREAD_NAME,
        Some(health_state),
    );
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
    delivery: WatchEventDelivery<'_>,
    shutdown_flag: &AtomicBool,
    registered_paths: &mut WatchDirectoryRegistry,
) {
    while !shutdown_flag.load(Ordering::Acquire) {
        match internal_rx.recv_timeout(Duration::from_millis(WATCHER_THREAD_PARK_MS)) {
            Ok(Ok(events)) => {
                let watcher = std::cell::RefCell::new(debouncer.watcher());
                process_debounced_events_with_watch_and_unwatch(
                    events,
                    delivery,
                    registered_paths,
                    |path, mode| watcher.borrow_mut().watch(path, mode),
                    |path| watcher.borrow_mut().unwatch(path),
                );
            }
            Ok(Err(error)) => {
                handle_debounced_watch_result(
                    Err(error),
                    delivery.strategy,
                    delivery.tx,
                    delivery.health_state,
                    delivery.error_delivery_state,
                );
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                handle_internal_channel_disconnected(
                    delivery.strategy,
                    delivery.tx,
                    delivery.health_state,
                    delivery.error_delivery_state,
                    shutdown_flag,
                );
                break;
            }
        }
    }
}

fn handle_internal_channel_disconnected(
    strategy: &WatchStrategy,
    tx: &mpsc::Sender<WatchEvent>,
    health_state: &WatcherHealthState,
    error_delivery_state: &ErrorDeliveryState,
    shutdown_flag: &AtomicBool,
) {
    tracing::warn!("[markdown-view] watcher internal channel が切断されました");
    if shutdown_flag.load(Ordering::Acquire) {
        return;
    }
    health_state.store_failed(WatcherFailureKind::Notify);
    send_watch_event_with_context(
        tx,
        WatchEvent::Error(WatchError::notify(
            "watcher internal channel が切断されました",
        )),
        strategy.error_label(),
        error_delivery_state,
        strategy.error_delivery_thread_name(),
        Some(health_state),
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::mpsc;
    use tracing_test::traced_test;

    use super::{
        handle_debounced_watch_result, handle_watcher_panic, process_debounced_events_with_watch,
        process_debounced_events_with_watch_and_unwatch, register_watch_plan_with,
        send_watch_event, send_watch_event_with_context, ErrorDeliveryState,
        WatchDirectoryRegistry, WatchEventDelivery, Watcher, WatcherFailureKind, WatcherHealth,
        WatcherHealthState, DEFAULT_ERROR_DELIVERY_THREAD_NAME,
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

    fn recv_event_until(
        rx: &mut mpsc::Receiver<WatchEvent>,
        timeout: Duration,
    ) -> Option<WatchEvent> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Ok(event) = rx.try_recv() {
                return Some(event);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());
        let mut watched = Vec::new();

        process_debounced_events_with_watch(
            events,
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
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
            rx.try_recv().expect("recovery markdown notificationを期待"),
            WatchEvent::FileChanged(md.canonicalize().unwrap())
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());

        process_debounced_events_with_watch(
            events,
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
            &mut registered,
            |_path, _mode| Err(notify::Error::generic("dynamic watch failed")),
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        match rx.try_recv().expect("dynamic watch error eventを期待") {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert!(error.detail().contains("dynamic watch failed"));
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({:?})を受信", path)
            }
        }
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());

        process_debounced_events_with_watch(
            events,
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
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
            rx.try_recv().expect("root recovery notificationを期待"),
            WatchEvent::FileChanged(root_md.canonicalize().unwrap())
        );
        match rx.try_recv().expect("partial watch error eventを期待") {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert!(error.detail().contains("nested watch failed"));
            }
            WatchEvent::FileChanged(path) => {
                panic!("nested配下のrecovery通知は期待しない: {:?}", path)
            }
        }
        assert!(
            rx.try_recv().is_err(),
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());

        process_debounced_events_with_watch(
            events,
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
            &mut registered,
            |_path, _mode| Err(notify::Error::new(notify::ErrorKind::MaxFilesWatch)),
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        match rx
            .try_recv()
            .expect("dynamic watch resource exhausted eventを期待")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::ResourceExhausted);
                assert!(error.detail().contains("新規ディレクトリの監視追加に失敗"));
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({:?})を受信", path)
            }
        }
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());
        registered.insert(existing_dir.canonicalize().unwrap());
        let mut watched = Vec::new();

        process_debounced_events_with_watch(
            events,
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
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
            rx.try_recv().expect("refresh recovery notificationを期待"),
            WatchEvent::FileChanged(md.canonicalize().unwrap())
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());
        registered.insert(existing_dir.canonicalize().unwrap());
        let mut watched = Vec::new();
        let mut unwatched = Vec::new();
        let delivery = WatchEventDelivery {
            strategy: &strategy,
            tx: &tx,
            health_state: &health_state,
            error_delivery_state: &error_delivery_state,
            error_delivery_thread_name: strategy.error_delivery_thread_name(),
        };

        process_debounced_events_with_watch_and_unwatch(
            events,
            delivery,
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
            rx.try_recv().expect("refresh recovery notificationを期待"),
            WatchEvent::FileChanged(md.canonicalize().unwrap())
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let mut watched = Vec::new();

        process_debounced_events_with_watch(
            events,
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
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
            rx.try_recv().is_err(),
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let mut watched = Vec::new();

        process_debounced_events_with_watch(
            events,
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
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
            rx.try_recv()
                .expect("recreated recovery notificationを期待"),
            WatchEvent::FileChanged(md.canonicalize().unwrap())
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());
        registered.insert(existing_dir.canonicalize().unwrap());

        process_debounced_events_with_watch(
            events,
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
            &mut registered,
            |_path, _mode| Ok(()),
        );

        assert_eq!(
            rx.try_recv().expect("new root recovery notificationを期待"),
            WatchEvent::FileChanged(new_md.canonicalize().unwrap())
        );
        assert!(
            rx.try_recv().is_err(),
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let mut registered = WatchDirectoryRegistry::default();
        registered.insert(dir.path().canonicalize().unwrap());

        process_debounced_events_with_watch(
            events,
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
            &mut registered,
            |_path, _mode| Ok(()),
        );

        assert_eq!(
            rx.try_recv().expect("nested recovery notificationを期待"),
            WatchEvent::FileChanged(md.canonicalize().unwrap())
        );
        assert!(rx.try_recv().is_err(), "重複recovery通知は不要");
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);

        assert!(matches!(
            internal_rx.recv_timeout(Duration::from_millis(1)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ));
        super::handle_internal_channel_disconnected(
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
            &shutdown_flag,
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        match rx.try_recv().expect("internal channel error eventを期待") {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert!(error.detail().contains("internal channel"));
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({:?})を受信", path)
            }
        }
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);

        super::send_internal_watch_result(
            &internal_tx,
            Ok(Vec::new()),
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        match rx
            .try_recv()
            .expect("internal channel full error eventを期待")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert!(error.detail().contains("internal channel が満杯"));
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({:?})を受信", path)
            }
        }
    }

    #[test]
    fn test_send_watch_event_filechangedはチャネル満杯時に破棄してブロックしない() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        let (_dir2, second) = create_markdown_fixture("second.md", "# second");
        tx.blocking_send(WatchEvent::FileChanged(first.clone()))
            .expect("channelを満杯にできる");

        send_watch_event(
            &tx,
            WatchEvent::FileChanged(second),
            "filechanged満杯時テスト",
            &error_delivery_state,
        );

        match rx.blocking_recv().expect("最初のイベントを受信できる") {
            WatchEvent::FileChanged(path) => assert_eq!(path, first),
            WatchEvent::Error(error) => {
                panic!("最初のメッセージはFileChangedを期待したがError({error})を受信")
            }
        }

        assert!(
            rx.try_recv().is_err(),
            "満杯時のFileChangedは破棄されているはず"
        );
    }

    #[test]
    fn test_send_watch_event_errorはチャネル満杯時もreceiverが開いていれば送達する() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        let health_state = WatcherHealthState::new_alive();
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        tx.blocking_send(WatchEvent::FileChanged(first.clone()))
            .expect("channelを満杯にできる");

        send_watch_event_with_context(
            &tx,
            WatchEvent::Error(WatchError::notify("満杯時も送達する")),
            "error満杯時テスト",
            &error_delivery_state,
            DEFAULT_ERROR_DELIVERY_THREAD_NAME,
            Some(&health_state),
        );
        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify),
            "Error送信時点でhealth failureを先行latchする"
        );
        drop(tx);

        match rx.blocking_recv().expect("先行FileChangedを受信できる") {
            WatchEvent::FileChanged(path) => assert_eq!(path, first),
            WatchEvent::Error(error) => {
                panic!("先行メッセージはFileChangedを期待したがError({error})を受信")
            }
        }

        let received_error = recv_event_until(&mut rx, Duration::from_secs(1));

        match received_error.expect("Error eventを受信できる") {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert_eq!(error.detail(), "満杯時も送達する");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }
    }

    #[test]
    fn test_send_watch_event_errorは送達中の追加エラーを集約する() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        tx.blocking_send(WatchEvent::FileChanged(first.clone()))
            .expect("channelを満杯にできる");

        send_watch_event(
            &tx,
            WatchEvent::Error(WatchError::notify("代表エラー")),
            "error集約テスト",
            &error_delivery_state,
        );
        send_watch_event(
            &tx,
            WatchEvent::Error(WatchError::notify("追加エラー1")),
            "error集約テスト",
            &error_delivery_state,
        );
        send_watch_event(
            &tx,
            WatchEvent::Error(WatchError::notify("追加エラー2")),
            "error集約テスト",
            &error_delivery_state,
        );

        assert_eq!(
            error_delivery_state.coalesced_count(),
            2,
            "送達中の追加Errorは別queueへ積まず件数集約する"
        );

        match rx.blocking_recv().expect("先行FileChangedを受信できる") {
            WatchEvent::FileChanged(path) => assert_eq!(path, first),
            WatchEvent::Error(error) => {
                panic!("先行メッセージはFileChangedを期待したがError({error})を受信")
            }
        }

        match recv_event_until(&mut rx, Duration::from_secs(1)).expect("代表Errorを受信できる")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert_eq!(error.detail(), "代表エラー");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }

        match recv_event_until(&mut rx, Duration::from_secs(1))
            .expect("集約サマリErrorを受信できる")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert_eq!(
                    error.detail(),
                    "送達待機中に発生した監視エラーを2件集約しました"
                );
            }
            WatchEvent::FileChanged(path) => {
                panic!("集約サマリErrorを期待したがFileChanged({path:?})を受信")
            }
        }

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while error_delivery_state.is_in_flight() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            !error_delivery_state.is_in_flight(),
            "代表Error送達後はin-flightを解除する"
        );
        assert_eq!(
            error_delivery_state.coalesced_count(),
            0,
            "集約件数は送達完了時に消費する"
        );
    }

    #[test]
    fn test_send_watch_event_errorは送達中ならチャネルに空きがあっても集約する() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(2);
        let error_delivery_state = ErrorDeliveryState::new();

        assert!(
            error_delivery_state.try_mark_in_flight(),
            "代表Error送達中の状態を作る"
        );

        send_watch_event(
            &tx,
            WatchEvent::Error(WatchError::notify("追加エラー")),
            "error空きあり集約テスト",
            &error_delivery_state,
        );

        assert_eq!(
            error_delivery_state.coalesced_count(),
            1,
            "送達中の追加Errorはチャネルに空きがあっても件数集約する"
        );
        assert!(
            rx.try_recv().is_err(),
            "送達中の追加Errorはchannelへ積まない"
        );
        assert_eq!(
            error_delivery_state.complete_in_flight(),
            1,
            "集約件数を消費できる"
        );
    }

    #[test]
    fn test_send_watch_event_errorはhelper_thread起動失敗時もfallback送達する() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        tx.blocking_send(WatchEvent::FileChanged(first.clone()))
            .expect("channelを満杯にできる");

        let delivery_state = error_delivery_state.clone();
        let delivery_tx = tx.clone();
        let delivery_thread = std::thread::spawn(move || {
            super::enqueue_blocking_error_delivery_with_spawner(
                delivery_tx,
                WatchError::notify("helper spawn failure fallback"),
                "helper起動失敗fallbackテスト",
                &delivery_state,
                DEFAULT_ERROR_DELIVERY_THREAD_NAME,
                None,
                |_thread_name, _job| {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        "thread spawn failed",
                    ))
                },
            );
        });
        drop(tx);

        match rx.blocking_recv().expect("先行FileChangedを受信できる") {
            WatchEvent::FileChanged(path) => assert_eq!(path, first),
            WatchEvent::Error(error) => {
                panic!("先行メッセージはFileChangedを期待したがError({error})を受信")
            }
        }
        match recv_event_until(&mut rx, Duration::from_secs(1)).expect("fallback Errorを受信できる")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert_eq!(error.detail(), "helper spawn failure fallback");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }
        delivery_thread
            .join()
            .expect("fallback delivery thread should finish");
        assert!(
            !error_delivery_state.is_in_flight(),
            "fallback送達後はin-flightを解除する"
        );
        assert_eq!(
            error_delivery_state.coalesced_count(),
            0,
            "fallback送達後は集約件数を消費する"
        );
    }

    #[test]
    fn test_send_watch_event_errorはhelper_thread名をspawnerへ渡す() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        const TEST_THREAD_NAME: &str = "markdown-view-watch-error-delivery-test";

        super::enqueue_blocking_error_delivery_with_spawner(
            tx,
            WatchError::notify("thread name propagation"),
            "helper thread名テスト",
            &error_delivery_state,
            TEST_THREAD_NAME,
            None,
            |thread_name, _job| {
                assert_eq!(thread_name, TEST_THREAD_NAME);
                Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "thread spawn failed",
                ))
            },
        );

        match rx.blocking_recv().expect("fallback Errorを受信できる") {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert_eq!(error.detail(), "thread name propagation");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_send_watch_event_errorはruntime内helper_thread起動失敗時もpanicせずfallback送達する(
    ) {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        tx.send(WatchEvent::FileChanged(first.clone()))
            .await
            .expect("channelを満杯にできる");

        super::enqueue_blocking_error_delivery_with_spawner(
            tx.clone(),
            WatchError::notify("runtime helper spawn failure fallback"),
            "runtime helper起動失敗fallbackテスト",
            &error_delivery_state,
            DEFAULT_ERROR_DELIVERY_THREAD_NAME,
            None,
            |_thread_name, _job| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "thread spawn failed",
                ))
            },
        );
        drop(tx);

        match rx.recv().await.expect("先行FileChangedを受信できる") {
            WatchEvent::FileChanged(path) => assert_eq!(path, first),
            WatchEvent::Error(error) => {
                panic!("先行メッセージはFileChangedを期待したがError({error})を受信")
            }
        }
        match tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("fallback Errorの受信がtimeoutしない")
            .expect("fallback Errorを受信できる")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert_eq!(error.detail(), "runtime helper spawn failure fallback");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }

        tokio::time::timeout(Duration::from_secs(1), async {
            while error_delivery_state.is_in_flight() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fallback送達後にin-flightが解除される");
        assert_eq!(
            error_delivery_state.coalesced_count(),
            0,
            "fallback送達後は集約件数を消費する"
        );
    }

    #[test]
    fn test_send_watch_event_errorは別watcher_stateの送達を詰まらせない() {
        let (blocked_tx, mut blocked_rx) = mpsc::channel::<WatchEvent>(1);
        let blocked_state = ErrorDeliveryState::new();
        let (_blocked_dir, blocked_first) = create_markdown_fixture("blocked.md", "# blocked");
        blocked_tx
            .blocking_send(WatchEvent::FileChanged(blocked_first.clone()))
            .expect("blocked channelを満杯にできる");

        let (independent_tx, mut independent_rx) = mpsc::channel::<WatchEvent>(1);
        let independent_state = ErrorDeliveryState::new();
        let (_independent_dir, independent_first) =
            create_markdown_fixture("independent.md", "# independent");
        independent_tx
            .blocking_send(WatchEvent::FileChanged(independent_first.clone()))
            .expect("independent channelを満杯にできる");

        send_watch_event(
            &blocked_tx,
            WatchEvent::Error(WatchError::notify("blocked error")),
            "blocked watcher",
            &blocked_state,
        );
        send_watch_event(
            &independent_tx,
            WatchEvent::Error(WatchError::notify("independent error")),
            "independent watcher",
            &independent_state,
        );

        assert!(
            blocked_state.is_in_flight(),
            "blocked watcherの送達待機は独立したstateに閉じる"
        );

        match independent_rx
            .blocking_recv()
            .expect("independent先行FileChangedを受信できる")
        {
            WatchEvent::FileChanged(path) => assert_eq!(path, independent_first),
            WatchEvent::Error(error) => {
                panic!("先行メッセージはFileChangedを期待したがError({error})を受信")
            }
        }
        match recv_event_until(&mut independent_rx, Duration::from_secs(1))
            .expect("independent Errorを受信できる")
        {
            WatchEvent::Error(error) => assert_eq!(error.detail(), "independent error"),
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }

        match blocked_rx
            .blocking_recv()
            .expect("blocked先行FileChangedを受信できる")
        {
            WatchEvent::FileChanged(path) => assert_eq!(path, blocked_first),
            WatchEvent::Error(error) => {
                panic!("先行メッセージはFileChangedを期待したがError({error})を受信")
            }
        }
        match recv_event_until(&mut blocked_rx, Duration::from_secs(1))
            .expect("blocked Errorを受信できる")
        {
            WatchEvent::Error(error) => assert_eq!(error.detail(), "blocked error"),
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_send_watch_event_errorはcurrent_thread_runtimeでも満杯時にruntimeを止めない() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        tx.send(WatchEvent::FileChanged(first.clone()))
            .await
            .expect("channelを満杯にできる");

        send_watch_event(
            &tx,
            WatchEvent::Error(WatchError::notify("current_threadでも送達する")),
            "current_thread満杯時テスト",
            &error_delivery_state,
        );
        drop(tx);

        tokio::task::yield_now().await;

        match rx.recv().await.expect("先行FileChangedを受信できる") {
            WatchEvent::FileChanged(path) => assert_eq!(path, first),
            WatchEvent::Error(error) => {
                panic!("先行メッセージはFileChangedを期待したがError({error})を受信")
            }
        }

        match tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("Error eventの受信がtimeoutしない")
            .expect("Error eventを受信できる")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert_eq!(error.detail(), "current_threadでも送達する");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }
    }

    #[test]
    fn test_send_watch_event_errorはreceiver_closedでもpanicしない() {
        let (tx, rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        drop(rx);

        send_watch_event(
            &tx,
            WatchEvent::Error(WatchError::notify("receiver closed")),
            "error receiver closedテスト",
            &error_delivery_state,
        );

        assert!(
            !error_delivery_state.is_in_flight(),
            "receiver closedではin-flightを残さない"
        );
        assert_eq!(
            error_delivery_state.coalesced_count(),
            0,
            "receiver closedでは集約件数を残さない"
        );
    }

    #[test]
    fn test_send_watch_event_errorはpoison復旧時にhealth_failedへlatchして送達を継続する() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
        let error_delivery_state = ErrorDeliveryState::new();
        let health_state = WatcherHealthState::new_alive();
        let poisoned_state = error_delivery_state.clone();

        let _ = std::panic::catch_unwind(move || {
            let _guard = poisoned_state.inner.lock().expect("poison前はlockできる");
            panic!("ErrorDeliveryStateをpoisonする");
        });

        send_watch_event_with_context(
            &tx,
            WatchEvent::Error(WatchError::notify("poison後も送達する")),
            "poison復旧テスト",
            &error_delivery_state,
            DEFAULT_ERROR_DELIVERY_THREAD_NAME,
            Some(&health_state),
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic),
            "poison復旧は内部panic相当としてhealthに残す"
        );
        match rx.blocking_recv().expect("poison復旧後のErrorを受信できる") {
            WatchEvent::Error(error) => assert_eq!(error.detail(), "poison後も送達する"),
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }
        assert!(
            !error_delivery_state.is_in_flight(),
            "poison復旧後に通常送達した場合はin-flightを残さない"
        );
    }

    #[test]
    fn test_watcher_drop中のhelper送達待機はdeadlockせず解除される() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        let health_state = WatcherHealthState::new_alive();
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(
            shutdown_flag,
            watcher_thread,
            health_state.clone(),
            error_delivery_state.clone(),
            tx.clone(),
        );
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        tx.blocking_send(WatchEvent::FileChanged(first.clone()))
            .expect("channelを満杯にできる");

        send_watch_event_with_context(
            &tx,
            WatchEvent::Error(WatchError::notify("drop中helper error")),
            "drop中helperテスト",
            &error_delivery_state,
            DEFAULT_ERROR_DELIVERY_THREAD_NAME,
            Some(&health_state),
        );
        assert!(
            error_delivery_state.is_in_flight(),
            "drop前にhelperが送達待機している"
        );

        drop(watcher);

        match rx.blocking_recv().expect("先行FileChangedを受信できる") {
            WatchEvent::FileChanged(path) => assert_eq!(path, first),
            WatchEvent::Error(error) => {
                panic!("先行メッセージはFileChangedを期待したがError({error})を受信")
            }
        }
        match recv_event_until(&mut rx, Duration::from_secs(1)).expect("helper Errorを受信できる")
        {
            WatchEvent::Error(error) => assert_eq!(error.detail(), "drop中helper error"),
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while error_delivery_state.is_in_flight() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            !error_delivery_state.is_in_flight(),
            "drop中に詰まっていたhelperもreceiver drain後に解除される"
        );
    }

    #[test]
    fn test_send_watch_event_errorは並列senderでも代表1件と集約サマリにまとめる() {
        const SENDER_COUNT: usize = 8;
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        tx.blocking_send(WatchEvent::FileChanged(first.clone()))
            .expect("channelを満杯にできる");
        let barrier = Arc::new(std::sync::Barrier::new(SENDER_COUNT));
        let mut handles = Vec::new();

        for index in 0..SENDER_COUNT {
            let thread_tx = tx.clone();
            let thread_state = error_delivery_state.clone();
            let thread_barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                thread_barrier.wait();
                send_watch_event(
                    &thread_tx,
                    WatchEvent::Error(WatchError::notify(format!("並列エラー{index}"))),
                    "並列senderテスト",
                    &thread_state,
                );
            }));
        }
        for handle in handles {
            handle.join().expect("sender threadがpanicしない");
        }
        drop(tx);

        match rx.blocking_recv().expect("先行FileChangedを受信できる") {
            WatchEvent::FileChanged(path) => assert_eq!(path, first),
            WatchEvent::Error(error) => {
                panic!("先行メッセージはFileChangedを期待したがError({error})を受信")
            }
        }
        match recv_event_until(&mut rx, Duration::from_secs(1)).expect("代表Errorを受信できる")
        {
            WatchEvent::Error(error) => assert!(error.detail().starts_with("並列エラー")),
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }
        match recv_event_until(&mut rx, Duration::from_secs(1))
            .expect("並列senderの集約サマリErrorを受信できる")
        {
            WatchEvent::Error(error) => assert_eq!(
                error.detail(),
                format!(
                    "送達待機中に発生した監視エラーを{}件集約しました",
                    SENDER_COUNT - 1
                )
            ),
            WatchEvent::FileChanged(path) => {
                panic!("集約サマリErrorを期待したがFileChanged({path:?})を受信")
            }
        }
        assert!(
            !error_delivery_state.is_in_flight(),
            "並列sender送達後はin-flightを解除する"
        );
        assert_eq!(
            error_delivery_state.coalesced_count(),
            0,
            "並列sender送達後は集約件数を消費する"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_send_watch_event_errorはmulti_thread_runtime内helper起動失敗時もfallback送達する()
    {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        tx.send(WatchEvent::FileChanged(first.clone()))
            .await
            .expect("channelを満杯にできる");

        super::enqueue_blocking_error_delivery_with_spawner(
            tx.clone(),
            WatchError::notify("multi_thread runtime fallback"),
            "multi_thread runtime fallbackテスト",
            &error_delivery_state,
            DEFAULT_ERROR_DELIVERY_THREAD_NAME,
            None,
            |_thread_name, _job| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "thread spawn failed",
                ))
            },
        );
        drop(tx);

        match rx.recv().await.expect("先行FileChangedを受信できる") {
            WatchEvent::FileChanged(path) => assert_eq!(path, first),
            WatchEvent::Error(error) => {
                panic!("先行メッセージはFileChangedを期待したがError({error})を受信")
            }
        }
        match tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("fallback Errorの受信がtimeoutしない")
            .expect("fallback Errorを受信できる")
        {
            WatchEvent::Error(error) => assert_eq!(error.detail(), "multi_thread runtime fallback"),
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }

        tokio::time::timeout(Duration::from_secs(1), async {
            while error_delivery_state.is_in_flight() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fallback送達後にin-flightが解除される");
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

        watcher.shutdown();
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

        watcher.shutdown();
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

        watcher.shutdown();
    }

    #[test]
    fn test_watcher_health_生成直後はaliveを返す() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let (tx, _rx) = mpsc::channel::<WatchEvent>(1);
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(
            shutdown_flag,
            watcher_thread,
            health_state,
            ErrorDeliveryState::new(),
            tx,
        );

        assert_eq!(watcher.health(), WatcherHealth::Alive);
        assert!(watcher.is_alive());

        watcher.shutdown();
    }

    #[test]
    fn test_watcher_panic経路はhealth_failedとerror_eventを記録する() {
        let health_state = WatcherHealthState::new_starting();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let error_delivery_state = ErrorDeliveryState::new();

        handle_watcher_panic(
            Box::new(String::from("panic detail")),
            "panic message",
            "panic label",
            &health_state,
            &error_delivery_state,
            &tx,
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
        );
        assert!(!matches!(health_state.load(), WatcherHealth::Alive));
        match rx.try_recv().expect("panic error eventを期待") {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::ThreadPanic);
                assert_eq!(error.detail(), "panic detail");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({:?})を受信", path)
            }
        }
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
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);

        handle_debounced_watch_result(
            Err(notify::Error::generic("notify detail")),
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
        );

        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        assert!(!matches!(health_state.load(), WatcherHealth::Alive));
        match rx.try_recv().expect("notify error eventを期待") {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert_eq!(error.detail(), "notify detail");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({:?})を受信", path)
            }
        }
    }

    #[test]
    fn test_notify_failed後もfilechanged通知は継続する() {
        let (_dir, file_path) = create_markdown_fixture("watch.md", "# before");
        let strategy = WatchStrategy::from_mode(&AppMode::new_single_file(&file_path).unwrap())
            .expect("watch strategyを作成できる");
        let health_state = WatcherHealthState::new_starting();
        let error_delivery_state = ErrorDeliveryState::new();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(2);

        handle_debounced_watch_result(
            Err(notify::Error::generic("notify detail")),
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
        );
        handle_debounced_watch_result(
            Ok(vec![notify_debouncer_mini::DebouncedEvent::new(
                file_path.clone(),
                notify_debouncer_mini::DebouncedEventKind::Any,
            )]),
            &strategy,
            &tx,
            &health_state,
            &error_delivery_state,
        );

        assert!(matches!(
            rx.try_recv().expect("notify error eventを期待"),
            WatchEvent::Error(_)
        ));
        assert_eq!(
            rx.try_recv().expect("file changed eventを期待"),
            WatchEvent::FileChanged(file_path)
        );
        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
    }

    #[test]
    fn test_failed_notifyはshutdown後もstoppedで上書きされない() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let (tx, _rx) = mpsc::channel::<WatchEvent>(1);
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(
            shutdown_flag,
            watcher_thread,
            health_state.clone(),
            ErrorDeliveryState::new(),
            tx,
        );

        health_state.store_failed(WatcherFailureKind::Notify);

        assert_eq!(
            watcher.shutdown(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::Notify)
        );
    }

    #[test]
    fn test_shutdown中のjoin_panicはhealthとerror_eventに記録する() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let watcher_thread = std::thread::spawn(|| panic!("join panic detail"));
        let watcher = Watcher::new(
            shutdown_flag,
            watcher_thread,
            health_state.clone(),
            ErrorDeliveryState::new(),
            tx,
        );

        assert_eq!(
            watcher.shutdown(),
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
        );
        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
        );
        match rx.try_recv().expect("join panic error eventを期待") {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::ThreadPanic);
                assert_eq!(error.detail(), "join panic detail");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({:?})を受信", path)
            }
        }
    }

    #[test]
    fn test_watcher_shutdown後はstoppedを記録する() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let (tx, _rx) = mpsc::channel::<WatchEvent>(1);
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(
            shutdown_flag,
            watcher_thread,
            health_state.clone(),
            ErrorDeliveryState::new(),
            tx,
        );

        let shutdown_health = watcher.shutdown();

        assert_eq!(health_state.load(), WatcherHealth::Stopped);
        assert_eq!(shutdown_health, WatcherHealth::Stopped);
    }

    #[traced_test]
    #[test]
    fn test_watch_runtime_stop_timeoutログに診断情報を含める() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let release_flag = Arc::new(AtomicBool::new(false));
        let thread_release_flag = release_flag.clone();
        let health_state = WatcherHealthState::new_alive();
        let (tx, _rx) = mpsc::channel::<WatchEvent>(1);
        let watcher_thread = std::thread::spawn(move || {
            while !thread_release_flag.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(1));
            }
        });
        let runtime = super::WatchRuntime {
            shutdown_flag,
            watcher_thread,
            health_state,
            error_delivery_state: ErrorDeliveryState::new(),
            error_tx: tx,
        };

        let shutdown_health =
            runtime.stop_with_timeout(Duration::from_secs(0), Duration::from_millis(1));
        release_flag.store(true, Ordering::Release);

        assert_eq!(shutdown_health, WatcherHealth::Stopping);
        assert!(logs_contain("監視スレッドの停止がタイムアウトしました"));
        assert!(logs_contain("elapsed_ms="));
        assert!(logs_contain("timeout_secs=0"));
    }

    #[tokio::test]
    async fn test_watcher_shutdownで監視スレッドを停止できる() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let (tx, _rx) = mpsc::channel::<WatchEvent>(1);
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(
            shutdown_flag.clone(),
            watcher_thread,
            health_state,
            ErrorDeliveryState::new(),
            tx,
        );
        watcher.shutdown();
        assert!(shutdown_flag.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn test_watcher_dropはフォールバック停止を行う() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let (tx, _rx) = mpsc::channel::<WatchEvent>(1);
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(
            shutdown_flag.clone(),
            watcher_thread,
            health_state,
            ErrorDeliveryState::new(),
            tx,
        );

        drop(watcher);
        assert!(shutdown_flag.load(Ordering::Acquire));
    }
}
