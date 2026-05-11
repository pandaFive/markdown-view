# Watcher Init Panic Result Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** Completed historical implementation plan. This document records the steps used for the completed issue 141 work; do not reapply it to the current branch as a fresh task list. If this plan is reused, first regenerate the steps against the current code and inspect `git status` / `git diff` before staging anything.

**Goal:** watcher thread が初期化完了前に panic した場合、`Watcher::spawn(...).await` の init result として `WatchErrorKind::ThreadPanic` を返す。

**Architecture:** `src/watcher/runtime.rs` の `handle_watcher_panic` を panic detail 生成の単一境界として維持し、`init_tx` が残っている場合だけ `send_init_result(..., Err(WatchError::thread_panic(...)))` を呼ぶ。稼働後 panic では `init_tx` が `None` なので、既存どおり health failed と `WatchEvent::Error` 補助通知だけを維持する。init 前 panic の補助通知は内部 error channel への投入であり、`Watcher::spawn()` が `Err` を返すため公開 receiver や WebSocket/client への配送は保証しない。

**Tech Stack:** Rust, Tokio `oneshot` / `mpsc`, `cargo test`, `cargo clippy`, repository `./verify.sh`

---

## File Structure

- Modify: `src/watcher/runtime.rs`
  - `handle_watcher_panic` の引数に `&mut Option<oneshot::Sender<InitResult>>` を追加する。
  - `spawn_watcher_thread` の panic 捕捉経路から `&mut init_tx` を渡す。
  - 既存の panic handler ユニットテストを新 signature に合わせる。
  - init 前 panic 用の failing test を追加する。
- Modify: `docs/todo/TODO.md`
  - issue 141 の追跡項目がある場合、実装完了状態へ更新する。該当項目が存在しない場合は変更しない。

## Task 1: Init 前 panic の失敗テストを書く

**Files:**
- Modify: `src/watcher/runtime.rs`
- Test: `src/watcher/runtime.rs`

- [x] **Step 1: Add failing unit test**

`test_watcher_panic経路はinit_tx残存時にthread_panicをinit_resultへ返す` を `test_watcher_panic経路はhealth_failedとerror_eventを記録する` の直前に追加する。

```rust
    #[test]
    fn test_watcher_panic経路はinit_tx残存時にthread_panicをinit_resultへ返す() {
        let health_state = WatcherHealthState::new_starting();
        let (error_tx, mut error_rx) =
            priority_error_channel(super::WATCHER_ERROR_MESSAGE_BUFFER);
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
```

- [x] **Step 2: Run targeted test and verify it fails to compile**

Run:

```bash
cargo test --lib watcher::runtime::tests::test_watcher_panic経路はinit_tx残存時にthread_panicをinit_resultへ返す
```

Expected at Task 1 execution time: FAIL to compile because `handle_watcher_panic` still took 5 arguments and the test called it with 6.

- [x] **Step 3: Commit failing test**

Before staging, inspect `git status --short --branch` and `git diff -- src/watcher/runtime.rs`. Stage only the Task 1 test diff, and do not include unrelated edits in the same file.

```bash
git add src/watcher/runtime.rs
git commit -m "test: watcher初期化前panicのinit結果を固定"
```

## Task 2: Panic handler で init result を返す

**Files:**
- Modify: `src/watcher/runtime.rs`
- Test: `src/watcher/runtime.rs`

- [x] **Step 1: Update `spawn_watcher_thread` panic call site**

Change the `handle_watcher_panic` call in `spawn_watcher_thread` to pass `&mut init_tx`.

```rust
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
```

- [x] **Step 2: Update `handle_watcher_panic` signature and implementation**

Replace the function body with this implementation.

```rust
fn handle_watcher_panic(
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
```

- [x] **Step 3: Update existing panic handler tests for post-init behavior**

In `test_watcher_panic経路はhealth_failedとerror_eventを記録する`, add a local `let mut init_tx = None;` and pass `&mut init_tx`.

```rust
        let mut init_tx = None;

        handle_watcher_panic(
            Box::new(String::from("panic detail")),
            "panic message",
            "panic label",
            &health_state,
            &error_tx,
            &mut init_tx,
        );
```

In `test_watcher_panic経路はanyhow_payload_detailを保持する`, add the same `None` init sender and pass it.

```rust
        let mut init_tx = None;

        handle_watcher_panic(
            Box::new(anyhow::anyhow!("anyhow panic detail")),
            "panic message",
            "panic label",
            &health_state,
            &error_tx,
            &mut init_tx,
        );
```

- [x] **Step 4: Run targeted panic handler tests**

Run:

```bash
cargo test --lib watcher::runtime::tests::test_watcher_panic
```

Expected: PASS. The new init result test, the existing health/error event test, and the anyhow detail test all pass.

- [x] **Step 5: Commit implementation**

Before staging, inspect `git status --short --branch` and `git diff -- src/watcher/runtime.rs`. Stage only the Task 2 implementation diff, and do not include unrelated edits in the same file.

```bash
git add src/watcher/runtime.rs
git commit -m "fix: watcher初期化前panicをinit結果に返す"
```

## Task 3: Anyhow panic detail を init result 側にも固定する

**Files:**
- Modify: `src/watcher/runtime.rs`
- Test: `src/watcher/runtime.rs`

- [x] **Step 1: Add init result detail assertion for `anyhow::Error` payload**

Add this test after `test_watcher_panic経路はanyhow_payload_detailを保持する`.

```rust
    #[test]
    fn test_watcher_panic経路はanyhow_payload_detailをinit_resultにも保持する() {
        let health_state = WatcherHealthState::new_starting();
        let (error_tx, mut error_rx) =
            priority_error_channel(super::WATCHER_ERROR_MESSAGE_BUFFER);
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
```

- [x] **Step 2: Run targeted test**

Run:

```bash
cargo test --lib watcher::runtime::tests::test_watcher_panic経路はanyhow_payload_detailをinit_resultにも保持する
```

Expected: PASS.

- [x] **Step 3: Run all watcher runtime unit tests**

Run:

```bash
cargo test --lib watcher::runtime
```

Expected: PASS. No watcher runtime regression.

- [x] **Step 4: Commit regression coverage**

Before staging, inspect `git status --short --branch` and `git diff -- src/watcher/runtime.rs`. Stage only the Task 3 regression test diff, and do not include unrelated edits in the same file.

```bash
git add src/watcher/runtime.rs
git commit -m "test: watcher panic detailのinit返却を固定"
```

## Task 4: TODO 追跡を更新する

**Files:**
- Modify: `docs/todo/TODO.md`

- [x] **Step 1: Search issue 141 tracking entry**

Run:

```bash
rg -n "141|初期化前 panic|init 結果|ThreadPanic" docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected: Any existing tracking line for issue 141 is shown, or no output if the issue is tracked only in GitHub.

- [x] **Step 2: Update only an existing issue 141 TODO entry**

If `docs/todo/TODO.md` contains an active issue 141 item, change that item to completed or remove it according to the surrounding file's existing convention. Do not invent a new TODO entry if no issue 141 item exists.

Example when the file uses checkbox completion:

```markdown
- [x] #141 watcher 初期化前 panic を init 結果として返す
```

Example when the file uses active-only lists:

```markdown
<!-- remove the #141 active item after implementation is verified -->
```

- [x] **Step 3: Validate docs diff**

Run:

```bash
git diff -- docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected: Only issue 141 tracking status changes are present. `docs/todo/BACKLOG.md` is unchanged unless the search shows issue 141 is tracked there.

- [x] **Step 4: Commit tracking update if a file changed**

If `docs/todo/TODO.md` or `docs/todo/BACKLOG.md` changed, inspect `git status --short --branch` and the targeted docs diff first. Stage only the tracking file that actually changed; do not stage unrelated docs edits.

```bash
git add <changed-tracking-file>
git commit -m "docs: issue141の追跡状態を更新"
```

If no tracking entry exists, skip this commit and record that in the final report.

## Task 5: Full verification

**Files:**
- No source edits expected.

- [x] **Step 1: Run format check**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS.

- [x] **Step 2: Run clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [x] **Step 3: Run full tests**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [x] **Step 4: Run repository verification script**

Run:

```bash
./verify.sh
```

Expected: PASS. If this repeats the previous checks, still run it because repository policy names it as the required completion gate.

- [x] **Step 5: Inspect final diff**

Run:

```bash
git status --short --branch
git diff --stat origin/develop...HEAD
git log --oneline origin/develop..HEAD
```

Expected: Branch is not `develop` or `main`. Commits include the spec commit, plan commit, test/implementation commits, and optional TODO tracking commit. Diff is limited to `src/watcher/runtime.rs`, `docs/superpowers/specs/2026-05-11-watcher-init-panic-result-design.md`, `docs/superpowers/plans/2026-05-11-watcher-init-panic-result.md`, and optional `docs/todo/TODO.md`.

## Security Notes

- Panic detail remains untrusted diagnostic text. Do not execute it or parse it as shell, SQL, HTML, or policy.
- This plan must not change path validation, Host/Origin checks, CSP, HTML sanitization, file-size limits, or traversal protections.
- Error details continue to flow through typed `WatchError` and existing JSON broadcast behavior only.

## Rollback

Revert the implementation commit `fix: watcher初期化前panicをinit結果に返す` and related test commits to return to the old behavior where init前 panic is only visible through health/error event. The spec and plan commits can remain as historical planning docs, or be reverted together if the whole issue branch is abandoned.
