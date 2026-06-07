# WS Host Bypass Structured Log Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Treat WS Host middleware bypass metrics as complete by hardening the existing structured log contract and documenting why no metrics runtime is added.

**Architecture:** Keep the runtime boundary in `src/server/guards.rs`. Strengthen unit tests around `WsOriginRejection` classification and emitted tracing fields, then move the BACKLOG item to Done with the metrics deferral rationale.

**Tech Stack:** Rust, axum header types, tracing, tracing-subscriber test capture, Markdown documentation, `cargo test`, `./verify.sh`.

---

## File Structure

- Modify `src/server/guards.rs`: Rename and tighten existing WS rejection log tests so Host-side anomalies are an explicit structured-log contract. Do not change Host / Origin validation behavior unless a test reveals the contract is not currently met.
- Modify `docs/todo/BACKLOG.md`: Move the P3 WS Host middleware bypass metrics item to Done and record that structured logs are the chosen lightweight observability contract.
- Reference `docs/superpowers/specs/2026-06-07-ws-host-bypass-structured-log-design.md`: Source design and acceptance criteria.

## Preflight Gate

Before implementation, confirm these items and stop if any check fails:

- The user approved this implementation after goal, non-goals, acceptance criteria, impact scope, and rollback path were presented.
- `git status --short --branch` shows a branch that is not `develop` or `main`.
- If the current branch is `develop` or `main`, create a feature/fix branch in the current repository before editing.
- The worktree has no unrelated uncommitted changes in files this plan will modify.

## Task 1: Contract-Test WS Host Bypass Structured Logs

**Files:**
- Modify: `src/server/guards.rs:788-976`

- [ ] **Step 1: Write the contract-focused test update**

Replace the current `test_ws_origin拒否実ログは分類levelと構造化fieldを出力する` body with the following contract-focused version. Keep `capture_ws_rejection_events()` unchanged.

```rust
    #[test]
    fn test_ws_host_bypass兆候は構造化errorログ契約として固定する() {
        let mut missing_host = HeaderMap::new();
        missing_host.insert(ORIGIN, "http://localhost:3000".parse().unwrap());

        let mut host_malformed = HeaderMap::new();
        host_malformed.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        host_malformed.insert(
            HOST,
            axum::http::HeaderValue::from_bytes(b"\xff non-ascii host").unwrap(),
        );

        let mut untrusted_host = HeaderMap::new();
        untrusted_host.insert(HOST, "evil.example:3000".parse().unwrap());
        untrusted_host.insert(ORIGIN, "http://localhost:3000".parse().unwrap());

        let cases = [
            (
                missing_host,
                "MissingHost",
                "<absent>",
                "http://localhost:3000",
            ),
            (
                host_malformed,
                "HostMalformed",
                "<non-ascii>",
                "http://localhost:3000",
            ),
            (
                untrusted_host,
                "UntrustedHost",
                "evil.example:3000",
                "http://localhost:3000",
            ),
        ];

        for (headers, expected_rejection, expected_host, expected_origin) in cases {
            let events = capture_ws_rejection_events(&headers);
            assert_eq!(
                events.len(),
                1,
                "{expected_rejection} の拒否ログ件数が不正: {events:?}"
            );

            let event = &events[0];
            assert_eq!(
                event.level,
                Level::ERROR,
                "{expected_rejection} は Host middleware bypass 兆候として ERROR で記録する"
            );
            assert!(
                event
                    .fields
                    .get("rejection")
                    .is_some_and(|actual| actual.contains(expected_rejection)),
                "{expected_rejection} の rejection field が不正: {:?}",
                event.fields
            );
            assert_eq!(
                event.fields.get("ws_rejection_class").map(String::as_str),
                Some("WS Host 検証異常"),
                "{expected_rejection} の分類 field が不正"
            );
            assert_eq!(
                event.fields.get("host_recheck_anomaly").map(String::as_str),
                Some("true"),
                "{expected_rejection} は host_recheck_anomaly=true で記録する"
            );
            assert_eq!(
                event.fields.get("host").map(String::as_str),
                Some(expected_host),
                "{expected_rejection} の host field は監査ログ用の正規化契約に従う"
            );
            assert_eq!(
                event.fields.get("origin").map(String::as_str),
                Some(expected_origin),
                "{expected_rejection} の origin field は監査ログ用の正規化契約に従う"
            );
        }
    }
```

- [ ] **Step 2: Add the Origin-side negative contract test**

Immediately after the Host contract test, add this separate Origin test. This keeps Host anomaly behavior and ordinary Origin rejection behavior distinct.

```rust
    #[test]
    fn test_ws_origin拒否はhost_bypass兆候として扱わない() {
        let mut missing_origin = HeaderMap::new();
        missing_origin.insert(HOST, "localhost:3000".parse().unwrap());

        let mut authority_mismatch = HeaderMap::new();
        authority_mismatch.insert(HOST, "localhost:3000".parse().unwrap());
        authority_mismatch.insert(ORIGIN, "http://127.0.0.1:3000".parse().unwrap());

        let cases = [
            (
                missing_origin,
                Level::INFO,
                "MissingOrigin",
                "localhost:3000",
                "<absent>",
            ),
            (
                authority_mismatch,
                Level::WARN,
                "AuthorityMismatch",
                "localhost:3000",
                "http://127.0.0.1:3000",
            ),
        ];

        for (headers, expected_level, expected_rejection, expected_host, expected_origin) in cases {
            let events = capture_ws_rejection_events(&headers);
            assert_eq!(
                events.len(),
                1,
                "{expected_rejection} の拒否ログ件数が不正: {events:?}"
            );

            let event = &events[0];
            assert_eq!(
                event.level, expected_level,
                "{expected_rejection} の実ログ level が不正"
            );
            assert!(
                event
                    .fields
                    .get("rejection")
                    .is_some_and(|actual| actual.contains(expected_rejection)),
                "{expected_rejection} の rejection field が不正: {:?}",
                event.fields
            );
            assert_eq!(
                event.fields.get("ws_rejection_class").map(String::as_str),
                Some("WS Origin 拒否"),
                "{expected_rejection} の分類 field が不正"
            );
            assert_eq!(
                event.fields.get("host_recheck_anomaly").map(String::as_str),
                Some("false"),
                "{expected_rejection} は host_recheck_anomaly=false で記録する"
            );
            assert_eq!(
                event.fields.get("host").map(String::as_str),
                Some(expected_host),
                "{expected_rejection} の host field は監査ログ用の正規化契約に従う"
            );
            assert_eq!(
                event.fields.get("origin").map(String::as_str),
                Some(expected_origin),
                "{expected_rejection} の origin field は監査ログ用の正規化契約に従う"
            );
        }
    }
```

- [ ] **Step 3: Run the targeted test and inspect the result**

Run:

```bash
cargo test server::guards::tests::test_ws_host_bypass兆候は構造化errorログ契約として固定する --all-targets --all-features
```

Expected: PASS if the current implementation already meets the contract. If it fails only because captured `host` / `origin` values are formatted differently, update the expected strings to match `CapturedFields::record_debug()` output without changing runtime logging behavior. If it fails because `host_recheck_anomaly`, level, or class is wrong, fix the runtime helper in the next step.

- [ ] **Step 4: Apply minimal runtime fix only if the test exposed a contract mismatch**

If Step 3 shows a runtime mismatch, ensure the existing helper functions match this logic. Do not change validation order or accepted hosts.

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

- [ ] **Step 5: Run all guards tests**

Run:

```bash
cargo test server::guards --all-targets --all-features
```

Expected: PASS. The output should show all `server::guards` unit tests passing.

- [ ] **Step 6: Commit Task 1**

Run:

```bash
git add src/server/guards.rs
git commit -m "test: WS Host bypassログ契約を固定"
```

Expected: A focused commit containing only `src/server/guards.rs`.

## Task 2: Move BACKLOG Item To Done

**Files:**
- Modify: `docs/todo/BACKLOG.md:33-40`

- [ ] **Step 1: Update BACKLOG text**

Remove the unchecked P3 item from `## P3: 長期改善・低緊急` and add this Done entry near the top of the `## Done` section.

```markdown
- [x] WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する
  - 完了根拠: `src/server/guards.rs` の Host 系 `WsOriginRejection` (`MissingHost`, `HostMalformed`, `UntrustedHost`) は、Host middleware 後段では通常到達しない bypass / malformed probe 兆候として `ERROR`、`ws_rejection_class="WS Host 検証異常"`、`host_recheck_anomaly=true` の構造化ログ契約で固定した。Origin 系拒否は `WS Origin 拒否`、`host_recheck_anomaly=false` として分離し、Host 系異常説明文を混ぜないことを unit test で確認した。
  - 判断: 個人向け localhost ツールとしては、既存の `error!` ログと structured field で異常兆候を確認できるため、metrics crate、counter state、HTTP endpoint、外部監視基盤は追加しない。継続集計が必要な本格運用要求が出た場合のみ、今回固定した `host_recheck_anomaly=true` ログを入力契約として counter 化を再検討する。
  - セキュリティ: Host / Origin は攻撃者制御の未信頼入力として扱い、ログ値は監査ログ用の正規化 helper 経由に限定する。Origin は parse 可能な場合も scheme + authority までを記録し、path / query / fragment は出さない。Host/Origin 検証、DNS Rebinding 対策、CSP、security headers、WebSocket payload は変更しない。query string、Markdown 本文、ファイルパス、full process args、環境変数は新規出力しない。
  - 由来: PR #123 レビュー follow-up (2026-05-04)、WS Host bypass structured log 契約設計 (2026-06-07)
```

- [ ] **Step 2: Validate BACKLOG no longer has the unchecked WS metrics item**

Run:

```bash
rg -n "WS Host middleware bypass|host_recheck_anomaly|metrics crate|query string" docs/todo/BACKLOG.md
```

Expected: The WS item appears only under `## Done`; no unchecked `- [ ] WS Host middleware bypass` line remains.

- [ ] **Step 3: Commit Task 2**

Run:

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: WS Host bypassメトリクス検討を完了整理"
```

Expected: A focused commit containing only `docs/todo/BACKLOG.md`.

## Task 3: Final Verification

**Files:**
- Verify: `src/server/guards.rs`
- Verify: `docs/todo/BACKLOG.md`
- Verify: `docs/superpowers/specs/2026-06-07-ws-host-bypass-structured-log-design.md`

- [ ] **Step 1: Run targeted verification**

Run:

```bash
cargo test server::guards --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 2: Run required repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS. If this fails due to environment or dependency setup, capture the failing command and stderr summary in the completion report.

- [ ] **Step 3: Run documentation sanity checks**

Run:

```bash
rg -n -P "T[B]D|TO[D]O[:：]|未[定]" docs/todo/BACKLOG.md docs/superpowers/specs/2026-06-07-ws-host-bypass-structured-log-design.md docs/superpowers/plans/2026-06-07-ws-host-bypass-structured-log.md
git diff --check
git status --short --branch
```

Expected: The placeholder scan prints no matches and exits with status 1 because there are no matches. `git diff --check` prints nothing and exits 0. `git status --short --branch` shows the current branch and no unstaged/untracked implementation files.

- [ ] **Step 4: Prepare completion report**

Report the completion summary in Japanese with these concrete facts:

- Changed files and rough line impact from `git show --stat --oneline HEAD`.
- Affected dependent files: `src/server/routes.rs` and `tests/integration/security.rs` are relevant references but should have no code changes.
- Verification outcomes for `cargo test server::guards --all-targets --all-features`, `./verify.sh`, placeholder scan, and `git diff --check`, using the actual command status and any failure summary.
- Residual risk: runtime metrics counters are intentionally not added; production-style monitoring remains a separate future task.
- Next backlog candidate: continue with a remaining search RSS P2 diagnostic only if absolute RSS improvement or environment comparison becomes necessary.
