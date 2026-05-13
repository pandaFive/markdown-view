use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use super::super::{WatchError, WatchEvent};
use super::dispatch::{send_merged_file_changed_event, WatcherDiagnostics};
use super::error_queue::{PriorityErrorReceiver, PriorityErrorSender};
use super::health::{WatcherFailureKind, WatcherHealthState};

pub(super) const WATCHER_THREAD_PARK_MS: u64 = 250;
pub(crate) const WATCH_SHUTDOWN_TIMEOUT_SECS: u64 = 2;
/// shutdown診断イベントの送信待機上限（ミリ秒）
pub(super) const WATCHER_DIAGNOSTIC_SEND_TIMEOUT_MS: u64 = 200;
pub(super) enum WatcherThreadStopResult {
    Stopped,
    Panicked(Box<dyn std::any::Any + Send>),
    TimedOut,
}

pub(super) struct MergeForwarderHandle {
    task: tokio::task::JoinHandle<()>,
    done_rx: oneshot::Receiver<()>,
}

pub(super) struct ForwarderDoneOnDrop(Option<oneshot::Sender<()>>);

impl Drop for ForwarderDoneOnDrop {
    fn drop(&mut self) {
        if let Some(done_tx) = self.0.take() {
            // abort_without_wait経路では受信側が先にdropされていることがある。
            let _ = done_tx.send(());
        }
    }
}

impl MergeForwarderHandle {
    pub(super) fn abort_without_wait(self) {
        self.task.abort();
    }

    #[cfg(test)]
    pub(super) async fn await_completion(self) -> Result<(), tokio::task::JoinError> {
        self.task.await
    }

    pub(super) async fn stop(self, timeout: Duration, diagnostics: &WatcherDiagnostics) {
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
pub(super) fn record_shutdown_task_panic(
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
pub(super) fn join_watcher_thread_with_timeout(
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
pub(super) fn spawn_watch_event_merge_forwarder(
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

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::{mpsc, oneshot};
    use tracing_test::traced_test;

    use crate::watcher::{WatchError, WatchErrorKind, WatchEvent, Watcher, WatcherHealth};

    use super::super::error_queue::priority_error_channel;

    #[tokio::test]
    async fn test_error_eventはfile_channel満杯時もmerged_rxに届く() {
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(1);
        let (error_tx, error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_QUEUE_CAPACITY);
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

        let forwarder =
            super::super::shutdown::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
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
        let (error_tx, error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_QUEUE_CAPACITY);
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);
        let (_dir, changed) = create_markdown_fixture("changed.md", "# changed");
        file_tx.send(changed).await.expect("file eventを送信できる");
        error_tx.send(WatchError::notify("優先されるerror"), "優先テスト");

        let forwarder =
            super::super::shutdown::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
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
        let (error_tx, error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_QUEUE_CAPACITY);
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(1);
        let (_dir, prefilled) = create_markdown_fixture("prefilled.md", "# prefilled");
        let (_dir2, changed) = create_markdown_fixture("changed.md", "# changed");
        merged_tx
            .send(WatchEvent::FileChanged(prefilled.clone()))
            .await
            .expect("merged channelを満杯にできる");
        file_tx.send(changed).await.expect("file eventを送信できる");

        let forwarder =
            super::super::shutdown::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
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
        const ERROR_COUNT: usize = super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER + 4;

        let (file_tx, file_rx) =
            mpsc::channel::<PathBuf>(super::super::dispatch::WATCHER_MESSAGE_BUFFER);
        let (error_tx, error_rx) = priority_error_channel(ERROR_COUNT);
        let error_sender = error_tx.clone();
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(1);
        let (_dir, prefilled) = create_markdown_fixture("prefilled.md", "# prefilled");
        merged_tx
            .send(WatchEvent::FileChanged(prefilled))
            .await
            .expect("merged channelを満杯にできる");

        for index in 0..super::super::dispatch::WATCHER_MESSAGE_BUFFER {
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

        let forwarder =
            super::super::shutdown::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
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
        let (error_tx, error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_QUEUE_CAPACITY);
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

        let forwarder =
            super::super::shutdown::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
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
        let (error_tx, error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_QUEUE_CAPACITY);
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

        let forwarder =
            super::super::shutdown::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
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
        let (error_tx, mut error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER);
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
    async fn test_watcher_shutdownはmerge_forwarder_panicを戻り値に反映する() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let health_state = WatcherHealthState::new_alive();
        let watcher_thread = spawn_idle_watcher_thread(shutdown_flag.clone());
        let (done_tx, done_rx) = oneshot::channel::<()>();
        let merge_forwarder = super::MergeForwarderHandle {
            task: tokio::spawn(async move {
                let _done = super::ForwarderDoneOnDrop(Some(done_tx));
                panic!("forwarder panic during shutdown");
            }),
            done_rx,
        };
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);
        let diagnostics = WatcherDiagnostics::new(merged_tx, health_state.clone());
        let (error_tx, _error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER);
        let watcher = Watcher::new(
            shutdown_flag,
            watcher_thread,
            merge_forwarder,
            health_state.clone(),
            diagnostics,
            error_tx,
        );

        let shutdown_health = shutdown_watcher_for_test(watcher).await;

        assert_eq!(
            shutdown_health,
            WatcherHealth::Failed(WatcherFailureKind::ForwarderTaskPanic)
        );
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
        let (error_tx, error_rx) =
            priority_error_channel(super::super::error_queue::WATCHER_ERROR_MESSAGE_BUFFER);
        let merge_forwarder =
            super::super::shutdown::spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        drop(file_tx);
        let watcher_thread = std::thread::spawn(move || {
            while !thread_release_flag.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(1));
            }
        });
        let runtime = super::super::thread::WatchRuntime {
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
}
