# Host Middleware Structure Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Host middleware の route 横断契約を型で表現し、WebSocket 後段で Host 系拒否に到達した場合を bypass 兆候として観測可能にする。

**Architecture:** `build_routes()` は route 定義専用の private newtype `RouteDefinitions` を返し、`create_router()` だけが Host middleware と security header layer を適用する。WebSocket Origin 検証では Host 系 `WsOriginRejection` を bypass indicator として `error!` に分類する。既存の Host / Origin 信頼判定仕様は変更しない。

**Tech Stack:** Rust, axum, tower-http, tracing, reqwest integration tests, cargo test, `./verify.sh`

---

## File Structure

- Modify: `tests/integration_test.rs`
  - WebSocket 不正 Host 拒否レスポンスにも security headers が付くことを固定する。
- Modify: `src/server/routes.rs`
  - `RouteDefinitions` private newtype を追加し、route 定義と security layer 適用を分離する。
- Modify: `src/server/guards.rs`
  - Host 系 `WsOriginRejection` を bypass indicator として分類する helper を追加し、`is_allowed_ws_origin()` のログレベルを分ける。

## Task 1: WebSocket Host 拒否レスポンスの security headers をテストで固定する

**Files:**
- Modify: `tests/integration_test.rs`

- [x] **Step 1: Write the failing test update**

`tests/integration_test.rs` の `test_websocketはhost_middlewareで不正hostを拒否する` 内で、素の status assertion を既存 helper に置き換える。

Replace:

```rust
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["error"], "許可されていないHostヘッダーです");
```

With:

```rust
    assert_forbidden_with_security_headers(&resp);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["error"], "許可されていないHostヘッダーです");
```

- [x] **Step 2: Run the targeted test**

Run:

```bash
cargo test --test integration_test test_websocketはhost_middlewareで不正hostを拒否する
```

Expected: PASS on current code if `/ws` Host middleware rejection already receives security headers. If it fails, the failure should show a missing `x-content-type-options`, `x-frame-options`, or `content-security-policy` header and Task 2 must preserve/fix layer ordering.

- [x] **Step 3: Commit the test lock**

Run:

```bash
git add tests/integration_test.rs
git commit -m "test: WS Host拒否のsecurity headersを固定"
```

Expected: commit succeeds with only `tests/integration_test.rs` staged.

## Task 2: route 定義専用 newtype で Host middleware 適用境界を型化する

**Files:**
- Modify: `src/server/routes.rs`

- [x] **Step 1: Change `build_routes()` to return `RouteDefinitions`**

In `src/server/routes.rs`, add this private newtype near `MEMO_JSON_BODY_LIMIT`:

```rust
/// Host middleware 適用前の route 定義だけを保持する。
///
/// 裸の `Router` と区別することで、route 定義と共通 security layer 適用を
/// `create_router` 側へ集約する契約を型で表現する。
struct RouteDefinitions(Router<Arc<AppState>>);
```

Then replace the current `create_router()` and `build_routes()` definitions with:

```rust
/// axumルーターを構築する
pub fn create_router(state: Arc<AppState>) -> Router {
    let csp_header = build_csp_header(state.syntax_css());
    let RouteDefinitions(routes) = build_routes();

    routes
        // `Router::layer` は呼び出し時点で存在する route にだけ適用される。
        // 新規 route は必ず build_routes() 内へ追加し、ここより後ろへ
        // `.route(...)` を足して Host middleware を完全に bypass させないこと。
        .layer(middleware::from_fn(require_allowed_request_host))
        // Host 拒否の 403 JSON にも security headers を付与するため、
        // 後から追加した response header layer が拒否 response も処理する順に置く。
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        // CSP: scriptはハッシュベース許可を維持し、unsafe-inlineを排除する。
        // styleはsyntect class-basedハイライトを使用し、unsafe-inlineを許可しない。
        // img-srcは同一オリジンのみに制限し、Markdown経由の外部画像読込を既定拒否する。
        // renderer モジュール側でリンクと画像に個別のURLポリシーを適用する。
        // frame-ancestors 'none'でクリックジャッキングを防止。
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            csp_header,
        ))
        .with_state(state)
}

/// Host middleware 適用前の route 定義だけを集約する。
///
/// ここでは route 登録だけを行い、共通 `.layer(...)` は追加しない。
/// 共通 security layer は `create_router` 側で route 群全体へ適用する。
fn build_routes() -> RouteDefinitions {
    RouteDefinitions(
        Router::new()
            .route("/", get(index_handler))
            .route("/ws", get(ws_handler))
            .route("/api/content", get(api_content_handler))
            .route("/api/search", get(api_search_handler))
            .route(
                "/api/memo",
                get(api_memo_handler)
                    .put(api_memo_save_handler)
                    .layer(DefaultBodyLimit::max(MEMO_JSON_BODY_LIMIT)),
            )
            .route("/api/files", get(api_files_handler)),
    )
}
```

- [x] **Step 2: Run focused compile/test checks**

Run:

```bash
cargo test --test integration_test test_host_middlewareは全http_routeの不正hostを拒否する
```

Expected: PASS.

Run:

```bash
cargo test --test integration_test test_websocketはhost_middlewareで不正hostを拒否する
```

Expected: PASS.

- [x] **Step 3: Commit route boundary change**

Run:

```bash
git add src/server/routes.rs
git commit -m "refactor: Host middleware適用境界を型化"
```

Expected: commit succeeds with only `src/server/routes.rs` staged.

## Task 3: WebSocket Host 系 rejection を bypass indicator としてログ分類する

**Files:**
- Modify: `src/server/guards.rs`

- [x] **Step 1: Add the classification helper and unit test**

In `src/server/guards.rs`, add this helper after the `WsOriginRejection` enum:

```rust
fn is_host_middleware_bypass_indicator(rejection: WsOriginRejection) -> bool {
    matches!(
        rejection,
        WsOriginRejection::MissingHost
            | WsOriginRejection::HostMalformed
            | WsOriginRejection::UntrustedHost
    )
}
```

In the existing `#[cfg(test)] mod tests` in the same file, add:

```rust
    #[test]
    fn test_ws_host系拒否はmiddleware_bypass兆候として分類する() {
        assert!(is_host_middleware_bypass_indicator(
            WsOriginRejection::MissingHost
        ));
        assert!(is_host_middleware_bypass_indicator(
            WsOriginRejection::HostMalformed
        ));
        assert!(is_host_middleware_bypass_indicator(
            WsOriginRejection::UntrustedHost
        ));

        assert!(!is_host_middleware_bypass_indicator(
            WsOriginRejection::MissingOrigin
        ));
        assert!(!is_host_middleware_bypass_indicator(
            WsOriginRejection::OriginMalformed
        ));
        assert!(!is_host_middleware_bypass_indicator(
            WsOriginRejection::OriginParseError
        ));
        assert!(!is_host_middleware_bypass_indicator(
            WsOriginRejection::UnsupportedScheme
        ));
        assert!(!is_host_middleware_bypass_indicator(
            WsOriginRejection::OriginMissingAuthority
        ));
        assert!(!is_host_middleware_bypass_indicator(
            WsOriginRejection::UntrustedOriginAuthority
        ));
        assert!(!is_host_middleware_bypass_indicator(
            WsOriginRejection::AuthorityMismatch
        ));
    }
```

- [x] **Step 2: Run helper test before logging change**

Run:

```bash
cargo test server::guards::tests::test_ws_host系拒否はmiddleware_bypass兆候として分類する
```

Expected: PASS after adding the helper and test.

- [x] **Step 3: Update `is_allowed_ws_origin()` log classification**

Replace the `match rejection { ... }` block inside `is_allowed_ws_origin()` with:

```rust
            if is_host_middleware_bypass_indicator(rejection) {
                tracing::error!(
                    "[markdown-view] WS Host middleware bypass 兆候 ({:?}): host={:?} origin={:?}",
                    rejection,
                    host,
                    origin
                );
            } else {
                match rejection {
                    WsOriginRejection::MissingOrigin => {
                        tracing::info!(
                            "[markdown-view] WS Origin 拒否 ({:?}): host={:?} origin={:?}",
                            rejection,
                            host,
                            origin
                        );
                    }
                    _ => {
                        tracing::warn!(
                            "[markdown-view] WS Origin 拒否 ({:?}): host={:?} origin={:?}",
                            rejection,
                            host,
                            origin
                        );
                    }
                }
            }
```

This intentionally moves `MissingHost` from info to error because Host middleware should reject that request before `ws_handler`.

- [x] **Step 4: Run guards tests**

Run:

```bash
cargo test server::guards
```

Expected: PASS. Existing `check_ws_origin()` tests should still return the same `WsOriginRejection` variants.

- [x] **Step 5: Commit logging classification**

Run:

```bash
git add src/server/guards.rs
git commit -m "fix: WS Host bypass兆候をerrorログ化"
```

Expected: commit succeeds with only `src/server/guards.rs` staged.

## Task 4: Full verification and completion update

**Files:**
- Verify: `src/server/routes.rs`
- Verify: `src/server/guards.rs`
- Verify: `tests/integration_test.rs`
- Modify: `docs/todo/TODO.md`

- [x] **Step 1: Run full Rust test suite**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [x] **Step 2: Run required repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [x] **Step 3: Mark the TODO item complete**

In `docs/todo/TODO.md`, change the Medium Priority item for `Host middleware の構造契約と WebSocket bypass 観測性を強化する` to the completed form:

```markdown
- [x] Host middleware の構造契約と WebSocket bypass 観測性を強化する
```

Keep the existing description below it so the completion context remains reviewable.

- [x] **Step 4: Run docs sanity checks**

Run:

```bash
rg -n "Host middleware の構造契約|WS Host middleware bypass|RouteDefinitions" docs/todo/TODO.md src/server/routes.rs src/server/guards.rs tests/integration_test.rs
```

Expected: output includes the completed TODO item, the `RouteDefinitions` newtype, and the bypass log message.

- [x] **Step 5: Commit TODO completion**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: Host middleware構造契約TODOを完了"
```

Expected: commit succeeds with only `docs/todo/TODO.md` staged.

- [x] **Step 6: Report completion**

Final report must include:

- Changed files with reason and rough line impact.
- Affected dependent files.
- `cargo test --all-targets --all-features` result.
- `./verify.sh` result.
- Residual risks, especially whether logging classification is verified by behavior rather than log capture.
- Security considerations: Host / Origin trust policy unchanged, DNS Rebinding boundary remains middleware-wide, external header values remain untrusted.

## Self-Review

- Spec coverage: route newtype is Task 2, WebSocket Host bypass observability is Task 3, `/ws` security header regression test is Task 1, full verification and TODO completion are Task 4.
- Placeholder scan: no deferred-work marker or unspecified test instruction remains. `TODO` appears only as the existing `docs/todo/TODO.md` file name and the target checklist item.
- Type consistency: `RouteDefinitions`, `WsOriginRejection`, `is_host_middleware_bypass_indicator`, `create_router`, and `build_routes` names match the planned code snippets.
- Scope check: the plan touches only routing, guard logging, integration tests, and TODO status. It does not change Host / Origin trust rules, UI, CSP content, or WebSocket message formats.
