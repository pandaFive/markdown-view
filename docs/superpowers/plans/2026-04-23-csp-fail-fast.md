# CSP Fail-Fast Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Status (2026-04-24): IMPLEMENTED on `feat/csp-fail-fast`.**
> Baseline `develop` at `7d872e3` already included PR #86 path-log-sanitizer work. This branch removes the CSP fallback, removes `x-markdown-view-security-warning`, and updates the integration test to assert that the warning header is absent.

**Goal:** `build_csp_header` のフォールバック CSP（sha256 制約を silent に喪失する経路）を削除し、`HeaderValue::from_str` 失敗時は startup panic で明示的に止める。`x-markdown-view-security-warning` ヘッダーも不要になるため削除する。

**Architecture:** `build_csp_header(syntax_css: &str) -> HeaderValue` が tuple を返さなくなる。`create_router` から `csp_fallback` 変数と `security_warning` ヘッダー設定を削除。フォールバック分岐は構造上到達不能（`csp_hash_sources` は base64 sha256 のみを返すため）であり、到達した場合は契約破り（バグ）として startup を止める。

**Tech Stack:** Rust 1.x, axum, http (HeaderValue, HeaderName), tokio runtime（startup 経路）

**関連 spec:** `docs/superpowers/specs/2026-04-23-defensive-observability-hardening-design.md` Part 1

---

## ブランチ戦略

このプランの実装は最新 `develop` から feature ブランチ `feat/csp-fail-fast` で行う（CLAUDE.md ブランチルール準拠）。develop へは squash merge。

既に同名ブランチが存在する場合は、実装前に最新 `develop`（PR #86 反映済み）との差分を確認し、必要なら rebase/作り直しで path-log-sanitizer 実装を巻き戻さない状態に揃える。

```bash
git checkout develop
git pull
git checkout -b feat/csp-fail-fast
```

---

### Task 1: 既存統合テストを「ヘッダー不在」アサーションに変更（RED フェーズ）

**Files:**
- Modify: `tests/integration_test.rs:932-937`

**背景:** 既存のテスト関数（L932-937）は `x-markdown-view-security-warning == "none"` をアサートしている。本プランでこのヘッダー自体を削除するため、アサーションを「ヘッダーが存在しないこと」に変更する。

- [ ] **Step 1: 既存テストの該当ブロックを修正**

`tests/integration_test.rs` の L932-937:

```rust
    assert_eq!(
        resp.headers()
            .get("x-markdown-view-security-warning")
            .unwrap(),
        "none"
    );
```

を以下に置換:

```rust
    assert!(
        resp.headers()
            .get("x-markdown-view-security-warning")
            .is_none(),
        "x-markdown-view-security-warning ヘッダーは廃止済みのため応答に含まれてはならない"
    );
```

- [ ] **Step 2: テストを実行して fail を確認**

Run: `cargo test --test integration_test 2>&1 | grep -A 3 'security-warning\|FAILED\|panicked'`

Expected: 該当テストが FAIL する（「security-warning ヘッダーは廃止済み」のメッセージで panic）。これは現状のサーバーが当該ヘッダーを設定するため。

- [ ] **Step 3: コミット（RED）**

```bash
git add tests/integration_test.rs
git commit -m "test: x-markdown-view-security-warning ヘッダー廃止に向けてアサーションを反転"
```

---

### Task 2: `build_csp_header` から fallback を削除し fail-fast に変更

**Files:**
- Modify: `src/server/guards.rs:13-38`

- [ ] **Step 1: `build_csp_header` 関数本体を書き換え**

`src/server/guards.rs` の L13-38（`build_csp_header` 関数全体）を以下に置換:

```rust
pub(super) fn build_csp_header(syntax_css: &str) -> HeaderValue {
    let (script_src, style_src) = csp_hash_sources(syntax_css);
    let csp = format!(
        "default-src 'self'; script-src {}; style-src {}; img-src 'self'; connect-src 'self' ws: wss:; object-src 'none'; frame-ancestors 'none'",
        script_src, style_src
    );
    // csp_hash_sources は base64 sha256 のみを返す契約のため、
    // visible-ASCII 違反による HeaderValue::from_str 失敗は構造上到達不能。
    // 到達した場合は契約破り（バグ）であり、permissive な fallback CSP で
    // silent に degradation するより startup panic で表面化させる。
    HeaderValue::from_str(&csp).unwrap_or_else(|e| {
        panic!(
            "CSP ヘッダー生成に失敗（csp_hash_sources の出力契約破り）: {} (CSP: {})",
            e, csp
        )
    })
}
```

戻り値型が `(HeaderValue, bool)` から `HeaderValue` に変わる。フォールバック静的 CSP と `tracing::error!`/`tracing::warn!` を削除。

- [ ] **Step 2: ビルドして call 側エラーを確認**

Run: `cargo build 2>&1 | head -40`

Expected: `src/server/routes.rs:40` で型ミスマッチエラー（`expected (HeaderValue, bool), found HeaderValue` 系の compile error）。これは Task 3 で解消する。

---

### Task 3: `create_router` から `csp_fallback` と `security_warning` ヘッダーを削除

**Files:**
- Modify: `src/server/routes.rs:40-78`

- [ ] **Step 1: 該当ブロックを書き換え**

`src/server/routes.rs` の L40-78 内を編集する。

L40 の以下を置換:

```rust
    let (csp_header, csp_fallback) = build_csp_header(state.syntax_css());
    let security_warning = if csp_fallback {
        HeaderValue::from_static("csp-fallback")
    } else {
        HeaderValue::from_static("none")
    };
```

を以下に変更:

```rust
    let csp_header = build_csp_header(state.syntax_css());
```

L75-78 の `x-markdown-view-security-warning` ヘッダー設定 layer を **削除**:

```rust
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-markdown-view-security-warning"),
            security_warning,
        ))
```

→ ブロックごと削除。

- [ ] **Step 2: 不要 import の整理**

Run: `cargo build 2>&1 | head -40`

Expected: `HeaderName` または `SetResponseHeaderLayer` が他で使われていれば pass。使われていない import を warn された場合は削除する。`grep -n 'HeaderName' src/server/routes.rs` で確認し、不要なら use 文から外す。

- [ ] **Step 3: 全テストを実行**

Run: `cargo test --all-targets --all-features 2>&1 | tail -30`

Expected: Task 1 で書き換えたテストを含めて **全テスト pass**。

- [ ] **Step 4: lint と format を確認**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS（warnings なし）

Run: `cargo fmt --all -- --check`
Expected: PASS（フォーマット差分なし）

- [ ] **Step 5: 統合検証**

Run: `./verify.sh`
Expected: 全 phase pass

- [ ] **Step 6: 手動起動確認**

Run（別ターミナルで）: `cargo run -- README.md --port 18080`

別シェルで:
```bash
curl -s -i -H 'Host: localhost:18080' http://127.0.0.1:18080/ | head -25
```

Expected:
- `Content-Security-Policy:` ヘッダーに `script-src 'sha256-...'` と `style-src 'sha256-...'` が含まれる
- `x-markdown-view-security-warning` ヘッダーが応答に **含まれない**

確認後 Ctrl+C でサーバー停止。

- [ ] **Step 7: コミット（GREEN）**

```bash
git add src/server/guards.rs src/server/routes.rs
git commit -m "feat: CSP フォールバック削除と x-markdown-view-security-warning ヘッダー廃止

変更内容:
- build_csp_header の戻り値を (HeaderValue, bool) から HeaderValue に変更
- HeaderValue::from_str 失敗時は startup panic にフォールバックを廃止
- create_router から csp_fallback 分岐と security_warning ヘッダーを削除

変更理由:
- フォールバック CSP は sha256 制約を silent に喪失する High 相当の degradation 経路
- csp_hash_sources は base64 sha256 のみを返す契約のため fallback 分岐は構造上到達不能であり、到達 = 契約破り（バグ）
- silent な permissive 化より startup panic のほうが利用者が確実に気づける

影響範囲:
- src/server/guards.rs: build_csp_header シグネチャ変更
- src/server/routes.rs: csp_fallback 変数と security_warning ヘッダー設定削除
- tests/integration_test.rs: x-markdown-view-security-warning 不在アサーションに反転"
```

---

## Self-Review チェックリスト（実装担当者向け）

実装後、以下を確認:

- [ ] `cargo test --all-targets --all-features` が全 pass
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` が pass
- [ ] `cargo fmt --all -- --check` が pass
- [ ] `./verify.sh` が pass
- [ ] `grep -rn 'csp_fallback\|security_warning' src/ tests/ --include='*.rs'` が空（runtime fallback 経路の削除確認）
- [ ] `grep -rn 'x-markdown-view-security-warning' src/ --include='*.rs'` が空（応答ヘッダー設定の削除確認。`tests/` の不在アサーションは許容）
- [ ] 手動 curl で CSP ヘッダーに `script-src 'sha256-` を含み、`x-markdown-view-security-warning` ヘッダーが含まれない

---

## 完了後

`feat/csp-fail-fast` ブランチを develop に **squash merge**。マージ後ローカルブランチ削除。
