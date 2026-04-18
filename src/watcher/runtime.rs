use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
}

impl WatchRuntime {
    /// 監視スレッドに停止を通知し、完了を待機する
    ///
    /// `SHUTDOWN_TIMEOUT_SECS` 以内にスレッドが終了しない場合はリークさせる
    /// （プロセス終了時にOSが回収する）。
    fn stop(self) {
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
            tracing::warn!(
                "[markdown-view] 監視スレッドの停止中にパニックを検出: {:?}",
                e
            );
        }
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
        let unexpected_exit = strategy.unexpected_exit_message();
        let watcher_thread =
            spawn_watcher_thread(strategy, watch_dir, tx, init_tx, thread_shutdown_flag)?;

        await_watcher_init(init_rx, unexpected_exit).await?;
        Ok((Self::new(shutdown_flag, watcher_thread), rx))
    }

    fn new(shutdown_flag: Arc<AtomicBool>, watcher_thread: std::thread::JoinHandle<()>) -> Self {
        Self {
            runtime: Some(WatchRuntime {
                shutdown_flag,
                watcher_thread,
            }),
        }
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
                let debouncer = new_debouncer(
                    Duration::from_millis(DEBOUNCE_MS),
                    move |res: std::result::Result<
                        Vec<notify_debouncer_mini::DebouncedEvent>,
                        notify::Error,
                    >| match res {
                        Ok(events) => {
                            for changed_path in callback_strategy.collect_changed_paths(&events) {
                                send_watch_event(
                                    &rt_tx,
                                    WatchEvent::FileChanged(changed_path),
                                    callback_strategy.change_label(),
                                );
                            }
                        }
                        Err(e) => {
                            let watch_error = WatchError::notify(e.to_string());
                            tracing::warn!(
                                "[markdown-view] {}: {}",
                                callback_strategy.watch_error_prefix(),
                                watch_error.detail()
                            );
                            send_watch_event(
                                &rt_tx,
                                WatchEvent::Error(watch_error),
                                callback_strategy.error_label(),
                            );
                        }
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
                let panic_detail = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "不明なパニック".to_string()
                };
                let watch_error = WatchError::thread_panic(panic_detail.clone());
                tracing::error!("[markdown-view] {}: {}", panic_message, panic_detail);
                send_watch_event(&panic_tx, WatchEvent::Error(watch_error), error_label);
            }
        })
        .context(spawn_context)
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

    use super::{keep_watcher_thread_alive, send_watch_event, Watcher};
    use crate::server::AppMode;
    use crate::watcher::{WatchError, WatchEvent};

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

    #[tokio::test]
    async fn test_watcher_shutdownで監視スレッドを停止できる() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(shutdown_flag.clone(), watcher_thread);
        watcher.shutdown();
        assert!(shutdown_flag.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn test_watcher_dropはフォールバック停止を行う() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let watcher = Watcher::new(shutdown_flag.clone(), watcher_thread);

        drop(watcher);
        assert!(shutdown_flag.load(Ordering::Acquire));
    }
}
