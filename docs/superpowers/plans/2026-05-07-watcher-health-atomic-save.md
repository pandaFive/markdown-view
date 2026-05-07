# Watcher Health Atomic Save Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** watcher の状態を enum で問い合わせ可能にし、単一ファイルモードとディレクトリモードの atomic save 後 WebSocket 更新を統合テストで固定する。

**Architecture:** `Watcher` が共有 `Arc<AtomicU8>` health state を保持し、`WatcherHealth` / `WatcherFailureKind` へ変換して公開する。`WatchService` は `Watcher` の health を thin wrapper として返す。atomic save テストは既存の WebSocket 統合テストに合わせ、production の notify 経路を通す。

**Tech Stack:** Rust, Tokio, notify-debouncer-mini, axum WebSocket, tokio-tungstenite, tempfile, cargo test

---

## File Structure

- Modify: `src/watcher/runtime.rs`
  - `WatcherHealth` と `WatcherFailureKind` を定義する。
  - watcher thread と service 側で共有する health state を追加する。
  - `Watcher::health()` と `Watcher::is_alive()` を追加する。
  - health 遷移のユニットテストを追加する。
- Modify: `src/server/watch.rs`
  - `WatchService::health()` と `WatchService::is_alive()` を追加する。
  - service wrapper のユニットテストを追加する。
- Modify: `tests/integration_test.rs`
  - atomic save helper を追加する。
  - 単一ファイルモード atomic save WebSocket 更新テストを追加する。
  - ディレクトリモード atomic save WebSocket 更新テストを追加する。
- Reference: `docs/superpowers/specs/2026-05-07-watcher-health-atomic-save-design.md`
  - 実装判断の根拠。変更しない。

---

### Task 1: Watcher Health State

**Files:**
- Modify: `src/watcher/runtime.rs`

- [ ] **Step 1: Write failing unit tests for health transitions**

Add these tests inside `#[cfg(test)] mod tests` in `src/watcher/runtime.rs`.

```rust
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
fn test_watcher_health_threadpanicはaliveではない() {
    let health_state = WatcherHealthState::new_starting();

    health_state.store_failed(WatcherFailureKind::ThreadPanic);

    assert_eq!(
        health_state.load(),
        WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
    );
    assert!(!matches!(health_state.load(), WatcherHealth::Alive));
}

#[test]
fn test_watcher_health_notifyエラーはaliveではない() {
    let health_state = WatcherHealthState::new_starting();

    health_state.store_failed(WatcherFailureKind::Notify);

    assert_eq!(
        health_state.load(),
        WatcherHealth::Failed(WatcherFailureKind::Notify)
    );
    assert!(!matches!(health_state.load(), WatcherHealth::Alive));
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
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test watcher::runtime::tests::test_watcher_health_ --all-features
```

Expected: FAIL because `WatcherHealth`, `WatcherFailureKind`, `WatcherHealthState`, `Watcher::health()`, `Watcher::is_alive()`, and the new `Watcher::new` signature do not exist yet.

- [ ] **Step 3: Add health types and shared state**

In `src/watcher/runtime.rs`, add these definitions near the constants.

```rust
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
    state: Arc<std::sync::atomic::AtomicU8>,
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
            state: Arc::new(std::sync::atomic::AtomicU8::new(Self::STARTING)),
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
            Self::FAILED_THREAD_PANIC => {
                WatcherHealth::Failed(WatcherFailureKind::ThreadPanic)
            }
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
            WatcherHealth::Failed(WatcherFailureKind::ThreadPanic) => {
                Self::FAILED_THREAD_PANIC
            }
            WatcherHealth::Stopping => Self::STOPPING,
            WatcherHealth::Stopped => Self::STOPPED,
        };
        self.state.store(raw, Ordering::Release);
    }

    fn store_failed(&self, kind: WatcherFailureKind) {
        self.store(WatcherHealth::Failed(kind));
    }
}
```

- [ ] **Step 4: Wire health through Watcher runtime**

Update `WatchRuntime`, `Watcher::spawn`, `Watcher::new`, and `WatchRuntime::stop` in `src/watcher/runtime.rs`.

```rust
struct WatchRuntime {
    shutdown_flag: Arc<AtomicBool>,
    watcher_thread: std::thread::JoinHandle<()>,
    health_state: WatcherHealthState,
}
```

```rust
let health_state = WatcherHealthState::new_starting();
let thread_health_state = health_state.clone();
let watcher_thread = spawn_watcher_thread(
    strategy,
    watch_dir,
    tx,
    init_tx,
    thread_shutdown_flag,
    thread_health_state,
)?;

await_watcher_init(init_rx, unexpected_exit).await?;
health_state.store(WatcherHealth::Alive);
Ok((Self::new(shutdown_flag, watcher_thread, health_state), rx))
```

```rust
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
```

```rust
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
```

- [ ] **Step 5: Mark failures from watcher thread**

Update `spawn_watcher_thread` signature and failure points.

```rust
fn spawn_watcher_thread(
    strategy: WatchStrategy,
    watch_dir: PathBuf,
    tx: mpsc::Sender<WatchEvent>,
    init_tx: oneshot::Sender<InitResult>,
    thread_shutdown_flag: Arc<AtomicBool>,
    health_state: WatcherHealthState,
) -> Result<std::thread::JoinHandle<()>> {
```

Inside the notify callback, clone and use a callback health state.

```rust
let callback_health_state = health_state.clone();
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
            callback_health_state.store_failed(WatcherFailureKind::Notify);
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
```

In the panic branch:

```rust
health_state.store_failed(WatcherFailureKind::ThreadPanic);
let watch_error = WatchError::thread_panic(panic_detail.clone());
tracing::error!("[markdown-view] {}: {}", panic_message, panic_detail);
send_watch_event(&panic_tx, WatchEvent::Error(watch_error), error_label);
```

- [ ] **Step 6: Run watcher health tests**

Run:

```bash
cargo test watcher::runtime::tests::test_watcher_health_ --all-features
```

Expected: PASS.

- [ ] **Step 7: Commit Task 1**

```bash
git add src/watcher/runtime.rs
git commit -m "feat: watcher health状態を追加"
```

---

### Task 2: WatchService Health Wrapper

**Files:**
- Modify: `src/server/watch.rs`

- [ ] **Step 1: Write failing WatchService tests**

Add imports in `#[cfg(test)] mod tests`:

```rust
use crate::watcher::WatcherHealth;
```

Add this test:

```rust
#[tokio::test]
async fn test_watch_service_health_開始後はaliveを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("watch-health.md");
    std::fs::write(&file_path, "# watch").unwrap();
    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState::new_with_tokio_memo_fs(
        AppMode::new_single_file(&file_path).unwrap(),
        false,
        None,
        tx,
    ));

    let service = WatchService::start(state).await.unwrap();

    assert_eq!(service.health(), WatcherHealth::Alive);
    assert!(service.is_alive());

    service.shutdown().await;
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cargo test server::watch::tests::test_watch_service_health_開始後はaliveを返す --all-features
```

Expected: FAIL because `WatchService::health()` and `WatchService::is_alive()` do not exist.

- [ ] **Step 3: Add WatchService wrapper methods**

Update imports in `src/server/watch.rs`.

```rust
use crate::watcher::{Watcher, WatcherHealth};
```

Add methods inside `impl WatchService`.

```rust
/// watcher の現在状態を返す
pub fn health(&self) -> WatcherHealth {
    self.watcher
        .as_ref()
        .map(Watcher::health)
        .unwrap_or(WatcherHealth::Stopped)
}

/// watcher が正常稼働中なら true
pub fn is_alive(&self) -> bool {
    matches!(self.health(), WatcherHealth::Alive)
}
```

- [ ] **Step 4: Run WatchService tests**

Run:

```bash
cargo test server::watch::tests::test_watch_service_ --all-features
```

Expected: PASS.

- [ ] **Step 5: Commit Task 2**

```bash
git add src/server/watch.rs
git commit -m "feat: WatchServiceでwatcher healthを公開"
```

---

### Task 3: Single-File Atomic Save WebSocket Test

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Add atomic save helper**

Add this helper near the existing WebSocket watcher tests in `tests/integration_test.rs`.

```rust
async fn atomic_save_markdown_file(path: &std::path::Path, new_content: &str) {
    let swp_path = path.with_extension("md.swp");
    let backup_path = path.with_extension("md~");

    tokio::fs::write(&swp_path, new_content)
        .await
        .expect("atomic save temp file should be written");
    tokio::fs::rename(path, &backup_path)
        .await
        .expect("atomic save backup rename should succeed");
    tokio::fs::rename(&swp_path, path)
        .await
        .expect("atomic save final rename should succeed");
    tokio::fs::remove_file(&backup_path)
        .await
        .expect("atomic save backup should be removed");
}
```

- [ ] **Step 2: Write failing single-file atomic save test**

Add this test after `test_ファイル変更でwebsocket更新`.

```rust
#[tokio::test]
async fn test_単一ファイルモード_atomic_save後にwebsocket更新() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("atomic_single.md");
    tokio::fs::write(&file_path, "# Before").await.unwrap();

    let (state, addr) = setup_single_file_server_from_path(&file_path).await;
    let watch_service = WatchService::start(state.clone()).await.unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    let _initial_message = next_ws_message(&mut read).await;

    atomic_save_markdown_file(&file_path, "# After Atomic Save")
        .await;

    let msg = next_ws_message(&mut read).await;
    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();

    assert!(json["content"]
        .as_str()
        .unwrap()
        .contains("After Atomic Save"));
    assert!(watch_service.is_alive());

    watch_service.shutdown().await;
    drop(tmp_dir);
}
```

- [ ] **Step 3: Run test and verify current behavior**

Run:

```bash
cargo test --test integration_test test_単一ファイルモード_atomic_save後にwebsocket更新 --all-features
```

Expected: PASS if existing watcher already handles inode replacement; FAIL or TIMEOUT if the existing watcher misses the final rename. If it fails, keep the failing output and continue with Step 4.

- [ ] **Step 4: Fix only if the test fails**

If Step 3 fails because single-file mode does not emit an update for the final rename, modify `src/watcher/strategy.rs` so `is_target_file` accepts delete/rename temporary NotFound cases by comparing normalized parent and file name. Use the existing fallback block in `is_target_file` and keep the comparison constrained to the original target parent.

Replace the `Err(e)` branch with this code:

```rust
Err(e) => {
    let log_base: &Path = target_path.parent().unwrap_or_else(|| Path::new(""));
    tracing::warn!(
        "[markdown-view] パス正規化に失敗（ファイル名比較にフォールバック）: {} ({})",
        sanitize_path_for_logging(event_path, log_base),
        e
    );
    if event_path.file_name() != target_path.file_name() {
        return false;
    }
    let Some(event_parent) = event_path.parent() else {
        return false;
    };
    let Some(target_parent) = target_path.parent() else {
        return false;
    };

    normalize_lexical_path(event_parent) == normalize_lexical_path(target_parent)
}
```

- [ ] **Step 5: Run single-file atomic save test**

Run:

```bash
cargo test --test integration_test test_単一ファイルモード_atomic_save後にwebsocket更新 --all-features
```

Expected: PASS.

- [ ] **Step 6: Commit Task 3**

If only the test was added:

```bash
git add tests/integration_test.rs
git commit -m "test: 単一ファイルatomic save更新を固定"
```

If `src/watcher/strategy.rs` also changed:

```bash
git add src/watcher/strategy.rs tests/integration_test.rs
git commit -m "fix: 単一ファイルatomic save更新を検知"
```

---

### Task 4: Directory Atomic Save WebSocket Test

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Write directory atomic save test**

Add this test after `test_ディレクトリモード_websocket更新にfileフィールドが含まれる`.

```rust
#[tokio::test]
async fn test_ディレクトリモード_atomic_save後にwebsocket更新() {
    let (state, addr, tmp_dir) = setup_dir_server().await;
    let watch_service = markdown_view::server::WatchService::start(state.clone())
        .await
        .unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    let initial = tokio::time::timeout(Duration::from_millis(500), read.next()).await;
    assert!(
        initial.is_err(),
        "ディレクトリモードでは更新前に初期WebSocketメッセージを送信しない"
    );

    let file_path = tmp_dir.path().join("README.md");
    atomic_save_markdown_file(&file_path, "# README\n\nAfter Atomic Save")
        .await;

    let msg = next_ws_message(&mut read).await;
    let text = msg
        .into_text()
        .expect("WebSocketメッセージのテキスト変換に失敗");
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSONパースに失敗");

    assert!(json["content"]
        .as_str()
        .unwrap()
        .contains("After Atomic Save"));
    assert_eq!(json["file"].as_str().unwrap(), "README.md");
    assert!(watch_service.is_alive());

    watch_service.shutdown().await;
}
```

- [ ] **Step 2: Run directory atomic save test**

Run:

```bash
cargo test --test integration_test test_ディレクトリモード_atomic_save後にwebsocket更新 --all-features
```

Expected: PASS.

- [ ] **Step 3: Run both atomic save tests together**

Run:

```bash
cargo test --test integration_test atomic_save --all-features
```

Expected: PASS for both atomic save tests.

- [ ] **Step 4: Commit Task 4**

```bash
git add tests/integration_test.rs
git commit -m "test: ディレクトリatomic save更新を固定"
```

---

### Task 5: Final Verification and TODO Update

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Run focused Rust tests**

Run:

```bash
cargo test watcher::runtime::tests::test_watcher_health_ --all-features
cargo test server::watch::tests::test_watch_service_ --all-features
cargo test --test integration_test atomic_save --all-features
```

Expected: all commands PASS.

- [ ] **Step 2: Run full verification**

Run:

```bash
cargo test --all-targets --all-features
./verify.sh
```

Expected: both commands PASS.

- [ ] **Step 3: Update TODO.md completion status**

In `docs/todo/TODO.md`, move the Medium Priority item titled `watcher の WatchEvent::Error 後の健全性 API と atomic save 耐性 E2E を追加する` from `## Medium Priority` to `## Done Summary`.

Use this Done Summary entry:

```markdown
- [x] watcher の `WatchEvent::Error` 後の健全性 API と atomic save 耐性 E2E を追加する
  - 完了根拠: `WatcherHealth` / `WatcherFailureKind` を追加し、`WatchService::health()` と `WatchService::is_alive()` から watcher 状態を内部 API として取得できるようにした。`ThreadPanic` と notify error は `Failed(...)` として分類され、`is_alive()` は `Alive` の場合だけ true を返す。単一ファイルモードとディレクトリモードの atomic save 相当の rename シーケンス後に WebSocket update を受け取る統合テストで固定した。HTTP API、UI、WebSocket エラー JSON の外部契約は増やしていない
```

- [ ] **Step 4: Validate TODO.md diff**

Run:

```bash
rg -n "健全性 API|atomic save|WatcherHealth|WatchService::health" docs/todo/TODO.md
git diff -- docs/todo/TODO.md
```

Expected:
- The unfinished Medium Priority item is gone.
- Done Summary contains the completion entry.
- No unrelated TODO entries changed.

- [ ] **Step 5: Commit TODO update**

```bash
git add docs/todo/TODO.md
git commit -m "docs: watcher健全性TODOを完了へ移動"
```

- [ ] **Step 6: Report completion**

Report:
- Changed files and rough line impact.
- Affected dependent files.
- Verification commands and results.
- Residual risks: notify behavior can vary by OS/filesystem; the tests cover the repository's Linux CI path and local watcher contract, not every editor implementation.

---

## Plan Self-Review

- Spec coverage: `WatcherHealth` / `WatcherFailureKind`, `WatchService::health()`, `is_alive()`, `ThreadPanic`, notify error, single-file atomic save, directory atomic save, no HTTP/UI/API expansion, security considerations, verification, and rollback are all mapped to tasks.
- Placeholder scan: no unresolved placeholders remain.
- Type consistency: `WatcherHealth`, `WatcherFailureKind`, `WatcherHealthState`, `Watcher::health()`, `Watcher::is_alive()`, `WatchService::health()`, and `WatchService::is_alive()` use consistent names across tasks.
