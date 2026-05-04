# WebSocket Origin Rejection Log Classification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `WsOriginRejection` のログ分類を wildcard なしの全列挙にし、Host 系 rejection の実ログ分類を traced test で固定する。

**Architecture:** `src/server/guards.rs` に分類 helper を追加し、`is_allowed_ws_origin()` は helper が返す level/message を `emit_ws_rejection_log()` へ渡して出力する。現行 `tracing` macro 制約に合わせ、helper 内で `error!` / `warn!` / `info!` などへ level dispatch する。Host / Origin の許可判定、HTTP status、WebSocket 拒否レスポンス body は変更しない。

**Tech Stack:** Rust, axum, tracing, tracing-test, cargo test, `./verify.sh`

---

## Execution Preconditions

Commit を含む実装手順を実行する前に、以下を確認する。

- ユーザーがこの plan の実装を承認している。
- `git status --short --branch` で現在ブランチと未コミット差分を確認する。
- `main` または `develop` 上なら停止し、feature/fix branch または worktree に移る。
- 未コミット差分に対象外ファイルやユーザー作業が混ざっている場合は停止し、扱いを確認する。
- `git commit` 手順は、承認済みの実装セッション内でのみ実行する。

## File Structure

- Modify: `src/server/guards.rs`
  - `WsOriginRejection` の Host 系 doc comment を更新する。
  - `is_host_middleware_bypass_indicator()` を wildcard なしの `match` にする。
  - `ws_rejection_log_level()` と `ws_rejection_log_message()` を追加する。
  - `is_allowed_ws_origin()` の nested `if/match` と `_ => warn!()` を `emit_ws_rejection_log()` helper 経由の level dispatch に置き換える。
  - 分類 unit test と traced log test を追加する。
- Modify: `src/server/routes.rs`
  - `ws_handler()` の Host 再検証コメントを、実装後のログ分類と一致する説明へ更新する。
- Modify: `docs/todo/TODO.md`
  - Medium Priority の該当項目を Done Summary に移し、完了根拠を残す。

## Task 1: 分類 helper の期待を TDD で固定する

**Files:**
- Modify: `src/server/guards.rs`

- [ ] **Step 1: Write the failing helper tests**

In `src/server/guards.rs`, replace the existing `test_ws_host系拒否はmiddleware_bypass兆候として分類する` test with this expanded test block:

```rust
    #[test]
    fn test_ws_origin拒否ログ分類は全variantを明示する() {
        let cases = [
            (
                WsOriginRejection::MissingHost,
                true,
                tracing::Level::ERROR,
                "WS Host 検証異常",
            ),
            (
                WsOriginRejection::HostMalformed,
                true,
                tracing::Level::ERROR,
                "WS Host 検証異常",
            ),
            (
                WsOriginRejection::UntrustedHost,
                true,
                tracing::Level::ERROR,
                "WS Host 検証異常",
            ),
            (
                WsOriginRejection::MissingOrigin,
                false,
                tracing::Level::INFO,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::OriginMalformed,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::OriginParseError,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::UnsupportedScheme,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::OriginMissingAuthority,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::UntrustedOriginAuthority,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::AuthorityMismatch,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
        ];

        for (rejection, is_bypass_indicator, level, message) in cases {
            assert_eq!(
                is_host_middleware_bypass_indicator(rejection),
                is_bypass_indicator,
                "{rejection:?} の Host bypass 分類が不正"
            );
            assert_eq!(
                ws_rejection_log_level(rejection),
                level,
                "{rejection:?} のログレベル分類が不正"
            );
            assert_eq!(
                ws_rejection_log_message(rejection),
                message,
                "{rejection:?} のログメッセージ分類が不正"
            );
        }
    }
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
cargo test --all-targets --all-features test_ws_origin拒否ログ分類は全variantを明示する
```

Expected: FAIL with compiler errors similar to:

```text
cannot find function `ws_rejection_log_level` in this scope
cannot find function `ws_rejection_log_message` in this scope
```

- [ ] **Step 3: Implement the classification helpers**

In `src/server/guards.rs`, replace the current `is_host_middleware_bypass_indicator()` helper with:

```rust
fn is_host_middleware_bypass_indicator(rejection: WsOriginRejection) -> bool {
    match rejection {
        WsOriginRejection::MissingHost
        | WsOriginRejection::HostMalformed
        | WsOriginRejection::UntrustedHost => true,
        WsOriginRejection::MissingOrigin
        | WsOriginRejection::OriginMalformed
        | WsOriginRejection::OriginParseError
        | WsOriginRejection::UnsupportedScheme
        | WsOriginRejection::OriginMissingAuthority
        | WsOriginRejection::UntrustedOriginAuthority
        | WsOriginRejection::AuthorityMismatch => false,
    }
}

fn ws_rejection_log_level(rejection: WsOriginRejection) -> tracing::Level {
    match rejection {
        WsOriginRejection::MissingHost
        | WsOriginRejection::HostMalformed
        | WsOriginRejection::UntrustedHost => tracing::Level::ERROR,
        WsOriginRejection::MissingOrigin => tracing::Level::INFO,
        WsOriginRejection::OriginMalformed
        | WsOriginRejection::OriginParseError
        | WsOriginRejection::UnsupportedScheme
        | WsOriginRejection::OriginMissingAuthority
        | WsOriginRejection::UntrustedOriginAuthority
        | WsOriginRejection::AuthorityMismatch => tracing::Level::WARN,
    }
}

fn ws_rejection_log_message(rejection: WsOriginRejection) -> &'static str {
    match rejection {
        WsOriginRejection::MissingHost
        | WsOriginRejection::HostMalformed
        | WsOriginRejection::UntrustedHost => "WS Host 検証異常",
        WsOriginRejection::MissingOrigin
        | WsOriginRejection::OriginMalformed
        | WsOriginRejection::OriginParseError
        | WsOriginRejection::UnsupportedScheme
        | WsOriginRejection::OriginMissingAuthority
        | WsOriginRejection::UntrustedOriginAuthority
        | WsOriginRejection::AuthorityMismatch => "WS Origin 拒否",
    }
}
```

- [ ] **Step 4: Run the focused helper test and verify it passes**

Run:

```bash
cargo test --all-targets --all-features test_ws_origin拒否ログ分類は全variantを明示する
```

Expected: PASS. The output should include:

```text
test server::guards::tests::test_ws_origin拒否ログ分類は全variantを明示する ... ok
```

- [ ] **Step 5: Commit helper classification**

Run:

```bash
git add src/server/guards.rs
git commit -m "refactor: WS Origin拒否ログ分類を全列挙化"
```

Expected: commit succeeds with only `src/server/guards.rs` staged.

## Task 2: `is_allowed_ws_origin()` のログ出力を helper 経由にする

**Files:**
- Modify: `src/server/guards.rs`

- [ ] **Step 1: Write traced tests for all Host-class rejections**

In `src/server/guards.rs`, replace the existing `test_ws_host系拒否はbypass兆候として専用ログに記録する` test with these three tests:

```rust
    #[test]
    #[traced_test]
    fn test_ws_missing_hostはhost検証異常ログに記録する() {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());

        assert!(!is_allowed_ws_origin(&headers));
        assert!(logs_contain("WS Host 検証異常"));
        assert!(logs_contain("middleware bypass、または Host 検証通過後の malformed/untrusted probe"));
        assert!(logs_contain("MissingHost"));
        assert!(!logs_contain("WS Origin 拒否"));
    }

    #[test]
    #[traced_test]
    fn test_ws_host_malformedはhost検証異常ログに記録する() {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        headers.insert(
            HOST,
            axum::http::HeaderValue::from_bytes(b"\xff non-ascii host").unwrap(),
        );

        assert!(!is_allowed_ws_origin(&headers));
        assert!(logs_contain("WS Host 検証異常"));
        assert!(logs_contain("middleware bypass、または Host 検証通過後の malformed/untrusted probe"));
        assert!(logs_contain("HostMalformed"));
        assert!(logs_contain("<non-ascii>"));
        assert!(!logs_contain("WS Origin 拒否"));
    }

    #[test]
    #[traced_test]
    fn test_ws_untrusted_hostはhost検証異常ログに記録する() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "evil.example:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());

        assert!(!is_allowed_ws_origin(&headers));
        assert!(logs_contain("WS Host 検証異常"));
        assert!(logs_contain("middleware bypass、または Host 検証通過後の malformed/untrusted probe"));
        assert!(logs_contain("UntrustedHost"));
        assert!(logs_contain("evil.example:3000"));
        assert!(!logs_contain("WS Origin 拒否"));
    }
```

- [ ] **Step 2: Run the new traced tests and verify they fail**

Run:

```bash
cargo test --all-targets --all-features test_ws_missing_hostはhost検証異常ログに記録する
cargo test --all-targets --all-features test_ws_host_malformedはhost検証異常ログに記録する
cargo test --all-targets --all-features test_ws_untrusted_hostはhost検証異常ログに記録する
```

Expected: FAIL because current logs contain `WS Host middleware bypass 兆候` instead of `WS Host 検証異常` and the new explanatory phrase is absent.

- [ ] **Step 3: Replace `is_allowed_ws_origin()` logging**

In `src/server/guards.rs`, replace the current `Err(rejection)` body inside `is_allowed_ws_origin()` with:

```rust
        Err(rejection) => {
            let host = log_value_for_header(headers, &HOST);
            let origin = log_value_for_header(headers, &ORIGIN);
            let level = ws_rejection_log_level(rejection);
            let message = ws_rejection_log_message(rejection);
            let host_recheck_anomaly = is_host_middleware_bypass_indicator(rejection);
            emit_ws_rejection_log(level, message, host_recheck_anomaly, rejection, host, origin);
            false
        }
```

`emit_ws_rejection_log()` は `tracing::event!` の動的 level 指定に依存せず、`match level` で `error!` / `warn!` / `info!` などへ dispatch する。ログ本文は既存の人間向け文字列を維持しつつ、`rejection = ?rejection`, `host = ?host`, `origin = ?origin`, `ws_rejection_class = message`, `host_recheck_anomaly = host_recheck_anomaly` を named fields として併記する。

After replacement, the full function should be:

```rust
pub(super) fn is_allowed_ws_origin(headers: &HeaderMap) -> bool {
    match check_ws_origin(headers) {
        Ok(()) => true,
        Err(rejection) => {
            let host = log_value_for_header(headers, &HOST);
            let origin = log_value_for_header(headers, &ORIGIN);
            let level = ws_rejection_log_level(rejection);
            let message = ws_rejection_log_message(rejection);
            let host_recheck_anomaly = is_host_middleware_bypass_indicator(rejection);
            emit_ws_rejection_log(level, message, host_recheck_anomaly, rejection, host, origin);
            false
        }
    }
}
```

- [ ] **Step 4: Update the `HostMalformed` enum doc comment**

In `src/server/guards.rs`, replace this sentence in the `HostMalformed` doc comment:

```rust
    /// middleware bypass 兆候として error レベルで記録する。
```

With:

```rust
    /// middleware bypass、または Host 検証通過後の malformed probe として
    /// error レベルで記録する。通常運用では到達しない。
```

- [ ] **Step 5: Run the traced tests and variant regression tests**

Run:

```bash
cargo test --all-targets --all-features test_ws_missing_hostはhost検証異常ログに記録する
cargo test --all-targets --all-features test_ws_host_malformedはhost検証異常ログに記録する
cargo test --all-targets --all-features test_ws_untrusted_hostはhost検証異常ログに記録する
cargo test --all-targets --all-features test_check_ws_origin_variants_網羅
```

Expected: PASS. The existing `test_check_ws_origin_variants_網羅` must still pass without changing any expected rejection variant.

- [ ] **Step 6: Confirm wildcard logging is gone**

Run:

```bash
rg -n "_ => warn|WS Host middleware bypass 兆候" src/server/guards.rs docs/todo/TODO.md src/server/routes.rs
```

Expected: no matches. `WS Origin 拒否` は helper の返り値や test expectation として許容されるため、この禁止文字列検索には含めない。

- [ ] **Step 7: Commit logging refactor**

Run:

```bash
git add src/server/guards.rs
git commit -m "test: WS Host系拒否の実ログ分類を固定"
```

Expected: commit succeeds with only `src/server/guards.rs` staged.

## Task 3: コメントと TODO を実装内容に合わせる

**Files:**
- Modify: `src/server/routes.rs`
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Update the WebSocket handler comment**

In `src/server/routes.rs`, replace the comment above `if !is_allowed_ws_origin(&headers)` with:

```rust
    // Host は router middleware で先に検証済み。ここでは WS 固有の
    // Origin authority 一致を検証する。
    // `is_allowed_ws_origin` 内の Host 再検証は middleware 後段では
    // 通常到達しないが、middleware bypass、または Host 検証通過後の
    // malformed/untrusted probe を検知する defense-in-depth として残す。
    // 通常の Origin 系拒否は MissingOrigin/UnsupportedScheme/AuthorityMismatch
    // などとして段階化して記録する。
```

- [ ] **Step 2: Move the TODO item to Done Summary**

In `docs/todo/TODO.md`, remove this full Medium Priority item:

```markdown
- [ ] `WsOriginRejection` ログ分類を完全列挙し、Host bypass 観測性テストを補強する
  - ファイル: `src/server/guards.rs`, `docs/todo/TODO.md`
  - 現状: PR #123 で Host 系 `WsOriginRejection::{MissingHost, HostMalformed, UntrustedHost}` を bypass 兆候として `error!` に上げ、MissingHost の traced log test を追加した。一方、`is_allowed_ws_origin()` の非 Host 系ログ分類は `_ => warn!()` に残っており、新しい rejection variant が追加された場合にコンパイラで分類漏れを検出できない。`is_host_middleware_bypass_indicator()` も `matches!` の false 側へ暗黙に落ちるため、variant 追加時の意図確認が弱い。HostMalformed / UntrustedHost の実ログ出力は helper 分類テストで間接的に守られているが、traced log test では直接固定していない
  - 対応: `WsOriginRejection` 全 variant を match で明示列挙し、Host 系 / MissingOrigin / その他 Origin 系の分類を compiler-enforced にする。可能なら `level_for_ws_rejection(rejection) -> tracing::Level` と `message_for_ws_rejection(rejection)` 相当の小 helper へ分け、`emit_ws_rejection_log()` でログ出力を集約する。HostMalformed / UntrustedHost の traced log test も追加し、コメントは「middleware bypass、または Host 検証通過後の malformed/untrusted probe。通常運用では到達しない」に更新する
  - 理由: DNS Rebinding 防御の判定自体は変えずに、将来 variant 追加時の silent fallback とログ分類漏れをコンパイル時・テスト時に検出しやすくする
```

Then add this Done Summary item near the top of `## Done Summary`:

```markdown
- [x] `WsOriginRejection` ログ分類を完全列挙し、Host bypass 観測性テストを補強する
  - 完了根拠: `WsOriginRejection` のログ分類を wildcard なしの helper に分離し、Host 系 3 variant の traced log test と `emit_ws_rejection_log()` 経由の単一 helper 出力で固定した
```

- [ ] **Step 3: Run docs/comment validation**

Run:

```bash
rg -n "WsOriginRejection` ログ分類を完全列挙|WS Host middleware bypass 兆候|_ => warn" docs/todo/TODO.md src/server/routes.rs src/server/guards.rs
```

Expected:

- `WsOriginRejection` ログ分類 item appears only in `Done Summary`.
- `WS Host middleware bypass 兆候` has no matches.
- `_ => warn` has no matches.

- [ ] **Step 4: Run formatting check**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS. If formatting fails, run `git status --short` and inspect the changed-file scope before running `cargo fmt --all`. If unrelated or user-owned Rust changes exist, stop and confirm handling first. After formatting, rerun `git status --short` and include only files touched by this task or already-owned Rust files in the commit.

- [ ] **Step 5: Commit comments and TODO update**

Run:

```bash
git add src/server/routes.rs docs/todo/TODO.md
git commit -m "docs: WS Origin拒否ログ分類TODOを完了"
```

Expected: commit succeeds with only `src/server/routes.rs` and `docs/todo/TODO.md` staged, unless `cargo fmt --all` changed Rust formatting in files already touched by Tasks 1-2.

## Task 4: Full verification

**Files:**
- Verify only

- [ ] **Step 1: Run full Rust test suite**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 2: Run required repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS. This should cover format, clippy, and tests according to repository policy.

- [ ] **Step 3: Confirm final diff scope**

Run:

```bash
git status --short --branch
git diff --stat 81981b2347ae1c44f0bb3a03ff28175e4576c80a HEAD
```

Expected:

- Working tree is clean.
- Branch diff includes the implementation files (`src/server/guards.rs`, `src/server/routes.rs`, `docs/todo/TODO.md`) plus this plan/spec documentation, and any retained review report under `reviews/`.
- If checking implementation commits only, use the relevant implementation commit range and document that narrower scope explicitly.

- [ ] **Step 4: Prepare completion report**

Report:

- Changed files and rough line impact.
- Affected dependent files: WebSocket route handling and guards tests.
- Verification results for `cargo test --all-targets --all-features` and `./verify.sh`.
- Residual risk: `OriginMissingAuthority` remains defensive and still lacks a directly reachable input under current `http` crate behavior.
