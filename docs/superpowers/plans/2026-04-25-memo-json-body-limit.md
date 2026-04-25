# Memo JSON Body Limit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Document and test the two-layer `/api/memo` size contract: raw memo content is limited to 10MB, while JSON request bodies allow escape overhead up to the existing transport limit.

**Architecture:** Keep the runtime behavior unchanged. Add a precise comment beside the private router body-limit constant, then add HTTP integration tests that distinguish save-layer rejection from axum body-limit rejection.

**Tech Stack:** Rust, axum `DefaultBodyLimit`, reqwest integration tests, Tokio, serde_json.

---

## File Structure

- Modify `src/server/routes.rs`: add a Japanese comment above `MEMO_JSON_BODY_LIMIT`; do not change the constant value or route layering.
- Modify `tests/integration_test.rs`: strengthen `/api/memo` PUT boundary coverage in the existing single-file memo API test group.
- Modify `docs/todo/TODO.md`: mark the High Priority memo body-limit item complete after verification.

## Task 1: Add Transport Body-Limit Regression Test

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Write the failing test**

Add this test after `test_apiメモ_jsonエスケープで膨らんでも上限内rawなら保存できる` and before `test_apiメモ_10mb超過は413で拒否する`.

```rust
#[tokio::test]
async fn test_apiメモ_jsonボディ制限超過は413で拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let escaped_raw = "\\\\".repeat(markdown_view::server::MAX_FILE_SIZE as usize);
    let padding = " ".repeat(4096 + 128);
    let body = format!("{{\"raw\":\"{}\"}}{}", escaped_raw, padding);

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
}
```

This builds a JSON payload whose parsed `raw` value would be exactly `MAX_FILE_SIZE` backslashes, then adds legal trailing JSON whitespace so the HTTP body exceeds the transport limit before serde deserialization.

- [ ] **Step 2: Run the new test to verify current behavior**

Run:

```bash
cargo test --test integration_test test_apiメモ_jsonボディ制限超過は413で拒否する
```

Expected: PASS. This task captures current behavior as a regression test; it may already pass because `DefaultBodyLimit` is already installed.

- [ ] **Step 3: Commit the test**

```bash
git add tests/integration_test.rs
git commit -m "test: メモAPIのJSONボディ制限境界を追加"
```

## Task 2: Clarify Raw-Limit Boundary Test

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Update the existing raw-size rejection test**

In `test_apiメモ_10mb超過は413で拒否する`, replace the hard-coded `10 * 1024 * 1024` expression with `markdown_view::server::MAX_FILE_SIZE` so the test follows the production constant.

```rust
#[tokio::test]
async fn test_apiメモ_10mb超過は413で拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let raw = "a".repeat((markdown_view::server::MAX_FILE_SIZE as usize) + 1);

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": raw
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["error"], "メモサイズが上限（10MB）を超えています");
}
```

- [ ] **Step 2: Run the updated raw-limit test**

Run:

```bash
cargo test --test integration_test test_apiメモ_10mb超過は413で拒否する
```

Expected: PASS with the existing application-layer JSON error message.

- [ ] **Step 3: Commit the test cleanup**

```bash
git add tests/integration_test.rs
git commit -m "test: メモAPIのrawサイズ境界を定数基準にする"
```

## Task 3: Document the Body-Limit Contract

**Files:**
- Modify: `src/server/routes.rs`

- [ ] **Step 1: Add the Japanese comment**

Replace:

```rust
const MEMO_JSON_BODY_LIMIT: usize = (MAX_FILE_SIZE as usize * 2) + 4096;
```

with:

```rust
// メモ本文の保存上限は save_route_memo 側の MAX_FILE_SIZE で判定する。
// ここは JSON envelope と string escape を含む HTTP body の上限。
// backslash や quote が多い本文は JSON 上で約 2 倍に膨らむため、
// 10MB 以下の合法メモを transport 層で誤拒否しない余白を持たせる。
const MEMO_JSON_BODY_LIMIT: usize = (MAX_FILE_SIZE as usize * 2) + 4096;
```

- [ ] **Step 2: Run route tests and memo boundary tests**

Run:

```bash
cargo test --test integration_test test_apiメモ_jsonエスケープで膨らんでも上限内rawなら保存できる
cargo test --test integration_test test_apiメモ_jsonボディ制限超過は413で拒否する
cargo test --test integration_test test_apiメモ_10mb超過は413で拒否する
```

Expected: all selected tests PASS.

- [ ] **Step 3: Commit the comment**

```bash
git add src/server/routes.rs
git commit -m "docs: メモAPIボディ制限の意図を明文化"
```

## Task 4: Close the TODO Item

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Mark the High Priority item complete**

In `docs/todo/TODO.md`, change:

```markdown
- [ ] メモ API のボディ制限値を意図明文化し、境界テストを追加
```

to:

```markdown
- [x] メモ API のボディ制限値を意図明文化し、境界テストを追加
```

Keep the existing detail bullets under that item for audit context.

- [ ] **Step 2: Commit docs bookkeeping**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: メモAPIボディ制限TODOを完了"
```

## Task 5: Final Verification

**Files:**
- No code edits.

- [ ] **Step 1: Run full verification**

Run:

```bash
./verify.sh
```

Expected: format, clippy, and all tests PASS.

- [ ] **Step 2: Inspect final diff**

Run:

```bash
git status --short
git log --oneline -5
```

Expected: working tree clean, recent commits include the test, comment, and docs bookkeeping commits.

- [ ] **Step 3: Report completion**

Include:

- Changed files and reason
- Affected dependent files
- Verification results
- Residual risks, especially that the body-limit rejection body is axum-generated and the test only asserts HTTP status
