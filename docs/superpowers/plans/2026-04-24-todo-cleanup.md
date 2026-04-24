# TODO Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Update `docs/todo/TODO.md` so verified-complete High / Medium items are checked while genuinely open items remain open.

**Architecture:** This is a docs-only cleanup driven by targeted evidence. Each candidate TODO item is verified against current source/tests with a focused command before its checkbox is changed. The final edit touches only `docs/todo/TODO.md`.

**Tech Stack:** Markdown, Git, Rust test suite via Cargo.

---

## File Structure

- Modify: `docs/todo/TODO.md` - active High / Medium task list; completed entries remain as `[x]`.
- Inspect: `src/server/guards.rs` - CSP fail-fast behavior.
- Inspect: `tests/integration_test.rs` - HTTP security-header assertions.
- Inspect: `src/server/log_path.rs` - base-relative path sanitization behavior and tests.
- Inspect: `src/server/files/{memo.rs,memo_sidecar.rs,tests.rs}` - memo sidecar invariant ownership and coverage.
- Inspect: `docs/superpowers/specs/2026-04-24-memo-sidecar-name-hardening-design.md` - sidecar residual-risk documentation.

## Task 1: Verify CSP TODO Candidate

**Files:**
- Inspect: `docs/todo/TODO.md`
- Inspect: `src/server/guards.rs`
- Inspect: `tests/integration_test.rs`
- Modify later: `docs/todo/TODO.md`

- [ ] **Step 1: Confirm current TODO entry**

Run: `rg -n "CSP フォールバック時の方針整理" docs/todo/TODO.md`

Expected: `39:- [ ] CSP フォールバック時の方針整理（fail-fast vs 現状運用）`

- [ ] **Step 2: Inspect CSP implementation evidence**

Run: `sed -n '13,32p' src/server/guards.rs`

Expected evidence includes:

```rust
pub(super) fn build_csp_header(syntax_css: &str) -> HeaderValue {
    let (script_src, style_src) = csp_hash_sources(syntax_css);
    let csp = format!(
        "default-src 'self'; script-src {}; style-src {}; img-src 'self'; connect-src 'self' ws: wss:; object-src 'none'; frame-ancestors 'none'",
        script_src, style_src
    );
    // ... fallback CSP で silent に degradation するより startup panic で表面化させる。
    HeaderValue::from_str(&csp).unwrap_or_else(|e| {
```

This proves the selected policy is fail-fast, not fallback.

- [ ] **Step 3: Inspect security-header test evidence**

Run: `sed -n '916,937p' tests/integration_test.rs`

Expected evidence includes assertions for:

```rust
assert!(csp.contains("script-src 'sha256-"));
assert!(csp.contains("style-src 'sha256-"));
assert!(!csp.contains("script-src 'unsafe-inline'"));
assert!(!csp.contains("style-src 'unsafe-inline'"));
assert!(!resp.headers().contains_key("x-markdown-view-security-warning"));
```

- [ ] **Step 4: Run targeted CSP verification**

Run: `cargo test --all-targets --all-features test_セキュリティヘッダが設定されている`

Expected: PASS.

- [ ] **Step 5: Record result**

If Steps 2-4 match expectations, Task 4 may mark the CSP TODO complete. If any step fails, leave it unchecked and report the failed evidence.

## Task 2: Verify Path Log Sanitization TODO Candidate

**Files:**
- Inspect: `docs/todo/TODO.md`
- Inspect: `src/server/log_path.rs`
- Inspect: `src/server/files/{resolve.rs,catalog.rs,content.rs,memo.rs}`
- Inspect: `src/watcher/strategy.rs`
- Modify later: `docs/todo/TODO.md`

- [ ] **Step 1: Confirm current TODO entry**

Run: `rg -n "エラー経路ログのパス情報を base 相対化" docs/todo/TODO.md`

Expected: `45:- [ ] エラー経路ログのパス情報を base 相対化`

- [ ] **Step 2: Inspect sanitizer contract**

Run: `sed -n '1,90p' src/server/log_path.rs`

Expected evidence includes:

```rust
//! 絶対パスの直接出力（`path.display()`）はディレクトリ構造を漏らすため、
//! base_dir 相対化を経由する。base 外パスは file_name のみ残して
//! `<outside-base>/{file_name}` で出力する。
pub(crate) fn sanitize_path_for_logging<'a>(path: &'a Path, base: &Path) -> Cow<'a, str> {
```

- [ ] **Step 3: Inspect warning callsites**

Run:

```bash
rg -n "warn!|sanitize_path_for_logging|\\.display\\(\\)" src/server/files src/watcher/strategy.rs src/server/log_path.rs
```

Expected: warning statements that include user-controlled path values pass path arguments through `sanitize_path_for_logging`. Do not mark complete if a relevant error-path `warn!` still logs a raw `path.display()` for a user-controlled path.

- [ ] **Step 4: Run targeted sanitizer verification**

Run: `cargo test --all-targets --all-features sanitize_path_for_logging`

Expected: PASS.

- [ ] **Step 5: Record result**

If Steps 2-4 match expectations, Task 4 may mark the path log TODO complete. If any step fails, leave it unchecked and report the failed evidence.

## Task 3: Verify Memo Sidecar TODO Candidate

**Files:**
- Inspect: `docs/todo/TODO.md`
- Inspect: `src/server/files/memo.rs`
- Inspect: `src/server/files/memo_sidecar.rs`
- Inspect: `src/server/files/tests.rs`
- Inspect: `docs/superpowers/specs/2026-04-24-memo-sidecar-name-hardening-design.md`
- Modify later: `docs/todo/TODO.md` only if fully verified

- [ ] **Step 1: Confirm current TODO entry**

Run: `rg -n "メモ sidecar 名生成の不変条件" docs/todo/TODO.md`

Expected:

```text
81:- [ ] メモ sidecar 名生成の不変条件を `SidecarMemoName` に集約し、境界テストと受容リスクを補強
```

- [ ] **Step 2: Inspect sidecar implementation state**

Run:

```bash
rg -n "SidecarMemoName|sidecar_name_too_long|MAX_FILENAME_BYTES|compat_from_file_name|fallback" src/server/files/memo.rs src/server/files/memo_sidecar.rs src/server/files/tests.rs
```

Expected current evidence includes `SidecarMemoName` and sidecar boundary tests. If `sidecar_name_too_long` still exists in `memo.rs`, the TODO's "delete or move this invariant into the type" acceptance point is not fully complete.

- [ ] **Step 3: Inspect residual-risk documentation**

Run:

```bash
rg -n "64 bit|64bit|hash 衝突|非 UTF-8|非utf8|fallback|受容リスク|既知リスク" docs/superpowers/specs/2026-04-24-memo-sidecar-name-hardening-design.md
```

Expected for completion: output mentions both hash-collision risk and non-UTF-8 fallback aggregation. If either risk is absent, leave the TODO unchecked.

- [ ] **Step 4: Run sidecar-focused verification**

Run: `cargo test --all-targets --all-features sidecar_name`

Expected: PASS.

- [ ] **Step 5: Record result**

Leave the sidecar TODO unchecked unless all conditions are true:

- `SidecarMemoName` owns the filename length invariant.
- `sidecar_name_too_long` is removed or moved behind the type contract.
- 255-byte / 256-byte / UTF-8-boundary / normalized-overlong tests exist.
- Hash collision and non-UTF-8 fallback residual risks are documented.
- The sidecar-focused test command passes.

## Task 4: Edit TODO Checkboxes

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Show current candidate lines**

Run: `sed -n '35,86p' docs/todo/TODO.md`

Expected: the CSP, path log, and sidecar candidate entries are visible.

- [ ] **Step 2: Update verified-complete checkboxes only**

If Tasks 1 and 2 passed and Task 3 did not fully pass, change only these two lines:

```diff
-- [ ] CSP フォールバック時の方針整理（fail-fast vs 現状運用）
+- [x] CSP フォールバック時の方針整理（fail-fast vs 現状運用）
```

```diff
-- [ ] エラー経路ログのパス情報を base 相対化
+- [x] エラー経路ログのパス情報を base 相対化
```

Do not change the sidecar TODO unless Task 3 proves all acceptance points are complete.

- [ ] **Step 3: Inspect the TODO diff**

Run: `git diff -- docs/todo/TODO.md`

Expected: only verified TODO checkbox lines changed. If product code or unrelated TODO entries changed, revert those unrelated edits before continuing.

## Task 5: Final Verification and Commit

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Run document sanity checks**

Run: `wc -l docs/todo/TODO.md`

Expected: below 300 lines.

Run: `rg -n "^- \\[[ x]\\]" docs/todo/TODO.md`

Expected: all TODO checkboxes use `- [ ]` or `- [x]`.

- [ ] **Step 2: Confirm no unintended files changed**

Run: `git status --short`

Expected after implementation edit:

```text
 M docs/todo/TODO.md
```

- [ ] **Step 3: Commit TODO cleanup**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: TODOの完了状態を検証結果に合わせる"
```

Expected: commit succeeds with only `docs/todo/TODO.md` changed.

- [ ] **Step 4: Prepare completion report**

Report changed file, inspected dependent files, every verification command result, residual risks for unchecked items, and rollback path: revert the TODO cleanup commit.

## Self-Review

- Spec coverage: The plan preserves checked completed items, requires implementation plus verification evidence before newly checking entries, leaves uncertain items open, reports security considerations, and keeps rollback docs-only.
- Red-flag scan: No incomplete markers are present outside literal file names and checklist syntax.
- Scope check: The plan touches only `docs/todo/TODO.md`; all other files are inspection or verification inputs.
- Security check: CSP, path disclosure, and sidecar path construction are verified against current source/tests before checkbox changes.
