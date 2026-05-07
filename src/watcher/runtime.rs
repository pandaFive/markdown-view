use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
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
/// shutdown() のグレースフル停止待機秒数
const SHUTDOWN_TIMEOUT_SECS: u64 = 2;

type InitResult = std::result::Result<(), WatchError>;

/// watcher の稼働状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherHealth {
    /// watcher thread 起動から init 完了まで
    Starting,
    /// watcher は正常に稼働中
    Alive,
    /// watcher は監視品質が劣化、または停止している
    Failed(WatcherFailureKind),
    /// 停止処理中
    Stopping,
    /// 停止完了
    Stopped,
}

/// watcher failure の分類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        let state = Self::new_starting();
        state.store(WatcherHealth::Alive);
        state
    }

    fn load(&self) -> WatcherHealth {
        match self.state.load(Ordering::Acquire) {
            Self::STARTING => WatcherHealth::Starting,
            Self::ALIVE => WatcherHealth::Alive,
            Self::FAILED_NOTIFY => WatcherHealth::Failed(WatcherFailureKind::Notify),
            Self::FAILED_THREAD_PANIC => WatcherHealth::Failed(WatcherFailureKind::ThreadPanic),
            Self::STOPPING => WatcherHealth::Stopping,
            Self::STOPPED => WatcherHealth::Stopped,
            _ => WatcherHealth::Failed(WatcherFailureKind::ThreadPanic),
        }
    }

    fn store(&self, health: WatcherHealth) {
        let raw = match health {
            WatcherHealth::Starting => Self::STARTING,
            WatcherHealth::Alive => Self::ALIVE,
            WatcherHealth::Failed(WatcherFailureKind::Notify) => Self::FAILED_NOTIFY,
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic) => Self::FAILED_THREAD_PANIC,
            WatcherHealth::Stopping => Self::STOPPING,
            WatcherHealth::Stopped => Self::STOPPED,
        };
        self.state.store(raw, Ordering::Release);
    }

    fn store_alive_if_starting(&self) {
        let _ = self.state.compare_exchange(
            Self::STARTING,
            Self::ALIVE,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    fn store_failed(&self, kind: WatcherFailureKind) {
        self.store(WatcherHealth::Failed(kind));
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
}

impl WatchRuntime {
    /// 監視スレッドに停止を通知し、完了を待機する
    ///
    /// `SHUTDOWN_TIMEOUT_SECS` 以内にスレッドが終了しない場合はリークさせる
    /// （プロセス終了時にOSが回収する）。
    fn stop(self) {
        self.health_state.store(WatcherHealth::Stopping);
        self.shutdown_flag.store(true, Ordering::Release);
        self.watcher_thread.thread().unpark();

        let start = std::time::Instant::now();
        while !self.watcher_thread.is_finished() {
            if start.elapsed() > Duration::from_secs(SHUTDOWN_TIMEOUT_SECS) {
                tracing::warn!(
                    "[markdown-view] 監視スレッドの停止がタイムアウトしました（{}秒）",
                    SHUTDOWN_TIMEOUT_SECS
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if let Err(e) = self.watcher_thread.join() {
            self.health_state
                .store_failed(WatcherFailureKind::ThreadPanic);
            tracing::warn!(
                "[markdown-view] 監視スレッドの停止中にパニックを検出: {:?}",
                e
            );
            return;
        }
        self.health_state.store(WatcherHealth::Stopped);
    }
}

impl Watcher {
    /// 監視を開始し、監視イベント受信用チャネルを返す
    pub async fn spawn(mode: AppMode) -> Result<(Self, mpsc::Receiver<WatchEvent>)> {
        let strategy = WatchStrategy::from_mode(&mode)?;
        let watch_dir = strategy.watch_dir()?;
        let (tx, rx) = mpsc::channel::<WatchEvent>(WATCHER_MESSAGE_BUFFER);
        let (init_tx, init_rx) = oneshot::channel::<InitResult>();
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let thread_shutdown_flag = shutdown_flag.clone();
        let health_state = WatcherHealthState::new_starting();
        let thread_health_state = health_state.clone();
        let unexpected_exit = strategy.unexpected_exit_message();
        let watcher_thread = spawn_watcher_thread(
            strategy,
            watch_dir,
            tx,
            init_tx,
            thread_shutdown_flag,
            thread_health_state,
        )?;

        await_watcher_init(init_rx, unexpected_exit).await?;
        health_state.store_alive_if_starting();
        Ok((Self::new(shutdown_flag, watcher_thread, health_state), rx))
    }

    fn new(
        shutdown_flag: Arc<AtomicBool>,
        watcher_thread: std::thread::JoinHandle<()>,
        health_state: WatcherHealthState,
    ) -> Self {
        Self {
            runtime: Some(WatchRuntime {
                shutdown_flag,
                watcher_thread,
                health_state,
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
    pub fn shutdown(mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.stop();
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

/// notifyコールバックからtokioチャネルへイベントを転送する（non-blocking）。
/// チャネル満杯時・クローズ時はイベントを破棄しwarnログを出力する。
fn send_watch_event(tx: &mpsc::Sender<WatchEvent>, event: WatchEvent, label: &str) {
    match tx.try_send(event) {
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

fn spawn_watcher_thread(
    strategy: WatchStrategy,
    watch_dir: PathBuf,
    tx: mpsc::Sender<WatchEvent>,
    init_tx: oneshot::Sender<InitResult>,
    thread_shutdown_flag: Arc<AtomicBool>,
    health_state: WatcherHealthState,
) -> Result<std::thread::JoinHandle<()>> {
    let thread_name = strategy.thread_name().to_string();
    let spawn_context = format!("監視スレッド {} の起動に失敗", strategy.thread_name());
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let rt_tx = tx;
            let panic_tx = rt_tx.clone();
            let mut init_tx = Some(init_tx);
            let callback_strategy = strategy.clone();
            let recursive_mode = strategy.recursive_mode();
            let start_error_prefix = strategy.start_error_prefix();
            let panic_message = strategy.panic_message();
            let error_label = strategy.error_label();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let callback_health_state = health_state.clone();
                let debouncer = new_debouncer(
                    Duration::from_millis(DEBOUNCE_MS),
                    move |res: std::result::Result<
                        Vec<notify_debouncer_mini::DebouncedEvent>,
                        notify::Error,
                    >| {
                        handle_debounced_watch_result(
                            res,
                            &callback_strategy,
                            &rt_tx,
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

                if let Err(e) = debouncer.watcher().watch(&watch_dir, recursive_mode) {
                    send_init_result(
                        &mut init_tx,
                        Err(WatchError::init(format!("{}: {}", start_error_prefix, e))),
                    );
                    return;
                }

                send_init_result(&mut init_tx, Ok(()));
                keep_watcher_thread_alive(&thread_shutdown_flag);
            }));

            if let Err(panic_payload) = result {
                handle_watcher_panic(
                    panic_payload,
                    panic_message,
                    error_label,
                    &health_state,
                    &panic_tx,
                );
            }
        })
        .context(spawn_context)
}

fn handle_debounced_watch_result(
    result: std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>,
    strategy: &WatchStrategy,
    tx: &mpsc::Sender<WatchEvent>,
    health_state: &WatcherHealthState,
) {
    match result {
        Ok(events) => {
            for changed_path in strategy.collect_changed_paths(&events) {
                send_watch_event(
                    tx,
                    WatchEvent::FileChanged(changed_path),
                    strategy.change_label(),
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
            send_watch_event(tx, WatchEvent::Error(watch_error), strategy.error_label());
        }
    }
}

fn handle_watcher_panic(
    panic_payload: Box<dyn std::any::Any + Send>,
    panic_message: &str,
    error_label: &str,
    health_state: &WatcherHealthState,
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
    send_watch_event(tx, WatchEvent::Error(watch_error), error_label);
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

fn keep_watcher_thread_alive(shutdown_flag: &AtomicBool) {
    while !shutdown_flag.load(Ordering::Acquire) {
        std::thread::park_timeout(Duration::from_millis(WATCHER_THREAD_PARK_MS));
    }
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

    use super::{
        handle_debounced_watch_result, handle_watcher_panic, keep_watcher_thread_alive,
        send_watch_event, Watcher, WatcherFailureKind, WatcherHealth, WatcherHealthState,
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
        std::thread::spawn(move || keep_watcher_thread_alive(&shutdown_flag))
    }

    #[test]
    fn test_send_watch_event_チャネル満杯時はメッセージを破棄してブロックしない() {
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        tx.blocking_send(WatchEvent::FileChanged(first.clone()))
            .unwrap();

        send_watch_event(
            &tx,
            WatchEvent::Error(WatchError::notify("満杯時テスト")),
            "満杯時テスト",
        );

        match rx.blocking_recv().unwrap() {
            WatchEvent::FileChanged(path) => assert_eq!(path, first),
            WatchEvent::Error(_) => panic!("最初のメッセージはFileChangedを期待"),
        }

        assert!(
            rx.try_recv().is_err(),
            "満杯時のメッセージは破棄されているはず"
        );
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

    #[test]
    fn test_watcher_health_生成直後はaliveを返す() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(shutdown_flag, watcher_thread, health_state);

        assert_eq!(watcher.health(), WatcherHealth::Alive);
        assert!(watcher.is_alive());

        watcher.shutdown();
    }

    #[test]
    fn test_watcher_panic経路はhealth_failedとerror_eventを記録する() {
        let health_state = WatcherHealthState::new_starting();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);

        handle_watcher_panic(
            Box::new(String::from("panic detail")),
            "panic message",
            "panic label",
            &health_state,
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
    fn test_notify_error経路はhealth_failedとerror_eventを記録する() {
        let (_dir, file_path) = create_markdown_fixture("watch.md", "# before");
        let strategy = WatchStrategy::from_mode(&AppMode::new_single_file(&file_path).unwrap())
            .expect("watch strategyを作成できる");
        let health_state = WatcherHealthState::new_starting();
        let (tx, mut rx) = mpsc::channel::<WatchEvent>(1);

        handle_debounced_watch_result(
            Err(notify::Error::generic("notify detail")),
            &strategy,
            &tx,
            &health_state,
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
    fn test_watcher_shutdown後はstoppedを記録する() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(shutdown_flag, watcher_thread, health_state.clone());

        watcher.shutdown();

        assert_eq!(health_state.load(), WatcherHealth::Stopped);
    }

    #[tokio::test]
    async fn test_watcher_shutdownで監視スレッドを停止できる() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(shutdown_flag.clone(), watcher_thread, health_state);
        watcher.shutdown();
        assert!(shutdown_flag.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn test_watcher_dropはフォールバック停止を行う() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(shutdown_flag.clone(), watcher_thread, health_state);

        drop(watcher);
        assert!(shutdown_flag.load(Ordering::Acquire));
    }
}
