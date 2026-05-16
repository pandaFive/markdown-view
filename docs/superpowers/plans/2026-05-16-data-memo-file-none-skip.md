# data-memo-file None Skip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `MemoResponse.file() == None` の初期 HTML では `data-memo-file` 属性を出力せず、`Some(file)` の属性出力は維持する。

**Architecture:** `src/template/page.rs` の HTML 属性生成だけを変更する。JSON、ブラウザ JS、メモ API、WebSocket payload には触れず、template unit test で初期 HTML 契約を固定する。

**Tech Stack:** Rust, cargo test, existing `src/template/page.rs` unit tests, `./verify.sh`

---

## File Structure

- Modify: `src/template/page.rs`
  - `render_page` の `memo_file_attr` 生成を `None` 時は空文字にする。
  - `#[cfg(test)] mod tests` に `MemoResponse::empty(None)` の属性非出力テストを追加する。
- No changes: `src/template/message.rs`
  - `MemoResponse` / `UpdateMessage` の JSON `file` 省略契約は既存のまま使う。
- No changes: `src/template/assets/js/*`
  - 現行 JS は `data-memo-file` を読んでいないため、新しい読み取り経路を追加しない。

## Task 1: `data-memo-file` None HTML Contract

**Files:**
- Modify: `src/template/page.rs`
- Test: `src/template/page.rs`

- [ ] **Step 1: Write the failing test**

Add this test near `test_メモuiが描画される` in `src/template/page.rs`:

```rust
    #[test]
    fn test_メモfileがnoneの場合data_memo_file属性を出力しない() {
        let memo = MemoResponse::empty(None);
        let html = render_single_file_page(&memo);

        assert!(
            !html.contains("data-memo-file"),
            "file=None は空属性ではなく属性なしとして表現すること"
        );
        assert!(html.contains("id=\"memo-editor\""));
        assert!(html.contains("id=\"memo-preview\""));
    }
```

- [ ] **Step 2: Run the targeted test and verify it fails**

Run:

```bash
cargo test template::page::tests::test_メモfileがnoneの場合data_memo_file属性を出力しない
```

Expected: FAIL because current `render_page` emits `data-memo-file=""` for `MemoResponse::empty(None)`.

- [ ] **Step 3: Implement the minimal HTML attribute change**

In `src/template/page.rs`, replace the current `memo_file_attr` block:

```rust
    let memo_file_attr = params
        .memo
        .file()
        .map(|file| html_attr("data-memo-file", file))
        .unwrap_or_else(|| html_attr("data-memo-file", ""));
```

with:

```rust
    let memo_file_attr = params
        .memo
        .file()
        .map(|file| html_attr("data-memo-file", file))
        .unwrap_or_default();
```

This keeps `Some(file)` on the existing `html_attr` / `html_escape` path and makes `None` contribute no attribute text to `<html>`.

- [ ] **Step 4: Run the targeted tests and verify they pass**

Run:

```bash
cargo test template::page::tests::test_メモfileがnoneの場合data_memo_file属性を出力しない
cargo test template::page::tests::test_メモuiが描画される
```

Expected:

- `test_メモfileがnoneの場合data_memo_file属性を出力しない` PASS
- `test_メモuiが描画される` PASS and still verifies `data-memo-file="README.md"`

- [ ] **Step 5: Run template module tests**

Run:

```bash
cargo test template::page
```

Expected: PASS. This confirms the change did not break page rendering, memo degraded UI, sidebar rendering, or embedded asset expectations covered by `page.rs` tests.

- [ ] **Step 6: Run full project verification**

Run:

```bash
cargo test --all-targets --all-features
./verify.sh
```

Expected: both commands PASS.

- [ ] **Step 7: Commit implementation**

Run:

```bash
git add src/template/page.rs
git commit -m "fix: data-memo-fileをNone時に省略"
```

Expected: one focused implementation commit after the existing spec commit.

## Security Notes

- `Some(file)` continues to pass through `html_attr` and `html_escape`, so attribute escaping is preserved.
- `None` no longer emits an empty file identifier, reducing the chance that future DOM code treats `""` as a real memo file target.
- No external text is newly executed, parsed as policy, sent to a shell, or assigned to a new DOM sink.
- Host / Origin validation, CSP, HTML sanitization, path validation, and file-size limits are outside the change surface.

## Rollback

Revert the implementation commit. The old behavior is restored by changing `unwrap_or_default()` back to `unwrap_or_else(|| html_attr("data-memo-file", ""))` and removing the new unit test. No data migration is needed because API payloads and saved memo files are unchanged.

## Completion Report Checklist

When implementation finishes, report:

- Changed files and rough line impact.
- Dependent files checked but not changed: `src/template/message.rs`, `src/template/assets/js/*`.
- Verification results for targeted tests, `cargo test --all-targets --all-features`, and `./verify.sh`.
- Residual risk, especially whether verification was partial or blocked.
