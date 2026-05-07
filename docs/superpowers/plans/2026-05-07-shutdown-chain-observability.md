# Shutdown Chain Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** watcher shutdown と forwarder shutdown の timeout 診断ログを統合し、abort 直前と自然終了時に最後のイベント種別と receiver 数を確認できるようにする。

**Architecture:** `broadcast.rs` に forwarder 専用 diagnostics と handle を追加し、`WatchService` は `JoinHandle<()>` ではなく diagnostics 付き handle を保持する。shutdown timeout 秒数は watcher runtime から `pub(crate)` に再公開し、watcher thread と forwarder task が同じ定数を参照する。

**Tech Stack:** Rust, Tokio, tracing, tracing-test, notify watcher, tokio broadcast/mpsc.

---

## File Structure

- Modify: `src/watcher/runtime.rs`
  - `SHUTDOWN_TIMEOUT_SECS` を `pub(crate) const WATCH_SHUTDOWN_TIMEOUT_SECS: u64 = 2` に改名する。
  - watcher thread timeout ログで `elapsed_ms` と `timeout_secs` を structured field として出す。
- Modify: `src/watcher/mod.rs`
  - `WATCH_SHUTDOWN_TIMEOUT_SECS` を crate 内へ再公開する。
- Modify: `src/server/broadcast.rs`
  - `WatchForwarderEventKind`, `WatchForwarderDiagnostics`, `WatchForwarderSnapshot`, `WatchForwarderHandle` を追加する。
  - `spawn_watch_event_forwarder` が `WatchForwarderHandle` を返すようにする。
  - forwarder がイベント処理前に `last_event_kind` を更新する。
  - forwarder 自然終了ログに `last_event_kind` と `receiver_count` を含める。
- Modify: `src/server/watch.rs`
  - `WatchService` が `WatchForwarderHandle` を保持する。
  - `shutdown_watch_forwarder` が diagnostics snapshot を使って timeout / abort 直前ログを出す。
- Tests: `src/server/broadcast.rs`, `src/server/watch.rs`
  - diagnostics の状態遷移、自然終了ログ、timeout ログを固定する。

## Task 1: 共通 timeout 定数を watcher から再公開する

**Files:**
- Modify: `src/watcher/runtime.rs`
- Modify: `src/watcher/mod.rs`

- [ ] **Step 1: `runtime.rs` の定数名を変更する**

Change near the current shutdown timeout constant:

```rust
/// shutdown() のグレースフル停止待機秒数
pub(crate) const WATCH_SHUTDOWN_TIMEOUT_SECS: u64 = 2;
```

Replace both `SHUTDOWN_TIMEOUT_SECS` references inside `WatchRuntime::stop` with `WATCH_SHUTDOWN_TIMEOUT_SECS`.

- [ ] **Step 2: watcher timeout ログに structured fields を追加する**

Replace the timeout branch in `WatchRuntime::stop` with:

```rust
if start.elapsed() > Duration::from_secs(WATCH_SHUTDOWN_TIMEOUT_SECS) {
    let elapsed_ms = start.elapsed().as_millis();
    tracing::warn!(
        elapsed_ms,
        timeout_secs = WATCH_SHUTDOWN_TIMEOUT_SECS,
        "[markdown-view] 監視スレッドの停止がタイムアウトしました"
    );
    return self.health_state.load();
}
```

This keeps behavior unchanged and makes watcher thread logs use the same field names as the forwarder logs.

- [ ] **Step 3: `mod.rs` から crate 内へ再公開する**

Change the runtime re-export block in `src/watcher/mod.rs` to:

```rust
pub(crate) use self::runtime::WATCH_SHUTDOWN_TIMEOUT_SECS;
pub use self::runtime::{Watcher, WatcherFailureKind, WatcherHealth};
```

- [ ] **Step 4: targeted compile を実行する**

Run:

```bash
cargo test --lib watcher:: --all-features
```

Expected: compile succeeds and watcher module tests pass.

- [ ] **Step 5: commit**

Run:

```bash
git add src/watcher/runtime.rs src/watcher/mod.rs
git commit -m "refactor: watcher停止timeout定数を共通化"
```

## Task 2: forwarder diagnostics 型を追加する

**Files:**
- Modify: `src/server/broadcast.rs`

- [ ] **Step 1: diagnostics の failing tests を追加する**

Inside `#[cfg(test)] mod tests` in `src/server/broadcast.rs`, add:

```rust
#[test]
fn test_watch_forwarder_diagnostics_初期状態は最後のイベントなし() {
    let (tx, rx1) = broadcast::channel(4);
    let rx2 = tx.subscribe();
    let rx3 = tx.subscribe();
    let diagnostics = WatchForwarderDiagnostics::new(tx);

    let snapshot = diagnostics.snapshot();
    drop((rx1, rx2, rx3));

    assert_eq!(snapshot.last_event_kind, None);
    assert_eq!(snapshot.receiver_count, 3);
}

#[test]
fn test_watch_forwarder_diagnostics_filechangedを記録できる() {
    let (tx, _rx) = broadcast::channel(4);
    let diagnostics = WatchForwarderDiagnostics::new(tx);

    diagnostics.record(WatchForwarderEventKind::FileChanged);
    let snapshot = diagnostics.snapshot();

    assert_eq!(
        snapshot.last_event_kind,
        Some(WatchForwarderEventKind::FileChanged)
    );
    assert_eq!(snapshot.receiver_count, 1);
}

#[test]
fn test_watch_forwarder_diagnostics_errorを記録できる() {
    let (tx, _rx) = broadcast::channel(4);
    let diagnostics = WatchForwarderDiagnostics::new(tx);

    diagnostics.record(WatchForwarderEventKind::Error);
    let snapshot = diagnostics.snapshot();

    assert_eq!(
        snapshot.last_event_kind,
        Some(WatchForwarderEventKind::Error)
    );
    assert_eq!(snapshot.receiver_count, 1);
}
```

- [ ] **Step 2: tests が失敗することを確認する**

Run:

```bash
cargo test --lib server::broadcast::tests::test_watch_forwarder_diagnostics_ --all-features
```

Expected: FAIL with unresolved types such as `WatchForwarderDiagnostics` or no matching test filter depending on Rust test filtering. The important signal is that the new tests do not pass before implementation.

- [ ] **Step 3: diagnostics 型を実装する**

At the top of `src/server/broadcast.rs`, change imports to include atomics and `JoinHandle`:

```rust
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
```

Add these types before `notify_update`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WatchForwarderEventKind {
    FileChanged,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WatchForwarderSnapshot {
    pub last_event_kind: Option<WatchForwarderEventKind>,
    pub receiver_count: usize,
}

#[derive(Debug, Clone)]
pub(super) struct WatchForwarderDiagnostics {
    last_event_kind: Arc<AtomicU8>,
    tx: tokio::sync::broadcast::Sender<BroadcastMessage>,
}

impl WatchForwarderDiagnostics {
    const NONE: u8 = 0;
    const FILE_CHANGED: u8 = 1;
    const ERROR: u8 = 2;

    pub(super) fn new(tx: tokio::sync::broadcast::Sender<BroadcastMessage>) -> Self {
        Self {
            last_event_kind: Arc::new(AtomicU8::new(Self::NONE)),
            tx,
        }
    }

    pub(super) fn record(&self, kind: WatchForwarderEventKind) {
        let raw = match kind {
            WatchForwarderEventKind::FileChanged => Self::FILE_CHANGED,
            WatchForwarderEventKind::Error => Self::ERROR,
        };
        self.last_event_kind.store(raw, Ordering::Release);
    }

    pub(super) fn snapshot(&self) -> WatchForwarderSnapshot {
        let last_event_kind = match self.last_event_kind.load(Ordering::Acquire) {
            Self::NONE => None,
            Self::FILE_CHANGED => Some(WatchForwarderEventKind::FileChanged),
            Self::ERROR => Some(WatchForwarderEventKind::Error),
            invalid => {
                debug_assert!(false, "不正なforwarder event kind: {}", invalid);
                None
            }
        };
        WatchForwarderSnapshot {
            last_event_kind,
            receiver_count: self.tx.receiver_count(),
        }
    }
}

pub(super) struct WatchForwarderHandle {
    pub(super) task: JoinHandle<()>,
    pub(super) diagnostics: WatchForwarderDiagnostics,
}
```

- [ ] **Step 4: diagnostics tests が通ることを確認する**

Run:

```bash
cargo test --lib server::broadcast::tests::test_watch_forwarder_diagnostics --all-features
```

Expected: PASS for the three diagnostics tests.

- [ ] **Step 5: commit**

Run:

```bash
git add src/server/broadcast.rs
git commit -m "test: forwarder診断状態を追加"
```

## Task 3: forwarder handle と自然終了ログを実装する

**Files:**
- Modify: `src/server/broadcast.rs`

- [ ] **Step 1: 自然終了ログの failing test を追加する**

Inside `#[cfg(test)] mod tests` in `src/server/broadcast.rs`, add:

```rust
#[traced_test]
#[tokio::test]
async fn test_watch_forwarder_自然終了ログに診断情報を含める() {
    let base_dir = tempfile::tempdir().unwrap();
    std::fs::write(base_dir.path().join("README.md"), "# before").unwrap();
    let state = Arc::new(create_directory_state(base_dir.path()));
    let (tx, rx) = mpsc::channel(4);
    let handle = spawn_watch_event_forwarder(state, rx);

    tx.send(WatchEvent::Error(WatchError::notify("forwarder-log-test")))
        .await
        .unwrap();
    drop(tx);

    handle.task.await.unwrap();

    assert!(logs_contain("ファイル変更通知タスクが終了しました"));
    assert!(logs_contain("last_event_kind=Some(Error)"));
    assert!(logs_contain("receiver_count=0"));
}
```

- [ ] **Step 2: test が失敗することを確認する**

Run:

```bash
cargo test --lib server::broadcast::tests::test_watch_forwarder_自然終了ログに診断情報を含める --all-features
```

Expected: FAIL because `spawn_watch_event_forwarder` still returns `JoinHandle<()>` or the log does not contain diagnostics.

- [ ] **Step 3: `spawn_watch_event_forwarder` の返り値を handle に変更する**

Replace the function signature and body with:

```rust
pub(super) fn spawn_watch_event_forwarder(
    state: Arc<AppState>,
    mut rx: mpsc::Receiver<WatchEvent>,
) -> WatchForwarderHandle {
    let diagnostics = WatchForwarderDiagnostics::new(state.tx().clone());
    let task_diagnostics = diagnostics.clone();
    let task = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                WatchEvent::FileChanged(changed_path) => {
                    task_diagnostics.record(WatchForwarderEventKind::FileChanged);
                    notify_update(&state, &changed_path).await;
                }
                WatchEvent::Error(error) => {
                    task_diagnostics.record(WatchForwarderEventKind::Error);
                    broadcast_error(&state, &error);
                }
            }
        }
        let snapshot = task_diagnostics.snapshot();
        tracing::info!(
            last_event_kind = ?snapshot.last_event_kind,
            receiver_count = snapshot.receiver_count,
            "[markdown-view] ファイル変更通知タスクが終了しました"
        );
    });
    WatchForwarderHandle { task, diagnostics }
}
```

- [ ] **Step 4: existing tests の handle await を修正する**

Search:

```bash
rg -n "spawn_watch_event_forwarder|\\.await\\.unwrap\\(\\)" src/server/broadcast.rs
```

For tests that previously did:

```rust
let handle = spawn_watch_event_forwarder(state, rx);
handle.await.unwrap();
```

Change them to:

```rust
let handle = spawn_watch_event_forwarder(state, rx);
handle.task.await.unwrap();
```

Do not change production behavior other than the return wrapper and diagnostics logging.

- [ ] **Step 5: broadcast tests を実行する**

Run:

```bash
cargo test --lib server::broadcast --all-features
```

Expected: PASS.

- [ ] **Step 6: commit**

Run:

```bash
git add src/server/broadcast.rs
git commit -m "feat: forwarder終了ログに診断情報を追加"
```

## Task 4: WatchService shutdown timeout ログへ diagnostics を接続する

**Files:**
- Modify: `src/server/watch.rs`

- [ ] **Step 1: timeout ログの failing test を追加する**

Inside `#[cfg(test)] mod tests` in `src/server/watch.rs`, add imports:

```rust
use tokio::sync::broadcast;
use tokio::task;
use tracing_test::traced_test;

use super::{shutdown_watch_forwarder, WatchService};
use super::super::broadcast::{
    WatchForwarderDiagnostics, WatchForwarderEventKind, WatchForwarderHandle,
};
```

Replace the existing `use super::WatchService;` line with the combined `use super::{shutdown_watch_forwarder, WatchService};` line.

Then add this test:

```rust
#[traced_test]
#[tokio::test]
async fn test_shutdown_watch_forwarder_timeoutログに診断情報を含める() {
    let (tx, _rx) = broadcast::channel(4);
    let diagnostics = WatchForwarderDiagnostics::new(tx);
    diagnostics.record(WatchForwarderEventKind::FileChanged);
    let handle = WatchForwarderHandle {
        task: task::spawn(async {
            std::future::pending::<()>().await;
        }),
        diagnostics,
    };

    shutdown_watch_forwarder(handle).await;

    assert!(logs_contain("監視イベント転送タスク停止がタイムアウトしたためabortします"));
    assert!(logs_contain("elapsed_ms="));
    assert!(logs_contain("timeout_secs=2"));
    assert!(logs_contain("last_event_kind=Some(FileChanged)"));
    assert!(logs_contain("receiver_count=1"));
}
```

- [ ] **Step 2: test が失敗することを確認する**

Run:

```bash
cargo test --lib server::watch::tests::test_shutdown_watch_forwarder_timeoutログに診断情報を含める --all-features
```

Expected: FAIL because `WatchService` and `shutdown_watch_forwarder` still use `JoinHandle<()>`.

- [ ] **Step 3: imports と `WatchService` field を変更する**

At the top of `src/server/watch.rs`, replace imports with:

```rust
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;

use super::broadcast::{spawn_watch_event_forwarder, WatchForwarderHandle};
use super::state::AppState;
use crate::watcher::{Watcher, WatcherHealth, WATCH_SHUTDOWN_TIMEOUT_SECS};
```

Change `WatchService` to:

```rust
pub struct WatchService {
    watcher: Option<Watcher>,
    watch_forwarder: Option<WatchForwarderHandle>,
}
```

Remove the local `WATCH_FORWARDER_SHUTDOWN_TIMEOUT_SECS` constant.

- [ ] **Step 4: `shutdown_watch_forwarder` を diagnostics 対応に変更する**

Replace `shutdown_watch_forwarder` with:

```rust
async fn shutdown_watch_forwarder(watch_forwarder: WatchForwarderHandle) {
    let WatchForwarderHandle {
        mut task,
        diagnostics,
    } = watch_forwarder;
    let start = Instant::now();
    match tokio::time::timeout(
        Duration::from_secs(WATCH_SHUTDOWN_TIMEOUT_SECS),
        &mut task,
    )
    .await
    {
        Ok(join_result) => {
            if let Err(e) = join_result {
                tracing::warn!(
                    "[markdown-view] 監視イベント転送タスクの終了待機に失敗: {}",
                    e
                );
            }
        }
        Err(_) => {
            let elapsed_ms = start.elapsed().as_millis();
            let snapshot = diagnostics.snapshot();
            tracing::warn!(
                elapsed_ms,
                timeout_secs = WATCH_SHUTDOWN_TIMEOUT_SECS,
                last_event_kind = ?snapshot.last_event_kind,
                receiver_count = snapshot.receiver_count,
                "[markdown-view] 監視イベント転送タスク停止がタイムアウトしたためabortします"
            );
            task.abort();
            if let Err(e) = task.await {
                if e.is_cancelled() {
                    return;
                }
                tracing::warn!(
                    "[markdown-view] 監視イベント転送タスクの終了待機に失敗: {}",
                    e
                );
            }
        }
    }
}
```

- [ ] **Step 5: `broadcast.rs` の可視性を確認する**

Before running the watch tests, confirm the diagnostics types and handle fields in `src/server/broadcast.rs` are visible to sibling server modules exactly as follows:

```rust
pub(super) enum WatchForwarderEventKind { /* variants unchanged */ }
pub(super) struct WatchForwarderSnapshot { /* fields unchanged */ }
pub(super) struct WatchForwarderDiagnostics { /* fields private */ }
pub(super) struct WatchForwarderHandle {
    pub(super) task: JoinHandle<()>,
    pub(super) diagnostics: WatchForwarderDiagnostics,
}
```

Keep the atomic field inside `WatchForwarderDiagnostics` private.

- [ ] **Step 6: watch tests を実行する**

Run:

```bash
cargo test --lib server::watch --all-features
```

Expected: PASS.

- [ ] **Step 7: commit**

Run:

```bash
git add src/server/watch.rs src/server/broadcast.rs
git commit -m "feat: forwarder停止ログに診断情報を追加"
```

## Task 5: 全体検証と TODO 更新

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: formatting / lint / tests を実行する**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Expected: all PASS.

- [ ] **Step 2: full verification を実行する**

Run:

```bash
./verify.sh
```

Expected: all PASS.

- [ ] **Step 3: TODO の対象項目を Done Summary に移す**

In `docs/todo/TODO.md`, remove the Medium item:

```markdown
- [ ] shutdown チェーンの観測性を統合する
```

Add this entry near the top of `Done Summary`:

```markdown
- [x] shutdown チェーンの観測性を統合する
  - 完了根拠: watcher thread と forwarder task の shutdown timeout 秒数を共通化し、forwarder の最後のイベント種別と WebSocket receiver 数を診断 snapshot として記録する構成にした。forwarder の自然終了ログと timeout / abort 直前ログに `last_event_kind` / `receiver_count` / `elapsed_ms` / `timeout_secs` を含め、停止遅延時にログだけで切り分けられるようにした。HTTP API、WebSocket payload、UI 表示の外部契約は変更していない
```

- [ ] **Step 4: TODO validation を実行する**

Run:

```bash
rg -n "shutdown チェーンの観測性を統合する" docs/todo/TODO.md
```

Expected: one Done Summary match only, no unchecked Medium item remains.

- [ ] **Step 5: final verification を再実行する**

Run:

```bash
./verify.sh
```

Expected: all PASS after TODO update.

- [ ] **Step 6: commit**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: shutdownチェーン観測性TODOを完了"
```

## Security Notes

- 新しい shutdown 診断ログには、イベント種別、経過時間、timeout 秒数、receiver 数だけを含める。
- ファイルパス、notify error detail、ユーザー入力由来文字列を新しい diagnostics snapshot へ保存しない。
- `WatchEvent::Error` の本文は既存の broadcast error 経路だけで扱い、diagnostics では `Error` という enum 種別だけを保持する。
- Host/Origin 検証、CSP、HTML sanitization、canonical path 検証、base 配下検証には触れない。

## Rollback

Rollback by reverting the implementation commits from this plan:

```bash
git revert <commit-for-task-5> <commit-for-task-4> <commit-for-task-3> <commit-for-task-2> <commit-for-task-1>
```

External HTTP/WebSocket/UI contracts are unchanged, so rollback should not require client migration.
