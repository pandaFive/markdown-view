# notify_update Error Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `notify_update` が WebSocket 受信者 0 のときでも、ファイル変更由来の検証・読込前エラーを warn ログに残す。

**Architecture:** 受信者がいる場合の broadcast 経路は変更しない。受信者 0 の場合だけ、本文読込・render/TOC を避ける軽量検査 helper を呼び、検証失敗、metadata/open 失敗、サイズ超過だけをログ化する。

**Tech Stack:** Rust, Tokio, tracing, tracing-test, axum server modules.

---

## File Structure

- Modify: `src/server/files/content.rs`
  - Add a receiver=0 lightweight helper that returns `Option<String>` for loggable file-change errors.
  - Add a small pre-render readability check using `tokio::fs::metadata` and `tokio::fs::File::open`.
  - Extract shared file-label formatting so the existing broadcast message path and new log-only path stay consistent.
- Modify: `src/server/broadcast.rs`
  - Route `receiver_count() == 0` through the new log-only helper.
  - Add tests for receiver=0 error logging, normal fast path, and non-UTF-8 non-observation.

## Task 1: Add Log-Only Error Classification

**Files:**
- Modify: `src/server/files/content.rs`

- [ ] **Step 1: Add the lightweight helper test by compiling against the intended API**

No standalone test is added in this file because the helper is exercised through `notify_update` in Task 2. Before editing, inspect the current functions:

```bash
sed -n '1,240p' src/server/files/content.rs
```

Expected: `build_change_broadcast_message`, `ReadMarkdownError`, and `read_markdown_with_limit` are present.

- [ ] **Step 2: Extract the shared change-error file label**

In `src/server/files/content.rs`, add this helper near `build_change_broadcast_message`:

```rust
fn change_error_file_label(state: &AppState, changed_file: &Path) -> String {
    state
        .mode()
        .single_file()
        .map(file_display_name)
        .unwrap_or_else(|| {
            sanitize_path_for_logging(changed_file, state.mode().base_dir()).into_owned()
        })
}
```

Then replace the inline `file_label` construction in `ValidateRenderOutcome::ResolveFailed` inside `build_change_broadcast_message` with:

```rust
let file_label = change_error_file_label(state, changed_file);
```

- [ ] **Step 3: Add the read-before-render preflight helper**

In `src/server/files/content.rs`, add this async helper after `build_change_broadcast_message`:

```rust
async fn check_readable_before_render(file_path: &Path) -> Result<(), ReadMarkdownError> {
    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(ReadMarkdownError::Io)?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err(ReadMarkdownError::TooLarge);
    }

    let _file = tokio::fs::File::open(file_path)
        .await
        .map_err(ReadMarkdownError::Io)?;
    Ok(())
}
```

This deliberately does not call `read_markdown_with_limit`, `render_markdown`, or `generate_toc`.

- [ ] **Step 4: Add the log-only classifier**

In `src/server/files/content.rs`, add this public-in-server helper after `check_readable_before_render`:

```rust
pub(in crate::server) async fn build_change_error_log_message_without_receivers(
    state: &AppState,
    changed_file: &Path,
) -> Option<String> {
    let resolve_result = resolve_change_target(state, changed_file);
    let target = match resolve_result {
        Ok(Some(target)) => target,
        Ok(None) => return None,
        Err(error) => {
            let file_label = change_error_file_label(state, changed_file);
            return Some(format!("ファイル検証エラー ({}): {}", file_label, error));
        }
    };

    match check_readable_before_render(target.file_path()).await {
        Ok(()) => None,
        Err(ReadMarkdownError::NotUtf8) => None,
        Err(error) => Some(format!(
            "ファイル読み込みエラー ({}): {}",
            target.file_label(),
            error.user_message()
        )),
    }
}
```

- [ ] **Step 5: Run formatting check for the touched file**

Run:

```bash
cargo fmt --all -- --check
```

Expected before formatting: PASS if formatting is already correct, otherwise FAIL with rustfmt diff guidance.

- [ ] **Step 6: Format if needed**

If Step 5 fails, run:

```bash
cargo fmt --all
```

Expected: command exits successfully and formats `content.rs`.

## Task 2: Route receiver=0 Through Log-Only Path

**Files:**
- Modify: `src/server/broadcast.rs`
- Modify: `src/server/files/content.rs`

- [ ] **Step 1: Write failing tests for receiver=0 logging**

In `src/server/broadcast.rs`, add these tests inside the existing `#[cfg(test)] mod tests`:

```rust
    #[traced_test]
    #[tokio::test]
    async fn test_notify_update_受信者ゼロ時の検証エラーはwarnログに残す() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("missing.md", "# missing");
        let rx = state.tx().subscribe();
        drop(rx);

        std::fs::remove_file(&file_path).unwrap();
        notify_update(&state, &file_path).await;

        assert!(logs_contain(
            "WebSocket受信者がいないためファイル変更エラーをローカル記録しました"
        ));
        assert!(logs_contain("ファイル検証エラー"));
        assert!(logs_contain("missing.md"));
    }

    #[traced_test]
    #[tokio::test]
    async fn test_notify_update_受信者ゼロ時のサイズ超過はwarnログに残す() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("large.md", "# large");
        let rx = state.tx().subscribe();
        drop(rx);

        let file = std::fs::File::options()
            .write(true)
            .open(&file_path)
            .unwrap();
        file.set_len(MAX_FILE_SIZE + 1).unwrap();

        notify_update(&state, &file_path).await;

        assert!(logs_contain(
            "WebSocket受信者がいないためファイル変更エラーをローカル記録しました"
        ));
        assert!(logs_contain("ファイル読み込みエラー"));
        assert!(logs_contain("ファイルサイズが上限"));
        assert!(logs_contain("large.md"));
    }
```

- [ ] **Step 2: Write failing tests for receiver=0 fast path boundaries**

In the same test module, add:

```rust
    #[traced_test]
    #[tokio::test]
    async fn test_notify_update_受信者ゼロ時の正常更新はwarnログに残さない() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("normal.md", "# normal");
        let rx = state.tx().subscribe();
        drop(rx);

        notify_update(&state, &file_path).await;

        assert!(!logs_contain(
            "WebSocket受信者がいないためファイル変更エラーをローカル記録しました"
        ));
        assert!(!logs_contain("ファイル読み込みエラー"));
        assert!(!logs_contain("ファイル検証エラー"));
    }

    #[traced_test]
    #[tokio::test]
    async fn test_notify_update_受信者ゼロ時の非utf8は本文読込せずwarnログに残さない() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("binary.md", "# valid");
        let rx = state.tx().subscribe();
        drop(rx);

        std::fs::write(&file_path, b"\xff\xfe\x80\x81").unwrap();
        notify_update(&state, &file_path).await;

        assert!(!logs_contain(
            "WebSocket受信者がいないためファイル変更エラーをローカル記録しました"
        ));
        assert!(!logs_contain("UTF-8"));
    }
```

- [ ] **Step 3: Run the focused tests and confirm they fail**

Run:

```bash
cargo test --lib notify_update_受信者ゼロ時 -- --nocapture
```

Expected: FAIL because `notify_update` still returns before calling the new helper.

- [ ] **Step 4: Import the new helper in broadcast.rs**

At the top of `src/server/broadcast.rs`, change:

```rust
use super::files::build_change_broadcast_message;
```

to:

```rust
use super::files::{
    build_change_broadcast_message, build_change_error_log_message_without_receivers,
};
```

- [ ] **Step 5: Add the log-only branch**

In `src/server/broadcast.rs`, replace `notify_update` with:

```rust
pub async fn notify_update(state: &AppState, changed_file: &Path) {
    if state.tx().receiver_count() == 0 {
        log_change_error_without_receivers(state, changed_file).await;
        return;
    }

    if let Some(message) = build_change_broadcast_message(state, changed_file).await {
        send_broadcast_message(state.tx(), message);
    }
}
```

- [ ] **Step 6: Add the private logger helper**

In `src/server/broadcast.rs`, add this function after `notify_update`:

```rust
async fn log_change_error_without_receivers(state: &AppState, changed_file: &Path) {
    if let Some(message) =
        build_change_error_log_message_without_receivers(state, changed_file).await
    {
        tracing::warn!(
            message = %message,
            "[markdown-view] WebSocket受信者がいないためファイル変更エラーをローカル記録しました"
        );
    }
}
```

- [ ] **Step 7: Run the focused tests and confirm they pass**

Run:

```bash
cargo test --lib notify_update_受信者ゼロ時 -- --nocapture
```

Expected: PASS for the new receiver=0 tests.

## Task 3: Regression and Verification

**Files:**
- Modify: `src/server/broadcast.rs`
- Modify: `src/server/files/content.rs`

- [ ] **Step 1: Run broadcast module tests**

Run:

```bash
cargo test --lib server::broadcast::tests -- --nocapture
```

Expected: PASS. Existing receiver-present Error broadcast behavior remains unchanged.

- [ ] **Step 2: Run server file content tests**

Run:

```bash
cargo test --lib server::files -- --nocapture
```

Expected: PASS. Existing file resolution and content behavior remains unchanged.

- [ ] **Step 3: Run full Rust test suite**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 4: Run required repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS for format, clippy, and tests.

- [ ] **Step 5: Commit implementation**

Run:

```bash
git status --short
git add src/server/broadcast.rs src/server/files/content.rs
git commit -m "fix: notify_updateの受信者なしエラーをログ化"
```

Expected: one focused implementation commit after the existing design/plan commits.

## Self-Review

- Spec coverage: receiver=0 validation/read-before-render errors are logged; normal updates and non-UTF-8 avoid body reads; receiver-present broadcast path is unchanged.
- Security: no Markdown body or rendered HTML is logged; path labels reuse existing sanitized/user-facing formatting.
- Scope: limited to `broadcast.rs` and `content.rs`; no WebSocket JSON, watcher strategy, renderer, or route changes.
