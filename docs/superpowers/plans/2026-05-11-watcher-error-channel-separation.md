# Watcher Error Channel Separation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure watcher errors are delivered through an internal error path that is separate from best-effort file change notifications, while preserving the existing `Watcher::spawn()` public API.

**Architecture:** Split watcher runtime delivery into file and error input channels, then re-merge them into the existing `mpsc::Receiver<WatchEvent>` returned to server code. File changes keep `try_send` best-effort behavior; errors use a dedicated bounded channel and are prioritized by an internal merge forwarder.

**Tech Stack:** Rust, Tokio `mpsc`, standard watcher thread, existing watcher runtime unit tests, `cargo test`, `./verify.sh`.

---

## Scope And File Structure

**Modify:** `src/watcher/runtime.rs`

Responsibilities:

- Add `WATCHER_ERROR_MESSAGE_BUFFER`.
- Add internal delivery types for split channels and forwarder handles.
- Replace variant-mixed `send_watch_event` with `send_file_changed_event` and `send_error_event`.
- Add an internal merge forwarder that prioritizes error events and emits existing `WatchEvent` values.
- Update watcher runtime wiring and shutdown to own the merge forwarder.
- Update watcher runtime tests.

**Do not modify:** `src/watcher/mod.rs`, `src/server/watch.rs`, `src/server/broadcast.rs`

The public `WatchEvent` enum and `Watcher::spawn() -> Result<(Watcher, mpsc::Receiver<WatchEvent>)>` contract stay unchanged.

## Task 0: Prepare Isolated Worktree

**Files:**
- No source file changes.

- [ ] **Step 1: Confirm current branch and untracked files**

Run:

```bash
git status --short --branch
```

Expected: working tree may include the existing untracked `docs/superpowers/plans/2026-05-09-watcher-error-event-delivery.md`. Do not delete or modify it.

- [ ] **Step 2: Create an implementation worktree**

Run:

```bash
git worktree add ../markdown-view-watcher-error-channel-separation -b fix/watcher-error-channel-separation
cd ../markdown-view-watcher-error-channel-separation
```

Expected: new worktree on `fix/watcher-error-channel-separation`. If the branch name already exists, use `fix/watcher-error-channel-separation-2`.

## Task 1: Add Failing Tests For Split Delivery

**Files:**
- Modify: `src/watcher/runtime.rs`

- [ ] **Step 1: Replace the old single-channel full test with FileChanged-specific coverage**

In `src/watcher/runtime.rs`, replace the test named `test_send_watch_event_チャネル満杯時はメッセージを破棄してブロックしない` with:

```rust
    #[test]
    fn test_send_file_changed_eventはチャネル満杯時に破棄してブロックしない() {
        let (file_tx, mut file_rx) = mpsc::channel::<PathBuf>(1);
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        let (_dir2, second) = create_markdown_fixture("second.md", "# second");
        file_tx
            .blocking_send(first.clone())
            .expect("file channelを満杯にできる");

        send_file_changed_event(&file_tx, second, "filechanged満杯時テスト");

        assert_eq!(
            file_rx
                .blocking_recv()
                .expect("先行file eventを受信できる"),
            first
        );
        assert!(
            file_rx.try_recv().is_err(),
            "満杯時のFileChangedは破棄されているはず"
        );
    }
```

- [ ] **Step 2: Add failing coverage that Error bypasses a full file channel**

Add this test immediately after the FileChanged full test:

```rust
    #[tokio::test]
    async fn test_error_eventはfile_channel満杯時もmerged_rxに届く() {
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(1);
        let (error_tx, error_rx) = mpsc::channel::<WatchError>(1);
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);
        let (_dir, first) = create_markdown_fixture("first.md", "# first");
        file_tx
            .send(first)
            .await
            .expect("file channelを満杯にできる");

        let (forwarder, _done_rx) = spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
        send_error_event(
            &error_tx,
            WatchError::notify("file channelが満杯でも送達する"),
            "error分離テスト",
        );
        drop(file_tx);
        drop(error_tx);

        match tokio::time::timeout(Duration::from_secs(1), merged_rx.recv())
            .await
            .expect("merged eventを待てる")
            .expect("merged eventを受信できる")
        {
            WatchEvent::Error(error) => {
                assert_eq!(error.kind(), WatchErrorKind::Notify);
                assert_eq!(error.detail(), "file channelが満杯でも送達する");
            }
            WatchEvent::FileChanged(path) => {
                panic!("Errorを期待したがFileChanged({path:?})を受信")
            }
        }

        forwarder.await.expect("merge forwarderが正常終了する");
    }
```

Expected initial failure: `send_file_changed_event`, `send_error_event`, and `spawn_watch_event_merge_forwarder` do not exist yet.

- [ ] **Step 3: Add failing coverage for Error priority**

Add this test after the previous one:

```rust
    #[tokio::test]
    async fn test_merge_forwarderはfileよりerrorを優先する() {
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(4);
        let (error_tx, error_rx) = mpsc::channel::<WatchError>(4);
        let (merged_tx, mut merged_rx) = mpsc::channel::<WatchEvent>(4);
        let (_dir, changed) = create_markdown_fixture("changed.md", "# changed");
        file_tx
            .send(changed)
            .await
            .expect("file eventを送信できる");
        error_tx
            .send(WatchError::notify("優先されるerror"))
            .await
            .expect("error eventを送信できる");

        let (forwarder, _done_rx) = spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
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

        forwarder.await.expect("merge forwarderが正常終了する");
    }
```

- [ ] **Step 4: Add receiver-closed coverage for Error helper**

Add this test after the priority test:

```rust
    #[test]
    fn test_send_error_eventはreceiver_closedでもpanicしない() {
        let (error_tx, error_rx) = mpsc::channel::<WatchError>(1);
        drop(error_rx);

        send_error_event(
            &error_tx,
            WatchError::notify("receiver closed"),
            "error receiver closedテスト",
        );
    }
```

- [ ] **Step 5: Run the new focused tests and verify failure**

Run:

```bash
cargo test --all-targets --all-features test_error_eventはfile_channel満杯時もmerged_rxに届く
```

Expected: FAIL at compile time because split delivery helpers are not implemented.

- [ ] **Step 6: Commit failing tests**

Run:

```bash
git add src/watcher/runtime.rs
git commit -m "test: watcher異常通知の分離配送を固定"
```

Expected: commit succeeds with test-only changes.

## Task 2: Implement Internal Split Channels And Merge Forwarder

**Files:**
- Modify: `src/watcher/runtime.rs`

- [ ] **Step 1: Add the error buffer constant**

Near `WATCHER_MESSAGE_BUFFER`, add:

```rust
/// watcher error から外部 WatchEvent へ橋渡しする専用チャネル容量
const WATCHER_ERROR_MESSAGE_BUFFER: usize = 8;
```

- [ ] **Step 2: Add split sender and merge forwarder fields**

Replace `WatchRuntime`:

```rust
struct WatchRuntime {
    shutdown_flag: Arc<AtomicBool>,
    watcher_thread: std::thread::JoinHandle<()>,
    merge_forwarder: tokio::task::JoinHandle<()>,
    merge_forwarder_done: std::sync::mpsc::Receiver<()>,
    health_state: WatcherHealthState,
    error_tx: mpsc::Sender<WatchError>,
}
```

Add this internal sender type near `WatchRuntime`:

```rust
#[derive(Clone)]
struct WatchEventSenders {
    file_tx: mpsc::Sender<PathBuf>,
    error_tx: mpsc::Sender<WatchError>,
}
```

- [ ] **Step 3: Add the merge forwarder helper**

Place this near `send_watch_event` after replacing it in the next step, or before `spawn_watcher_thread`:

```rust
fn spawn_watch_event_merge_forwarder(
    mut file_rx: mpsc::Receiver<PathBuf>,
    mut error_rx: mpsc::Receiver<WatchError>,
    merged_tx: mpsc::Sender<WatchEvent>,
) -> (
    tokio::task::JoinHandle<()>,
    std::sync::mpsc::Receiver<()>,
) {
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let task = tokio::spawn(async move {
        let mut file_closed = false;
        let mut error_closed = false;

        loop {
            while let Ok(error) = error_rx.try_recv() {
                if merged_tx.send(WatchEvent::Error(error)).await.is_err() {
                    let _ = done_tx.send(());
                    return;
                }
            }

            if file_closed && error_closed {
                break;
            }

            tokio::select! {
                biased;

                error = error_rx.recv(), if !error_closed => {
                    match error {
                        Some(error) => {
                            if merged_tx.send(WatchEvent::Error(error)).await.is_err() {
                                let _ = done_tx.send(());
                                return;
                            }
                        }
                        None => error_closed = true,
                    }
                }
                file = file_rx.recv(), if !file_closed => {
                    match file {
                        Some(path) => {
                            if merged_tx.send(WatchEvent::FileChanged(path)).await.is_err() {
                                let _ = done_tx.send(());
                                return;
                            }
                        }
                        None => file_closed = true,
                    }
                }
            }
        }

        let _ = done_tx.send(());
    });
    (task, done_rx)
}
```

- [ ] **Step 4: Replace `send_watch_event` with split helpers**

Replace the current `send_watch_event` function with:

```rust
/// notifyコールバックからtokioチャネルへ通常変更通知を転送する。
/// FileChangedは過負荷時に破棄する。
fn send_file_changed_event(tx: &mpsc::Sender<PathBuf>, path: PathBuf, label: &str) {
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
        }
    }
}

/// watcher異常通知を専用チャネルへ転送する。
/// ErrorはFileChangedのbacklogとは分離し、receiverが開いている限り送達を待つ。
fn send_error_event(tx: &mpsc::Sender<WatchError>, error: WatchError, label: &str) {
    let detail = error.detail().to_string();
    match tx.blocking_send(error) {
        Ok(()) => {}
        Err(_) => {
            tracing::warn!(
                "[markdown-view] 通知チャネルが閉じているため監視エラーを送達できませんでした: {}, {}",
                label,
                detail
            );
        }
    }
}
```

- [ ] **Step 5: Update `Watcher::spawn()` channel wiring**

In `Watcher::spawn()`, replace the single `tx/rx` creation block:

```rust
        let (merged_tx, merged_rx) = mpsc::channel::<WatchEvent>(WATCHER_MESSAGE_BUFFER);
        let (file_tx, file_rx) = mpsc::channel::<PathBuf>(WATCHER_MESSAGE_BUFFER);
        let (error_tx, error_rx) = mpsc::channel::<WatchError>(WATCHER_ERROR_MESSAGE_BUFFER);
        let senders = WatchEventSenders {
            file_tx,
            error_tx: error_tx.clone(),
        };
        let (merge_forwarder, merge_forwarder_done) =
            spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
```

Pass `senders` to `spawn_watcher_thread`, and return:

```rust
        Ok((
            Self::new(
                shutdown_flag,
                watcher_thread,
                merge_forwarder,
                merge_forwarder_done,
                health_state,
                error_tx,
            ),
            merged_rx,
        ))
```

- [ ] **Step 6: Update `Watcher::new()` signature**

Replace `Watcher::new(...)` with:

```rust
    fn new(
        shutdown_flag: Arc<AtomicBool>,
        watcher_thread: std::thread::JoinHandle<()>,
        merge_forwarder: tokio::task::JoinHandle<()>,
        merge_forwarder_done: std::sync::mpsc::Receiver<()>,
        health_state: WatcherHealthState,
        error_tx: mpsc::Sender<WatchError>,
    ) -> Self {
        Self {
            runtime: Some(WatchRuntime {
                shutdown_flag,
                watcher_thread,
                merge_forwarder,
                merge_forwarder_done,
                health_state,
                error_tx,
            }),
        }
    }
```

- [ ] **Step 7: Update `spawn_watcher_thread()` signature and call sites**

Change the parameter:

```rust
    senders: WatchEventSenders,
```

Inside the spawned thread, replace:

```rust
            let rt_tx = tx;
            let panic_tx = rt_tx.clone();
```

with:

```rust
            let rt_senders = senders;
            let panic_error_tx = rt_senders.error_tx.clone();
```

Inside the debouncer setup, replace `callback_tx` with:

```rust
                let callback_senders = rt_senders.clone();
```

Pass `&callback_senders` to `send_internal_watch_result`. In `run_watcher_event_loop`, pass `&rt_senders`. In `handle_watcher_panic`, pass `&panic_error_tx`.

- [ ] **Step 8: Run focused compile/test**

Run:

```bash
cargo test --all-targets --all-features test_send_file_changed_eventはチャネル満杯時に破棄してブロックしない
```

Expected: compile errors remain for functions that still accept `mpsc::Sender<WatchEvent>`. Fix them in Task 3.

## Task 3: Convert Runtime Call Sites To Split Senders

**Files:**
- Modify: `src/watcher/runtime.rs`

- [ ] **Step 1: Update `send_internal_watch_result` signature and body**

Change the parameter:

```rust
    senders: &WatchEventSenders,
```

Replace both error sends with:

```rust
            send_error_event(
                &senders.error_tx,
                WatchError::notify("watcher internal channel が満杯です"),
                strategy.error_label(),
            );
```

and:

```rust
            send_error_event(
                &senders.error_tx,
                WatchError::notify("watcher internal channel が閉じています"),
                strategy.error_label(),
            );
```

- [ ] **Step 2: Update `handle_debounced_watch_result` signature and body**

Change the parameter from `tx: &mpsc::Sender<WatchEvent>` to:

```rust
    senders: &WatchEventSenders,
```

Replace FileChanged sends with:

```rust
                send_file_changed_event(
                    &senders.file_tx,
                    changed_path,
                    strategy.change_label(),
                );
```

Replace Error sends with:

```rust
            send_error_event(&senders.error_tx, watch_error, strategy.error_label());
```

- [ ] **Step 3: Update `process_debounced_events_with_watch` test helper**

Change the parameter from `tx: &mpsc::Sender<WatchEvent>` to:

```rust
    senders: &WatchEventSenders,
```

Pass `senders` through to `process_debounced_events_with_watch_and_unwatch`.

- [ ] **Step 4: Update `process_debounced_events_with_watch_and_unwatch`**

Change the parameter from `tx: &mpsc::Sender<WatchEvent>` to:

```rust
    senders: &WatchEventSenders,
```

Replace the initial changed path send with:

```rust
        send_file_changed_event(
            &senders.file_tx,
            changed_path,
            strategy.change_label(),
        );
```

Replace recovery sends with the same `send_file_changed_event(...)` call.

Replace registration error send with:

```rust
            send_error_event(&senders.error_tx, watch_error, strategy.error_label());
```

- [ ] **Step 5: Update panic and disconnected handlers**

Change `handle_watcher_panic` parameter from `tx: &mpsc::Sender<WatchEvent>` to:

```rust
    error_tx: &mpsc::Sender<WatchError>,
```

Replace its send with:

```rust
    send_error_event(error_tx, watch_error, error_label);
```

Change `handle_internal_channel_disconnected` parameter from `tx: &mpsc::Sender<WatchEvent>` to:

```rust
    senders: &WatchEventSenders,
```

Replace its send with:

```rust
    send_error_event(
        &senders.error_tx,
        WatchError::notify("watcher internal channel が切断されました"),
        strategy.error_label(),
    );
```

- [ ] **Step 6: Update `run_watcher_event_loop` signature**

Change its `tx` parameter to:

```rust
    senders: &WatchEventSenders,
```

Pass `senders` into `handle_debounced_watch_result` and `handle_internal_channel_disconnected`.

- [ ] **Step 7: Update tests to create split senders and merge forwarder**

For tests that currently create:

```rust
let (tx, mut rx) = mpsc::channel::<WatchEvent>(N);
```

and pass `&tx` into watcher runtime helpers, replace with:

```rust
let (file_tx, file_rx) = mpsc::channel::<PathBuf>(N);
let (error_tx, error_rx) = mpsc::channel::<WatchError>(N);
let (merged_tx, mut rx) = mpsc::channel::<WatchEvent>(N);
let senders = WatchEventSenders {
    file_tx,
    error_tx,
};
let (forwarder, _done_rx) = spawn_watch_event_merge_forwarder(file_rx, error_rx, merged_tx);
```

After the helper under test, drop sender clones so the forwarder can exit:

```rust
drop(senders);
```

At the end of async tests, wait for forwarder:

```rust
forwarder.await.expect("merge forwarderが正常終了する");
```

For non-async tests that need merged receive assertions, convert them to `#[tokio::test] async fn ...` and use `tokio::time::timeout(Duration::from_secs(1), rx.recv()).await`.

- [ ] **Step 8: Run focused runtime tests**

Run:

```bash
cargo test --all-targets --all-features test_error_eventはfile_channel満杯時もmerged_rxに届く
cargo test --all-targets --all-features test_merge_forwarderはfileよりerrorを優先する
```

Expected: PASS.

- [ ] **Step 9: Commit split delivery implementation**

Run:

```bash
git add src/watcher/runtime.rs
git commit -m "fix: watcher異常通知を内部専用チャネルで配送"
```

Expected: commit succeeds with runtime implementation and updated tests.

## Task 4: Finish Shutdown Handling And Regression Verification

**Files:**
- Modify: `src/watcher/runtime.rs`
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Update `WatchRuntime::stop_with_timeout` to wait for merge forwarder**

After `self.watcher_thread.join()` succeeds and before `store_stopped_if_not_failed()`, add:

```rust
        let forwarder_start = std::time::Instant::now();
        loop {
            match self.merge_forwarder_done.recv_timeout(poll_interval) {
                Ok(()) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if forwarder_start.elapsed() > timeout {
                        tracing::warn!(
                            elapsed_ms = forwarder_start.elapsed().as_millis(),
                            timeout_secs = timeout.as_secs(),
                            "[markdown-view] watcher内部イベント統合タスクの停止がタイムアウトしました"
                        );
                        self.merge_forwarder.abort();
                        return self.health_state.load();
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    tracing::warn!(
                        "[markdown-view] watcher内部イベント統合タスクの完了通知チャネルが切断されました"
                    );
                    break;
                }
            }
        }
```

This waits for merge forwarder completion without adding a new dependency and without blocking Tokio runtime workers.

- [ ] **Step 2: Update TODO tracking**

In `docs/todo/TODO.md`, remove the High Priority item:

```markdown
- [ ] watcher の `try_send` で `WatchEvent::Error` を `FileChanged` と同列に破棄しない
```

Add a concise Done Summary entry near the top of `## Done Summary`:

```markdown
- [x] watcher の `try_send` で `WatchEvent::Error` を `FileChanged` と同列に破棄しない
  - 完了根拠: watcher 内部の通常変更通知と異常通知を別 channel に分離し、外部 API は既存の `WatchEvent` receiver に再統合する構成にした。`FileChanged` は満杯時 best-effort で破棄する一方、`Error` は専用 channel 経由で file backlog から独立して配送される。内部 forwarder は error を優先して merged receiver へ流し、既存の server broadcast 契約と watcher health latch を維持した。
```

- [ ] **Step 3: Run targeted regression tests**

Run:

```bash
cargo test --all-targets --all-features test_send_file_changed_eventはチャネル満杯時に破棄してブロックしない
cargo test --all-targets --all-features test_error_eventはfile_channel満杯時もmerged_rxに届く
cargo test --all-targets --all-features test_merge_forwarderはfileよりerrorを優先する
cargo test --all-targets --all-features watcher::runtime::tests::test_notify_error経路はhealth_failedとerror_eventを記録する
cargo test --all-targets --all-features watcher::runtime::tests::test_watcher_panic経路はhealth_failedとerror_eventを記録する
cargo test --all-targets --all-features watcher::runtime::tests::test_send_internal_watch_result_満杯時はhealth_failedとerror_eventを送る
```

Expected: all PASS.

- [ ] **Step 4: Run full verification**

Run:

```bash
./verify.sh
```

Expected: format, clippy, and tests all PASS.

- [ ] **Step 5: Commit final tracking and shutdown refinements**

Run:

```bash
git add src/watcher/runtime.rs docs/todo/TODO.md
git commit -m "docs: watcher異常通知配送issueを完了扱いに更新"
```

Expected: commit succeeds.

## Self-Review Checklist

- Spec coverage: internal file/error channel split, public API preservation, error priority, shutdown ownership, tests, security considerations, and rollback are covered by Tasks 1-4.
- Placeholder scan: no `TBD`, `TODO`, `implement later`, or unspecified test instructions remain.
- Type consistency: public `WatchEvent` remains unchanged; internal split uses `mpsc::Sender<PathBuf>` for file events and `mpsc::Sender<WatchError>` for error events; `Watcher::spawn()` still returns `mpsc::Receiver<WatchEvent>`.
