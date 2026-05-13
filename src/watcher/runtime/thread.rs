use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify_debouncer_mini::new_debouncer;
use tokio::sync::oneshot;

use super::super::strategy::{WatchPlan, WatchStrategy};
use super::super::WatchError;
use super::dispatch::{
    handle_debounced_watch_result, handle_internal_channel_disconnected,
    process_debounced_events_with_watch_and_unwatch, send_internal_watch_result,
    InternalWatchResult, WatchEventSenders, WatcherDiagnostics, WATCHER_INTERNAL_EVENT_BUFFER,
};
use super::error_queue::PriorityErrorSender;
use super::health::{WatcherFailureKind, WatcherHealth, WatcherHealthState};
use super::registration::{register_watch_plan_with, WatchDirectoryRegistry};
use super::shutdown::{
    join_watcher_thread_with_timeout, record_shutdown_task_panic, MergeForwarderHandle,
    WatcherThreadStopResult, WATCHER_THREAD_PARK_MS, WATCH_SHUTDOWN_TIMEOUT_SECS,
};

const DEBOUNCE_MS: u64 = 300;
pub(super) type InitResult = std::result::Result<(), WatchError>;
pub(super) struct WatchRuntime {
    pub(super) shutdown_flag: Arc<AtomicBool>,
    pub(super) watcher_thread: std::thread::JoinHandle<()>,
    pub(super) merge_forwarder: MergeForwarderHandle,
    pub(super) health_state: WatcherHealthState,
    pub(super) diagnostics: WatcherDiagnostics,
    pub(super) error_tx: PriorityErrorSender,
}

impl WatchRuntime {
    /// 監視スレッドに停止を通知し、完了を待機する
    ///
    /// `WATCH_SHUTDOWN_TIMEOUT_SECS` 以内にスレッドが終了しない場合はjoin待機を打ち切り、
    /// thread handleをdetachする（プロセス終了時にOSが回収する）。
    pub(super) async fn stop(self) -> WatcherHealth {
        self.stop_with_timeout(
            Duration::from_secs(WATCH_SHUTDOWN_TIMEOUT_SECS),
            Duration::from_millis(50),
        )
        .await
    }

    pub(super) async fn stop_with_timeout(
        self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> WatcherHealth {
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
                let mut init_tx = None;
                handle_watcher_panic(
                    panic_payload,
                    "監視スレッドの停止中にパニックを検出",
                    "監視スレッド停止時パニック",
                    &health_state,
                    &error_tx,
                    &mut init_tx,
                );
            }
            Ok(WatcherThreadStopResult::Stopped) => {
                health_state.store_stopped_if_not_failed();
            }
            Err(error) => {
                record_shutdown_task_panic(&health_state, &error_tx, error);
            }
        }

        drop(error_tx);
        merge_forwarder
            .stop(timeout.saturating_sub(elapsed), &diagnostics)
            .await;
        health_state.load()
    }

    pub(super) fn request_stop_without_wait(self) {
        self.health_state.store_stopping_if_not_failed();
        self.shutdown_flag.store(true, Ordering::Release);
        self.watcher_thread.thread().unpark();
        self.merge_forwarder.abort_without_wait();
    }
}
pub(super) fn spawn_watcher_thread(
    strategy: WatchStrategy,
    watch_plan: WatchPlan,
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
                    &mut init_tx,
                );
            }
        })
        .context(spawn_context)
}
pub(super) fn handle_watcher_panic(
    panic_payload: Box<dyn std::any::Any + Send>,
    panic_message: &str,
    error_label: &str,
    health_state: &WatcherHealthState,
    error_tx: &PriorityErrorSender,
    init_tx: &mut Option<oneshot::Sender<InitResult>>,
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
    if init_tx.is_some() {
        send_init_result(init_tx, Err(watch_error.clone()));
    }
    tracing::error!("[markdown-view] {}: {}", panic_message, panic_detail);
    error_tx.send(watch_error, error_label);
}

pub(super) fn send_init_result(
    init_tx: &mut Option<oneshot::Sender<InitResult>>,
    result: InitResult,
) {
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

pub(super) fn run_watcher_event_loop(
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
pub(super) async fn await_watcher_init(
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
    use super::super::test_support::*;
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::oneshot;
    use tracing_test::traced_test;

    use crate::server::AppMode;
    use crate::watcher::{WatchError, WatchErrorKind, WatchEvent, Watcher};

    use super::super::error_queue::priority_error_channel;

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
    fn test_watcher_panic経路はinit_tx残存時にthread_panicをinit_resultへ返す() {
        let health_state = WatcherHealthState::new_starting();
        let (error_tx, mut error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER);
        let (init_tx, mut init_rx) = oneshot::channel::<InitResult>();
        let mut init_tx = Some(init_tx);

        handle_watcher_panic(
            Box::new(String::from("init panic detail")),
            "panic message",
            "panic label",
            &health_state,
            &error_tx,
            &mut init_tx,
        );

        let init_error = init_rx
            .try_recv()
            .expect("init resultを受信できる")
            .expect_err("init前panicはThreadPanicとして返す");
        assert_eq!(init_error.kind(), WatchErrorKind::ThreadPanic);
        assert_eq!(init_error.detail(), "init panic detail");
        assert!(init_tx.is_none());
        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
        );

        let event_error = error_rx.try_recv().expect("panic error eventを期待");
        assert_eq!(event_error.kind(), WatchErrorKind::ThreadPanic);
        assert_eq!(event_error.detail(), "init panic detail");
    }

    #[tokio::test]
    async fn test_watcher_spawn相当のinit待機境界は初期化前panicをthread_panicとして返す() {
        let (init_rx, mut error_rx, health_state, watcher_thread) =
            spawn_init_panic_thread_for_test("spawn init panic detail");

        let spawn_error = tokio::time::timeout(
            Duration::from_secs(1),
            super::await_watcher_init(init_rx, "watcher thread exited before init"),
        )
        .await
        .expect("init待機がtimeoutしない")
        .expect_err("初期化前panicはspawn境界で失敗として返す");

        let watch_error = spawn_error
            .downcast_ref::<WatchError>()
            .expect("WatchErrorとして返す");
        assert_eq!(watch_error.kind(), WatchErrorKind::ThreadPanic);
        assert_eq!(watch_error.detail(), "spawn init panic detail");
        assert_eq!(
            health_state.load(),
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
        );

        watcher_thread.join().expect("panicはthread内で捕捉される");

        let event_error = error_rx.try_recv().expect("panic error eventを期待");
        assert_eq!(event_error.kind(), WatchErrorKind::ThreadPanic);
        assert_eq!(event_error.detail(), "spawn init panic detail");
    }

    #[test]
    fn test_watcher_panic経路はhealth_failedとerror_eventを記録する() {
        let health_state = WatcherHealthState::new_starting();
        let (error_tx, mut error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER);
        let mut init_tx = None;

        handle_watcher_panic(
            Box::new(String::from("panic detail")),
            "panic message",
            "panic label",
            &health_state,
            &error_tx,
            &mut init_tx,
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
        let (error_tx, mut error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER);
        let mut init_tx = None;

        handle_watcher_panic(
            Box::new(anyhow::anyhow!("anyhow panic detail")),
            "panic message",
            "panic label",
            &health_state,
            &error_tx,
            &mut init_tx,
        );

        let error = error_rx.try_recv().expect("panic error eventを期待");
        assert_eq!(error.kind(), WatchErrorKind::ThreadPanic);
        assert_eq!(error.detail(), "anyhow panic detail");
    }

    #[test]
    fn test_watcher_panic経路はanyhow_payload_detailをinit_resultにも保持する() {
        let health_state = WatcherHealthState::new_starting();
        let (error_tx, mut error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER);
        let (init_tx, mut init_rx) = oneshot::channel::<InitResult>();
        let mut init_tx = Some(init_tx);

        handle_watcher_panic(
            Box::new(anyhow::anyhow!("anyhow init panic detail")),
            "panic message",
            "panic label",
            &health_state,
            &error_tx,
            &mut init_tx,
        );

        let init_error = init_rx
            .try_recv()
            .expect("init resultを受信できる")
            .expect_err("init前panicはThreadPanicとして返す");
        assert_eq!(init_error.kind(), WatchErrorKind::ThreadPanic);
        assert_eq!(init_error.detail(), "anyhow init panic detail");

        let event_error = error_rx.try_recv().expect("panic error eventを期待");
        assert_eq!(event_error.kind(), WatchErrorKind::ThreadPanic);
        assert_eq!(event_error.detail(), "anyhow init panic detail");
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
