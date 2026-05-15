# Host Middleware Boundary Tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Host middleware 化後の主要 route 境界を統合テストで固定し、許可 Host は通り、異常 Host は security headers 付き `403` で拒否されることを確認する。

**Architecture:** 既存の `tests/integration/security.rs` にある `HOST_SMOKE_CASES` を唯一の route 列挙元として使い、許可 Host smoke と拒否 Host smoke の両方を同じ route 群へ適用する。production code は変更せず、HTTP 経由で再現できない malformed Host は既存の `src/server/guards.rs` unit test で担保済みであることを確認し、今回の新規差分は integration smoke に集中する。

**Tech Stack:** Rust, Tokio, reqwest, tokio-tungstenite, axum integration tests.

---

## File Structure

- Modify: `tests/integration/security.rs`
  - `HOST_SMOKE_CASES` と `HostSmokeRequest` を既存の route 列挙元として維持する。
  - 許可 Host smoke test を追加する。
  - 空 Host rejection test を追加する。
- Reference only: `src/server/guards.rs`
  - 欠落 Host と非 ASCII Host は request Host guard 境界の unit test で固定する。HTTP 経由で送れない異常値を integration test 用に無理に公開しない。
- Reference only: `tests/integration/support.rs`
  - `/ws` 許可 Host smoke では既存 `connect_ws` helper を使う。

実装は通常の `develop` 直作業ではなく、実行時に `superpowers:using-git-worktrees` で作業用 worktree / feature branch を作ってから進める。

## Task 1: 許可 Host Smoke を追加する

**Files:**
- Modify: `tests/integration/security.rs`
- Test: `tests/integration/security.rs`

- [ ] **Step 1: Write the failing test**

Add this test after `test_host_middlewareは主要routeの不正hostを拒否しsecurity_headerを維持する`:

```rust
#[tokio::test]
async fn test_host_middlewareは許可hostで主要routeを通過させsecurity_headerを維持する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Host Allowed").await;
    let client = reqwest::Client::new();
    let allowed_host = format!("127.0.0.1:{}", addr.port());

    for case in HOST_SMOKE_CASES {
        match &case.request {
            HostSmokeRequest::WebSocketUpgrade => {
                let url = format!("ws://{}/ws", addr);
                let origin = format!("http://{}", addr);
                connect_ws(&url, &origin).await.unwrap_or_else(|err| {
                    panic!("{} should connect with allowed Host/Origin: {err}", case.name)
                });
            }
            _ => {
                let resp = send_host_smoke_request(&client, addr, &allowed_host, case)
                    .await
                    .unwrap_or_else(|err| {
                        panic!("{} should receive a response with allowed Host: {err}", case.name)
                    });

                assert_ne!(
                    resp.status(),
                    reqwest::StatusCode::FORBIDDEN,
                    "{} should not be rejected by Host middleware",
                    case.name
                );
                assert_security_headers(&resp);
            }
        }
    }
}
```

- [ ] **Step 2: Run the targeted test to verify it fails**

Run:

```bash
cargo test --test integration_test --all-features test_host_middlewareは許可hostで主要routeを通過させsecurity_headerを維持する
```

Expected: FAIL because `assert_security_headers` does not exist yet.

- [ ] **Step 3: Extract the security-header assertion helper**

In `tests/integration/security.rs`, replace `assert_forbidden_with_security_headers` with these two helpers:

```rust
fn assert_forbidden_with_security_headers(resp: &reqwest::Response) {
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
    assert_security_headers(resp);
}

fn assert_security_headers(resp: &reqwest::Response) {
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
```

- [ ] **Step 4: Run the targeted test to verify it passes**

Run:

```bash
cargo test --test integration_test --all-features test_host_middlewareは許可hostで主要routeを通過させsecurity_headerを維持する
```

Expected: PASS. If `/api/files` returns a non-2xx application status in single-file mode, keep the test assertion as `status != 403`; this task tests Host middleware passage, not endpoint semantics.

- [ ] **Step 5: Commit Task 1**

Run:

```bash
git add tests/integration/security.rs
git commit -m "test: Host middlewareの許可Host smokeを追加"
```

## Task 2: 空 Host Rejection を追加する

**Files:**
- Modify: `tests/integration/security.rs`
- Test: `tests/integration/security.rs`

- [ ] **Step 1: Write the failing test**

Add this test after the allowed Host smoke test:

```rust
#[tokio::test]
async fn test_host_middlewareは空hostを拒否しsecurity_headerを維持する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Empty Host").await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://{}/", addr))
        .header("Host", "")
        .send()
        .await
        .unwrap();

    assert_forbidden_with_security_headers(&resp);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["error"], "許可されていないHostヘッダーです");
}
```

- [ ] **Step 2: Run the targeted test**

Run:

```bash
cargo test --test integration_test --all-features test_host_middlewareは空hostを拒否しsecurity_headerを維持する
```

Expected: PASS. The response status is `403`, and the JSON body is `{"error":"許可されていないHostヘッダーです"}`.

- [ ] **Step 3: Run the existing invalid Host smoke again**

Run:

```bash
cargo test --test integration_test --all-features test_host_middlewareは主要routeの不正hostを拒否しsecurity_headerを維持する
```

Expected: PASS.

- [ ] **Step 4: Commit Task 2**

Run:

```bash
git add tests/integration/security.rs
git commit -m "test: Host middlewareの空Host拒否を固定"
```

## Task 3: 欠落・Malformed Host の Request Host Guard Coverage を追加する

**Files:**
- Reference: `src/server/guards.rs`
- Test: `src/server/guards.rs`

- [ ] **Step 1: Add request Host guard tests for missing and non-ASCII Host**

Add unit tests in `src/server/guards.rs`:

```rust
#[test]
fn test_request_host_guardは欠落hostをforbidden_jsonで拒否する() {
    let headers = HeaderMap::new();

    assert_request_host_guard_forbidden(&headers);
}

#[test]
fn test_request_host_guardは非ascii_hostをforbidden_jsonで拒否する() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HOST,
        axum::http::HeaderValue::from_bytes(b"\xff non-ascii host").unwrap(),
    );

    assert_request_host_guard_forbidden(&headers);
}

fn assert_request_host_guard_forbidden(headers: &HeaderMap) {
    let error = ensure_allowed_request_host(headers).unwrap_err();
    assert_eq!(error.0, StatusCode::FORBIDDEN);
    assert_eq!(error.1["error"], "許可されていないHostヘッダーです");
}
```

- [ ] **Step 2: Run guard tests**

Run:

```bash
cargo test --lib --all-features server::guards::tests::test_request_host_guardは欠落hostをforbidden_jsonで拒否する
cargo test --lib --all-features server::guards::tests::test_request_host_guardは非ascii_hostをforbidden_jsonで拒否する
```

Expected: Both commands PASS.

## Task 4: Final Verification

**Files:**
- Verify: `tests/integration/security.rs`
- Verify: `src/server/guards.rs`
- Verify: `docs/superpowers/specs/2026-05-15-host-middleware-boundary-tests-design.md`

- [ ] **Step 1: Run integration tests**

Run:

```bash
cargo test --test integration_test --all-features
```

Expected: PASS.

- [ ] **Step 2: Run full verification**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 3: Review final diff**

Run:

```bash
git status --short
git diff --stat HEAD
git diff HEAD -- tests/integration/security.rs src/server/guards.rs
```

Expected: Only the intended test files are changed since the last task commit. No production behavior changes are present.

- [ ] **Step 4: Commit any remaining verification-only cleanup**

If formatting changed files after the task commits, commit them:

```bash
git add tests/integration/security.rs src/server/guards.rs
git commit -m "test: Host middleware境界テストを整える"
```

If there are no remaining changes, do not create an empty commit.
