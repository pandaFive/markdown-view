# Issue 145 Security Layer Boundary Implementation Plan

> **For agentic workers:** This non-authoritative implementation record is guidance, not policy. Follow the current user instruction, `AGENTS.md`, active skills/hooks, and approval requirements before using any commands below. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Host/security layer の適用境界を helper 化し、不正 Host smoke test を case-driven に整理して route 追加時の検証漏れを減らす。

**Architecture:** `src/server/routes.rs` で route 定義、security layer 適用、state 注入の責務を分ける。`apply_security_layers()` に Host middleware と security response headers を集約し、`create_router()` は構築フローだけを表す。integration test は route 種別ごとの case table で不正 Host と security headers を検証する。

**Tech Stack:** Rust, axum, tower-http `SetResponseHeaderLayer`, reqwest, tokio integration tests.

---

## File Structure

- Modify: `src/server/routes.rs`
  - `apply_security_layers(RouteDefinitions, HeaderValue) -> Router<Arc<AppState>>` を追加する。
  - `create_router()` を `build_routes()`、`apply_security_layers()`、`.with_state(state)` の流れへ整理する。
- Modify: `tests/integration/security.rs`
  - 既存の個別 Host 拒否テストを case-driven helper に寄せる。
  - `HOST_SMOKE_CASES` を唯一の Host/security smoke list として扱う。
  - HTTP GET、memo PUT、WebSocket upgrade の不正 Host rejection と security headers を同じ assertion に通す。
- Existing helper: `tests/integration/support.rs`
  - 変更しない。`setup_single_file_server` など既存 helper をそのまま使う。

## Task 1: Case-Driven Host Rejection Integration Test

**Files:**
- Modify: `tests/integration/security.rs:1-150`

- [ ] **Step 1: Replace the duplicated Host rejection tests with a case-driven failing test**

Replace lines 1-150 of `tests/integration/security.rs` with this code. Keep the rest of the file after `test_websocketはrebind相当のhost_origin一致を拒否する` unchanged.

```rust
use super::support::{
    connect_ws, connect_ws_with_host, setup_dir_server, setup_single_file_server,
};

enum HostSmokeRequest {
    Get(&'static str),
    MemoPut,
    WebSocketUpgrade,
}

struct HostSmokeCase {
    name: &'static str,
    request: HostSmokeRequest,
}

const HOST_SMOKE_CASES: &[HostSmokeCase] = &[
    // 新規 route を追加した場合は、Host/security smoke 対象としてここへ追加する。
    HostSmokeCase {
        name: "index",
        request: HostSmokeRequest::Get("/"),
    },
    HostSmokeCase {
        name: "content",
        request: HostSmokeRequest::Get("/api/content"),
    },
    HostSmokeCase {
        name: "memo_get",
        request: HostSmokeRequest::Get("/api/memo"),
    },
    HostSmokeCase {
        name: "files",
        request: HostSmokeRequest::Get("/api/files"),
    },
    HostSmokeCase {
        name: "search",
        request: HostSmokeRequest::Get("/api/search?q=test"),
    },
    HostSmokeCase {
        name: "memo_put",
        request: HostSmokeRequest::MemoPut,
    },
    HostSmokeCase {
        name: "websocket",
        request: HostSmokeRequest::WebSocketUpgrade,
    },
];

#[tokio::test]
async fn test_host_middlewareは主要routeの不正hostを拒否しsecurity_headerを維持する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Host Check").await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());

    for case in HOST_SMOKE_CASES {
        let resp = send_host_smoke_request(&client, addr, &attack_host, case)
            .await
            .unwrap_or_else(|error| panic!("{} のrequest送信に失敗: {error}", case.name));

        assert_forbidden_with_security_headers(&resp);
        let json: serde_json::Value = resp
            .json()
            .await
            .unwrap_or_else(|error| panic!("{} のJSON応答解析に失敗: {error}", case.name));
        assert_eq!(
            json["error"], "許可されていないHostヘッダーです",
            "{} のHost拒否メッセージが不正",
            case.name
        );
    }
}

async fn send_host_smoke_request(
    client: &reqwest::Client,
    addr: std::net::SocketAddr,
    attack_host: &str,
    case: &HostSmokeCase,
) -> reqwest::Result<reqwest::Response> {
    match &case.request {
        HostSmokeRequest::Get(path) => {
            client
                .get(format!("http://{}{}", addr, path))
                .header("Host", attack_host)
                .send()
                .await
        }
        HostSmokeRequest::MemoPut => {
            client
                .put(format!("http://{}/api/memo", addr))
                .header("Host", attack_host)
                .json(&serde_json::json!({
                    "raw": "blocked memo"
                }))
                .send()
                .await
        }
        HostSmokeRequest::WebSocketUpgrade => {
            client
                .get(format!("http://{}/ws", addr))
                .header("Host", attack_host)
                .header("Origin", format!("http://{}", addr))
                .header("Connection", "Upgrade")
                .header("Upgrade", "websocket")
                .header("Sec-WebSocket-Version", "13")
                .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
                .send()
                .await
        }
    }
}

#[tokio::test]
async fn test_host_middlewareは巨大body付きmemo_putもbody_limit前に不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Host Memo PUT").await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());
    let oversized_raw = "x".repeat(21 * 1024 * 1024);
    let body = serde_json::json!({
        "raw": oversized_raw
    })
    .to_string();

    let resp = client
        .put(format!("http://{}/api/memo", addr))
        .header("Host", &attack_host)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .unwrap();

    assert_forbidden_with_security_headers(&resp);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["error"], "許可されていないHostヘッダーです");
}

fn assert_forbidden_with_security_headers(resp: &reqwest::Response) {
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
    assert!(resp
        .headers()
        .get(reqwest::header::CONTENT_SECURITY_POLICY)
        .and_then(|value| value.to_str().ok())
        .is_some());
}

#[tokio::test]
async fn test_websocketは異なるoriginを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;

    let url = format!("ws://{}/ws", addr);
    let result = connect_ws(&url, "https://evil.example").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_websocketはoriginポート不一致を拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;

    let url = format!("ws://{}/ws", addr);
    let wrong_port_origin = format!("http://localhost:{}", addr.port() + 1);
    let result = connect_ws(&url, &wrong_port_origin).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_websocketはrebind相当のhost_origin一致を拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;

    let url = format!("ws://{}/ws", addr);
    let rebinding_authority = format!("evil.example:{}", addr.port());
    let rebinding_origin = format!("http://{}", rebinding_authority);
    let result = connect_ws_with_host(&url, &rebinding_origin, Some(&rebinding_authority)).await;
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run the focused integration test**

Run:

```bash
cargo test --test integration_test test_host_middleware -- --nocapture
```

Expected: PASS. This step may pass before production changes because it is a refactor of existing coverage. If it fails, the failure should be a compile error from the replacement block or a concrete Host/security regression.

- [ ] **Step 3: Commit the test refactor after approval**

Example commands after approval:

```bash
git add tests/integration/security.rs
git commit -m "test: Host拒否smoke testをcase table化"
```

## Task 2: Security Layer Helper

**Files:**
- Modify: `src/server/routes.rs:33-90`

- [ ] **Step 1: Implement the security layer helper**

Replace `create_router()` and add the helper definitions near `RouteDefinitions`.

```rust
/// Host middleware 適用前の route 定義だけを保持する。
///
/// 裸の `Router` と区別することで、route 定義と共通 security layer 適用を
/// `create_router` 側へ集約する契約を型で表現する。
struct RouteDefinitions(Router<Arc<AppState>>);

/// axumルーターを構築する
pub fn create_router(state: Arc<AppState>) -> Router {
    let csp_header = build_csp_header(state.syntax_css());
    let routes = build_routes();

    apply_security_layers(routes, csp_header).with_state(state)
}

fn apply_security_layers(
    route_definitions: RouteDefinitions,
    csp_header: HeaderValue,
) -> Router<Arc<AppState>> {
    let RouteDefinitions(routes) = route_definitions;

    routes
        // `Router::layer` は呼び出し時点で存在する route にだけ適用される。
        // 新規 route は必ず build_routes() 内へ追加し、security layer 適用後へ
        // `.route(...)` を足して Host middleware を bypass させないこと。
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
}
```

- [ ] **Step 2: Run the routes unit tests**

Run:

```bash
cargo test --lib server::routes
```

Expected: PASS.

- [ ] **Step 3: Run the integration smoke test**

Run:

```bash
cargo test --test integration_test test_host_middleware -- --nocapture
```

Expected: PASS. If `assert_forbidden_with_security_headers` fails, inspect the order inside `apply_security_layers()` before changing assertions.

- [ ] **Step 4: Commit the router helper refactor after approval**

Example commands after approval:

```bash
git add src/server/routes.rs
git commit -m "refactor: security layer適用境界をhelper化"
```

## Task 3: Verification and Issue Closure Notes

**Files:**
- Modify: `docs/todo/TODO.md` only if issue 145 is tracked there and the existing project convention marks completed issue work in that file.
- No source changes unless verification exposes a concrete failure.

- [ ] **Step 1: Run Host-related tests**

Run:

```bash
cargo test --all-targets --all-features host
```

Expected: PASS. This covers Host middleware unit tests, integration Host rejection tests, and WebSocket Host anomaly tests.

- [ ] **Step 2: Run the full integration test binary**

Run:

```bash
cargo test --test integration_test
```

Expected: PASS. This confirms security test refactoring did not break unrelated integration modules.

- [ ] **Step 3: Run full verification**

Run:

```bash
./verify.sh
```

Expected: PASS for format, clippy, Rust tests, and any project-required checks.

- [ ] **Step 4: Inspect the final diff**

Run:

```bash
git diff --stat HEAD~2..HEAD
git diff --check HEAD~2..HEAD
```

Expected: `git diff --check` exits successfully. For the implementation commits, the stat should show only `src/server/routes.rs` and `tests/integration/security.rs`, plus `docs/todo/TODO.md` if the issue tracker file was intentionally updated. For the full branch diff, `docs/superpowers/plans/...` and `docs/superpowers/specs/...` are also expected because this work includes planning artifacts.

- [ ] **Step 5: Commit verification-only documentation if needed**

If `docs/todo/TODO.md` was updated, commit it separately after approval:

```bash
git add docs/todo/TODO.md
git commit -m "docs: issue 145完了を記録"
```

Expected: commit succeeds. If no documentation file changed, skip this step.

## Rollback

Revert the implementation commits in reverse order.

```bash
git revert <verification-docs-commit>
git revert <router-helper-commit>
git revert <test-refactor-commit>
```

If only the router helper causes problems, revert `refactor: security layer適用境界をhelper化`; the case-driven integration test can remain because it preserves the same public behavior.

## Self-Review

- Spec coverage: Task 1 covers integration `HOST_SMOKE_CASES`, Host/security smoke tests, and security headers. Task 2 covers `apply_security_layers()`. Task 3 covers full verification and rollback-ready documentation.
- Security: Host, Origin, CSP, and response header policies are reused without relaxation. Rejection paths continue to assert `403`, JSON error, `nosniff`, `DENY`, and CSP.
- Scope: The implementation touches `src/server/routes.rs`, `tests/integration/security.rs`, and optional issue bookkeeping. The full branch also includes this plan and the companion spec under `docs/superpowers/`. It does not change renderer, watcher, file service, or public API.
