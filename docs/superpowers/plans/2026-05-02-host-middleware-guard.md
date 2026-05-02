# Host Middleware Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move Host validation from per-handler calls into a shared axum middleware applied by `create_router()`.

**Architecture:** Reuse the existing `ensure_allowed_request_host` policy from `src/server/guards.rs` inside a new middleware function. Apply that middleware to all routes in `src/server/routes.rs`; keep WebSocket Origin validation in `ws_handler()` so Host and Origin remain separate security checks.

**Tech Stack:** Rust 2021, axum 0.8 middleware, tokio integration tests, reqwest, tokio-tungstenite, existing `./verify.sh`.

---

## File Structure

- Modify: `src/server/guards.rs`
  - Add `require_allowed_request_host(request, next)` middleware.
  - Add a focused async test proving the middleware rejects untrusted Host and passes trusted Host.
- Modify: `src/server/routes.rs`
  - Apply `middleware::from_fn(require_allowed_request_host)` in `create_router()`.
  - Remove per-handler Host checks.
  - Leave `ws_handler()` responsible for Origin validation only.
- Modify: `tests/integration_test.rs`
  - Broaden route-level Host rejection coverage and assert rejection responses keep security headers.
  - Keep existing WebSocket Origin rejection tests.
- Modify: `docs/todo/TODO.md`
  - Mark the Host middleware item complete after implementation and verification.

## Task 1: Middleware Red Test

**Files:**
- Modify: `src/server/guards.rs`

- [ ] **Step 1: Add imports for the middleware test**

In `src/server/guards.rs`, inside the existing `#[cfg(test)] mod tests`, extend the imports from:

```rust
use axum::http::header::{HOST, ORIGIN};
use axum::http::HeaderMap;
```

to:

```rust
use axum::http::header::{HOST, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::{middleware, routing::get, Router};
```

- [ ] **Step 2: Write the failing middleware test**

Append this test near the other Host tests in `src/server/guards.rs`:

```rust
#[tokio::test]
async fn test_host_middlewareは不正hostを拒否して許可hostを通す() {
    let app = Router::new()
        .route("/probe", get(|| async { "ok" }))
        .layer(middleware::from_fn(require_allowed_request_host));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());
    let rejected = client
        .get(format!("http://{}/probe", addr))
        .header("Host", &attack_host)
        .send()
        .await
        .unwrap();

    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    let json: serde_json::Value = rejected.json().await.unwrap();
    assert!(json["error"].as_str().is_some());

    let allowed = client
        .get(format!("http://{}/probe", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(allowed.text().await.unwrap(), "ok");
}
```

- [ ] **Step 3: Run the focused red test**

Run:

```bash
cargo test --all-targets --all-features server::guards::tests::test_host_middlewareは不正hostを拒否して許可hostを通す
```

Expected: FAIL to compile with an error equivalent to:

```text
cannot find value `require_allowed_request_host` in this scope
```

- [ ] **Step 4: Commit the red test**

```bash
git add src/server/guards.rs
git commit -m "test: Host検証middlewareの境界を追加"
```

## Task 2: Middleware Implementation and Router Application

**Files:**
- Modify: `src/server/guards.rs`
- Modify: `src/server/routes.rs`

- [ ] **Step 1: Add middleware imports in `guards.rs`**

In `src/server/guards.rs`, change the axum imports from:

```rust
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::Json;
```

to:

```rust
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
```

- [ ] **Step 2: Add the middleware function**

Insert this function after `ensure_allowed_request_host` in `src/server/guards.rs`:

```rust
/// Host 検証を全 route へ適用する axum middleware。
pub(super) async fn require_allowed_request_host(request: Request, next: Next) -> Response {
    if let Err(error) = ensure_allowed_request_host(request.headers()) {
        return error.into_response();
    }

    next.run(request).await
}
```

- [ ] **Step 3: Run the middleware unit test**

Run:

```bash
cargo test --all-targets --all-features server::guards::tests::test_host_middlewareは不正hostを拒否して許可hostを通す
```

Expected: PASS.

- [ ] **Step 4: Add route middleware imports in `routes.rs`**

In `src/server/routes.rs`, change:

```rust
use axum::extract::{DefaultBodyLimit, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
```

to:

```rust
use axum::extract::{DefaultBodyLimit, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware;
```

Then change the guards import from:

```rust
use super::guards::{
    build_csp_header, ensure_allowed_request_host, is_allowed_ws_origin, json_error,
};
```

to:

```rust
use super::guards::{build_csp_header, is_allowed_ws_origin, json_error, require_allowed_request_host};
```

If rustfmt splits the import over multiple lines, accept rustfmt's output.

- [ ] **Step 5: Apply Host middleware in `create_router()`**

In `src/server/routes.rs`, add the middleware layer to the route stack. The intended shape is:

```rust
        .layer(middleware::from_fn(require_allowed_request_host))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
```

Keep the existing CSP layer after the frame options layer. This order is chosen so Host rejection responses are still processed by the response header layers.

- [ ] **Step 6: Run existing Host rejection tests before removing handler calls**

Run:

```bash
cargo test --all-targets --all-features 不正host
```

Expected: PASS. This filter runs the existing invalid Host tests for memo, search, files, and WebSocket rebinding. At this point Host validation is duplicated by middleware and handlers.

- [ ] **Step 7: Commit middleware addition**

```bash
git add src/server/guards.rs src/server/routes.rs
git commit -m "feat: Host検証middlewareを追加"
```

## Task 3: Remove Handler-Level Host Checks and Strengthen Integration Coverage

**Files:**
- Modify: `src/server/routes.rs`
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Remove Host checks from HTTP handlers**

In `src/server/routes.rs`, delete these lines from `index_handler`, `api_content_handler`, `api_memo_handler`, `api_memo_save_handler`, `api_files_handler`, and `api_search_handler`:

```rust
    ensure_allowed_request_host(&headers)?;
```

Then remove the now-unused `headers: HeaderMap,` argument from those six HTTP handlers. Keep query and JSON extractor arguments unchanged.

- [ ] **Step 2: Simplify `ws_handler()` to Origin-only validation**

Replace the current `ws_handler()` body:

```rust
    // HOST 経路の拒否を監査ログに残すため ensure_allowed_request_host を使う。
    // `||` の短絡評価により、HOST 拒否時は is_allowed_ws_origin (内部で HOST を
    // 再チェックする) が走らず、重複ログを防ぐ。
    if ensure_allowed_request_host(&headers).is_err() || !is_allowed_ws_origin(&headers) {
        return json_error(StatusCode::FORBIDDEN, "WebSocket接続元が許可されていません")
            .into_response();
    }
```

with:

```rust
    if !is_allowed_ws_origin(&headers) {
        return json_error(StatusCode::FORBIDDEN, "WebSocket接続元が許可されていません")
            .into_response();
    }
```

Keep `headers: HeaderMap` on `ws_handler()` because Origin validation still needs it.

- [ ] **Step 3: Remove unused imports**

After the handler cleanup, ensure `src/server/routes.rs` no longer imports `ensure_allowed_request_host`. Keep these imports:

```rust
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware;
```

`HeaderMap` remains needed by `ws_handler()`.

- [ ] **Step 4: Broaden the route-level Host rejection integration test**

Replace `test_httpは許可されないhostを拒否する` in `tests/integration_test.rs` with:

```rust
#[tokio::test]
async fn test_host_middlewareは全http_routeの不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Host Check").await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());

    for path in ["/", "/api/content", "/api/memo", "/api/search?q=test"] {
        let resp = client
            .get(format!("http://{}{}", addr, path))
            .header("Host", &attack_host)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
        assert_eq!(
            resp.headers()
                .get(reqwest::header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|value| value.to_str().ok()),
            Some("nosniff")
        );
        assert_eq!(
            resp.headers()
                .get(reqwest::header::X_FRAME_OPTIONS)
                .and_then(|value| value.to_str().ok()),
            Some("DENY")
        );
        assert!(
            resp.headers()
                .get(reqwest::header::CONTENT_SECURITY_POLICY)
                .and_then(|value| value.to_str().ok())
                .is_some()
        );
        let json: serde_json::Value = resp.json().await.unwrap();
        assert!(json["error"].as_str().is_some());
    }
}
```

- [ ] **Step 5: Add WebSocket invalid Host integration coverage**

Add this test next to the existing WebSocket Origin rejection tests in `tests/integration_test.rs`:

```rust
#[tokio::test]
async fn test_websocketはhost_middlewareで不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Host Test").await;

    let url = format!("ws://{}/ws", addr);
    let attack_host = format!("evil.example:{}", addr.port());
    let allowed_origin = format!("http://{}", addr);
    let result = connect_ws_with_host(&url, &allowed_origin, Some(&attack_host)).await;

    assert!(result.is_err());
}
```

- [ ] **Step 6: Run focused integration tests**

Run:

```bash
cargo test --all-targets --all-features host
cargo test --all-targets --all-features origin
```

Expected: PASS. The `host` filter covers the new middleware tests and existing invalid Host tests. The `origin` filter covers existing WebSocket Origin rejection tests. If the HTTP rejection test fails only because one of `nosniff`, `DENY`, or CSP is missing, change the middleware/header layer order in `create_router()` so rejected Host responses pass through the response header layers, then rerun both commands.

- [ ] **Step 7: Commit handler cleanup and integration coverage**

```bash
git add src/server/routes.rs tests/integration_test.rs
git commit -m "refactor: Host検証をrouter middlewareへ集約"
```

## Task 4: Documentation, Full Verification, and Final Commit

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Mark the TODO item complete**

In `docs/todo/TODO.md`, change:

```markdown
- [ ] Host 検証を router middleware 化して新規 route の守り忘れを防ぐ
```

to:

```markdown
- [x] Host 検証を router middleware 化して新規 route の守り忘れを防ぐ
```

Keep the existing description under that item unless implementation details require a small factual update.

- [ ] **Step 2: Run format check**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS. If it fails with formatting diffs, run `cargo fmt --all`, then rerun the check.

- [ ] **Step 3: Run clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Run all tests**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 5: Run repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 6: Commit TODO completion**

```bash
git add docs/todo/TODO.md
git commit -m "docs: Host検証middleware化TODOを完了"
```

- [ ] **Step 7: Inspect final diff and history**

Run:

```bash
git status --short
git log --oneline -4
```

Expected:

```text
git status --short
```

prints no modified files. `git log --oneline -4` includes the three implementation commits plus the existing design commit.

## Self-Review

- Spec coverage: The plan covers middleware creation, router application, HTTP handler cleanup, WebSocket Origin preservation, Host rejection tests, security header checks, verification, and TODO completion.
- Placeholder scan: No placeholder requirement remains; every code-changing step includes concrete code or exact deletion instructions.
- Type consistency: The middleware uses axum 0.8 `Request`, `Next`, and `Response`; `ApiError` is converted with `IntoResponse`; `HeaderMap` remains only where `ws_handler()` needs Origin validation.
