# Host Middleware Observability Follow-up Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the Host middleware follow-up by adding path-aware Host rejection logging, fixing WS Origin rejection message coverage, improving test server failure context, and moving the TODO item to Done Summary.

**Architecture:** Keep the existing `create_router()` and Host middleware layer structure unchanged. Add observability only at the middleware rejection boundary, add integration coverage for the existing WS Origin rejection response, and keep docs cleanup separate from runtime behavior changes.

**Tech Stack:** Rust, axum middleware, tracing, reqwest integration tests, Tokio tests, Markdown project docs.

---

## File Structure

- Modify: `src/server/guards.rs`
  - Responsibility: Host/Origin validation, JSON error creation, and audit logging.
  - Planned changes: add path-aware Host rejection logging without changing Host validation semantics.
- Modify: `tests/integration/security.rs`
  - Responsibility: integration coverage for Host/Origin security boundaries and security headers.
  - Planned changes: assert WS Origin rejection response body using the existing request style in this file.
- Modify: `tests/integration/support.rs`
  - Responsibility: shared integration test state/server/WebSocket helpers.
  - Planned changes: replace `axum::serve(...).unwrap()` in `spawn_test_server` with a context-rich `expect(...)`.
- Modify: `docs/todo/TODO.md`
  - Responsibility: active High/Medium work queue and Done Summary.
  - Planned changes: move the Host middleware follow-up item from Medium Priority into Done Summary with completion evidence.

## Work Estimate

- Human effort estimate: 1 to 1.5 hours, including targeted tests and full verification.
- Codex/AI-assisted estimate: 20 to 35 minutes, assuming tests do not expose unrelated failures.

## Task 1: Add Host Rejection Path Logging

**Files:**
- Modify: `src/server/guards.rs`

- [ ] **Step 1: Write the failing unit test**

Add this test inside `#[cfg(test)] mod tests` in `src/server/guards.rs`, near the existing request Host guard tests:

```rust
#[test]
#[traced_test]
fn test_request_host_guard拒否ログはpathを含みqueryを含めない() {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "evil.example:3000".parse().unwrap());

    let error = ensure_allowed_request_host_with_path(&headers, Some("/api/search")).unwrap_err();

    assert_eq!(error.0, StatusCode::FORBIDDEN);
    assert_eq!(error.1["error"], "許可されていないHostヘッダーです");
    assert!(logs_contain("path=\"/api/search\""));
    assert!(!logs_contain("q=secret"));
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
cargo test --lib server::guards::tests::test_request_host_guard拒否ログはpathを含みqueryを含めない --all-features
```

Expected: FAIL because `ensure_allowed_request_host_with_path` is not defined yet.

- [ ] **Step 3: Add the path-aware Host rejection helper**

In `src/server/guards.rs`, replace `ensure_allowed_request_host` with this pair of functions:

```rust
/// 許可されたHostヘッダーのみ受け付け、拒否時は監査向けwarnログを残す。
pub(super) fn ensure_allowed_request_host(headers: &HeaderMap) -> Result<(), ApiError> {
    ensure_allowed_request_host_with_path(headers, None)
}

fn ensure_allowed_request_host_with_path(
    headers: &HeaderMap,
    request_path: Option<&str>,
) -> Result<(), ApiError> {
    if is_allowed_request_host(headers) {
        Ok(())
    } else {
        let host = log_value_for_header(headers, &HOST);
        match request_path {
            Some(path) => {
                tracing::warn!(
                    host = ?host,
                    request_path = path,
                    "[markdown-view] 許可されていないHostヘッダーを拒否: host={:?} path={:?}",
                    host,
                    path
                );
            }
            None => {
                tracing::warn!(
                    host = ?host,
                    "[markdown-view] 許可されていないHostヘッダーを拒否: {:?}",
                    host
                );
            }
        }
        Err(json_error(
            StatusCode::FORBIDDEN,
            "許可されていないHostヘッダーです",
        ))
    }
}
```

Then update `require_allowed_request_host` in the same file:

```rust
pub(super) async fn require_allowed_request_host(request: Request, next: Next) -> Response {
    if let Err(error) =
        ensure_allowed_request_host_with_path(request.headers(), Some(request.uri().path()))
    {
        return error.into_response();
    }

    next.run(request).await
}
```

- [ ] **Step 4: Run the focused test and verify it passes**

Run:

```bash
cargo test --lib server::guards::tests::test_request_host_guard拒否ログはpathを含みqueryを含めない --all-features
```

Expected: PASS.

- [ ] **Step 5: Run all guards tests**

Run:

```bash
cargo test --lib server::guards::tests --all-features
```

Expected: PASS.

- [ ] **Step 6: Commit Task 1**

```bash
git add src/server/guards.rs
git commit -m "fix: Host拒否ログにrequest pathを追加"
```

## Task 2: Assert WS Origin Rejection Message

**Files:**
- Modify: `tests/integration/security.rs`

- [ ] **Step 1: Replace the existing broad Origin rejection test**

In `tests/integration/security.rs`, replace the current `test_websocketは異なるoriginを拒否する` body with this exact version:

```rust
#[tokio::test]
async fn test_websocketは異なるoriginをorigin拒否messageで拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;
    let client = reqwest::Client::new();
    let allowed_host = format!("127.0.0.1:{}", addr.port());

    let resp = client
        .get(format!("http://{}/ws", addr))
        .header("Host", &allowed_host)
        .header("Origin", "https://evil.example")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap();

    assert_forbidden_with_security_headers(&resp);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["error"], "WebSocket接続元が許可されていません");
}
```

- [ ] **Step 2: Run the focused integration test**

Run:

```bash
cargo test --test integration_test test_websocketは異なるoriginをorigin拒否messageで拒否する --all-features
```

Expected: PASS. This behavior already exists; the task fixes missing assertion coverage.

- [ ] **Step 3: Run security integration tests**

Run:

```bash
cargo test --test integration_test security:: --all-features
```

Expected: PASS.

- [ ] **Step 4: Commit Task 2**

```bash
git add tests/integration/security.rs
git commit -m "test: WS Origin拒否messageを固定"
```

## Task 3: Improve Test Server Failure Context

**Files:**
- Modify: `tests/integration/support.rs`

- [ ] **Step 1: Update the shared server helper**

In `tests/integration/support.rs`, replace `spawn_test_server` with:

```rust
pub(super) async fn spawn_test_server(state: Arc<AppState>) -> std::net::SocketAddr {
    let router = markdown_view::server::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("integration test server should bind to an ephemeral loopback port");
    let addr = listener
        .local_addr()
        .expect("integration test server should expose local_addr after bind");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("integration test server should run until test shutdown");
    });
    addr
}
```

- [ ] **Step 2: Run a representative integration test using the helper**

Run:

```bash
cargo test --test integration_test test_host_middlewareは主要routeの不正hostを拒否しsecurity_headerを維持する --all-features
```

Expected: PASS.

- [ ] **Step 3: Commit Task 3**

```bash
git add tests/integration/support.rs
git commit -m "test: integration server helperの失敗文脈を明確化"
```

## Task 4: Move the TODO Item to Done Summary

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Edit the TODO document**

In `docs/todo/TODO.md`, remove this unchecked item from `## Medium Priority`:

```markdown
- [ ] Host middleware 化後の低優先 follow-up を整理して追加検証する
  - ファイル: `src/server/routes.rs`, `src/server/guards.rs`, `tests/integration_test.rs`, `docs/superpowers/specs/2026-05-02-host-middleware-guard-design.md`
  - 現状: PR #120 で Host 検証を router middleware へ集約し、主要 route の不正 Host 拒否、security headers、WS Host/Origin 経路の分離、大容量 PUT body の順序を固定した。一方、許可 Host の全 route smoke、malformed/missing/empty Host の middleware 統合テスト、WS Origin 拒否の error message assert、middleware warn ログへの URI path 追加、test helper 内 `axum::serve(...).unwrap()` の panic 観測性、CHANGELOG 相当の運用ドキュメント化は未対応
  - 対応: 追加する価値が高い順に、許可 Host 明示ループ、malformed/missing/empty Host の middleware 経路 403、WS Origin 拒否 message assert、warn ログへの `request.uri().path()` 追加を検討する。`axum::serve(...).unwrap()` は test helper の失敗文脈が分かる `expect(...)` へ寄せる。WS Host 拒否 message 変更は PR 本文には明記済みなので、必要になった時点で README か CHANGELOG 相当へ移す
  - 昇格理由: Host security boundary の検証網を厚くするが、主要 middleware 化は実装済みなので Medium とする
  - 由来: PR #120 再レビュー follow-up (2026-05-02)
```

Add this entry at the top of `## Done Summary`:

```markdown
- [x] Host middleware 化後の低優先 follow-up を整理して追加検証する
  - 完了根拠: 主要 route の不正 Host 拒否、許可 Host smoke、空 Host 拒否、巨大 body 付き memo PUT の body limit 前拒否を統合テストで固定済み。追加で WS Origin 拒否が Host 拒否とは別の `WebSocket接続元が許可されていません` message を返すことを固定し、Host middleware 拒否ログには query string を含めず request path を出すようにした。共通 integration test server helper の `axum::serve(...).unwrap()` は失敗文脈付き `expect(...)` に寄せた。Host/Origin 許可条件、security headers、CSP、route layer 構造は変更していない。
```

- [ ] **Step 2: Validate the TODO cleanup text**

Run:

```bash
rg -n "Host middleware 化後|request path|WebSocket接続元|CSP/syntax_theme_css" docs/todo/TODO.md
```

Expected:

- `Host middleware 化後` appears only in Done Summary.
- `CSP/syntax_theme_css` remains in Medium Priority.
- The Done Summary entry mentions `request path` and `WebSocket接続元`.

- [ ] **Step 3: Commit Task 4**

```bash
git add docs/todo/TODO.md
git commit -m "docs: Host middleware follow-upを完了扱いに整理"
```

## Task 5: Final Verification

**Files:**
- No new edits expected.

- [ ] **Step 1: Run formatting**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS.

- [ ] **Step 2: Run targeted integration verification**

Run:

```bash
cargo test --test integration_test --all-features
```

Expected: PASS.

- [ ] **Step 3: Run full project verification**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 4: Inspect final diff and history**

Run:

```bash
git status --short --branch
git log --oneline -5
```

Expected:

- Working tree is clean.
- Recent commits include the four task commits after the design commit.

- [ ] **Step 5: Report completion**

Report:

- Changed files and rough line impact.
- Affected dependent files: `src/server/routes.rs` and the existing Host middleware design specs were referenced but not modified.
- Verification results for `cargo fmt`, `cargo test --test integration_test --all-features`, and `./verify.sh`.
- Residual risk: Host/Origin validation semantics were intentionally not changed; remaining Medium TODO should be only the CSP/syntax fallback item.
