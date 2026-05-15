# Assets Sentinel Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** CSS/JS asset template sentinel の許可箇所と `MAX_FILE_SIZE` 表示値を単体テストで固定する。

**Architecture:** 現行 10MiB 上限の表示は維持し、置換元に最も近い `css_bundle.rs` と `inline_script.rs` の unit test を追加する。CSS は `base.css` だけ、JS は `bootstrap.js` だけを sentinel 許可ファイルとして扱い、生成後 asset に sentinel が残らないことも確認する。

**Tech Stack:** Rust unit tests, `include_str!`, existing `cargo test`, repository `./verify.sh`

---

## File Structure

- Modify: `src/template/assets/css_bundle.rs`
  - `__DARK_THEME_VARS__` の許可出現箇所をテストする。
  - `css(...)` 生成後に sentinel が残らないことをテストする。
- Modify: `src/template/assets/inline_script.rs`
  - `__MAX_FILE_SIZE_MB__` の許可出現箇所をテストする。
  - `inline_js(crate::server::MAX_FILE_SIZE)` 生成後に `MAX_FILE_SIZE` 由来の `maxFileSizeMb` が含まれ、sentinel が残らないことをテストする。
  - 非整数 MiB の上限は MB 表示を切り上げる private helper で扱う。
- Read-only dependency: `src/template/assets/css/*.css`
- Read-only dependency: `src/template/assets/js/*.js`
- Read-only dependency: `src/server/files/content.rs`
  - `MAX_FILE_SIZE` の定義元。値は変更しない。

## Task 1: CSS Sentinel 契約テスト

**Files:**
- Modify: `src/template/assets/css_bundle.rs`
- Test: `src/template/assets/css_bundle.rs`

- [ ] **Step 1: Write the failing CSS sentinel tests**

Add this test module to the end of `src/template/assets/css_bundle.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const DARK_THEME_SENTINEL: &str = "__DARK_THEME_VARS__";

    fn count_occurrences(source: &str, needle: &str) -> usize {
        source.matches(needle).count()
    }

    #[test]
    fn test_dark_theme_sentinelはbase_cssだけに存在する() {
        let allowed_base_css = include_str!("css/base.css");
        let expected_sentinel_count = count_occurrences(allowed_base_css, DARK_THEME_SENTINEL);
        let disallowed_sources = [
            ("css/sidebar.css", include_str!("css/sidebar.css")),
            ("css/content.css", include_str!("css/content.css")),
            ("css/memo.css", include_str!("css/memo.css")),
            ("css/file_tree.css", include_str!("css/file_tree.css")),
            ("css/overlay.css", include_str!("css/overlay.css")),
        ];

        assert_eq!(
            expected_sentinel_count,
            2,
            "base.css の dark theme sentinel 出現回数が変わった"
        );

        let mut listed_sentinel_count = expected_sentinel_count;
        for (path, source) in disallowed_sources {
            let source_sentinel_count = count_occurrences(source, DARK_THEME_SENTINEL);
            listed_sentinel_count += source_sentinel_count;
            assert!(
                source_sentinel_count == 0,
                "{path} に dark theme sentinel が混入している"
            );
        }

        assert_eq!(
            count_occurrences(TEMPLATE, DARK_THEME_SENTINEL),
            listed_sentinel_count,
            "CSS template include一覧とsentinel契約テストの一覧が同期していない"
        );
    }

    #[test]
    fn test_css生成後にdark_theme_sentinelが残らない() {
        let generated = TEMPLATE.replace(DARK_THEME_SENTINEL, ":root { --test-color: #fff; }");

        assert!(
            !generated.contains(DARK_THEME_SENTINEL),
            "生成済み CSS に dark theme sentinel が残っている"
        );
    }
}
```

- [ ] **Step 2: Run the CSS module tests and confirm the expected failure or pass state**

Run:

```bash
cargo test --all-targets --all-features template::assets::css_bundle -- --nocapture
```

Expected:

- If the copied test module compiles against the current file, the tests should pass because the current asset layout already satisfies the intended contract.
- If a future source file contains `__DARK_THEME_VARS__` outside `base.css`, this command should fail with a message naming the offending file.

- [ ] **Step 3: Commit the CSS test change**

Run:

```bash
git add src/template/assets/css_bundle.rs
git commit -m "test: CSS sentinel契約を固定"
```

Expected:

- Commit succeeds.
- Only `src/template/assets/css_bundle.rs` is included.

## Task 2: JS Sentinel と MAX_FILE_SIZE 表示契約テスト

**Files:**
- Modify: `src/template/assets/inline_script.rs`
- Test: `src/template/assets/inline_script.rs`

- [ ] **Step 1: Add JS sentinel helper constants inside the existing test module**

In `src/template/assets/inline_script.rs`, find the existing `#[cfg(test)] mod tests { ... }` module. Add these items near the top of the module, after the current `use` statements:

```rust
    const MAX_FILE_SIZE_SENTINEL: &str = "__MAX_FILE_SIZE_MB__";

    fn count_occurrences(source: &str, needle: &str) -> usize {
        source.matches(needle).count()
    }
```

- [ ] **Step 2: Write the failing JS sentinel location test**

Still inside the same `tests` module in `src/template/assets/inline_script.rs`, add this test:

```rust
    #[test]
    fn test_max_file_size_sentinelはbootstrap_jsだけに存在する() {
        let allowed_bootstrap_js = include_str!("js/bootstrap.js");
        let expected_sentinel_count =
            count_occurrences(allowed_bootstrap_js, MAX_FILE_SIZE_SENTINEL);
        let disallowed_sources = [
            ("js/selection.js", include_str!("js/selection.js")),
            (
                "js/content-renderer.js",
                include_str!("js/content-renderer.js"),
            ),
            (
                "js/content-enhancements.js",
                include_str!("js/content-enhancements.js"),
            ),
            (
                "js/content-navigation.js",
                include_str!("js/content-navigation.js"),
            ),
            (
                "js/document-search.js",
                include_str!("js/document-search.js"),
            ),
            (
                "js/directory-search.js",
                include_str!("js/directory-search.js"),
            ),
            (
                "js/content-controller.js",
                include_str!("js/content-controller.js"),
            ),
            ("js/memo.js", include_str!("js/memo.js")),
            ("js/fetch.js", include_str!("js/fetch.js")),
            ("js/websocket.js", include_str!("js/websocket.js")),
            ("js/sidebar.js", include_str!("js/sidebar.js")),
        ];

        assert_eq!(
            expected_sentinel_count,
            1,
            "bootstrap.js の max file size sentinel 出現回数が変わった"
        );

        let mut listed_sentinel_count = expected_sentinel_count;
        for (path, source) in disallowed_sources {
            let source_sentinel_count = count_occurrences(source, MAX_FILE_SIZE_SENTINEL);
            listed_sentinel_count += source_sentinel_count;
            assert!(
                source_sentinel_count == 0,
                "{path} に max file size sentinel が混入している"
            );
        }

        assert_eq!(
            count_occurrences(TEMPLATE, MAX_FILE_SIZE_SENTINEL),
            listed_sentinel_count,
            "JS template include一覧とsentinel契約テストの一覧が同期していない"
        );
    }
```

- [ ] **Step 3: Write the generated JS display value tests**

Add a private `file_size_display_mb(max_file_size: u64) -> String` helper that uses `div_ceil(1024 * 1024)`, and have `inline_js` call it. Still inside the same `tests` module in `src/template/assets/inline_script.rs`, add a small tree-sitter helper that reads the numeric `maxFileSizeMb` property value from generated JS, then add these tests:

```rust
    #[test]
    fn test_inline_jsはmax_file_size由来のmb値へ置換する() {
        let generated = inline_js(crate::server::MAX_FILE_SIZE);

        assert!(
            !generated.contains(MAX_FILE_SIZE_SENTINEL),
            "生成済み JS に max file size sentinel が残っている"
        );
        assert_eq!(
            max_file_size_mb_value(&generated),
            Some(file_size_display_mb(crate::server::MAX_FILE_SIZE).parse().unwrap()),
            "生成済み JS の maxFileSizeMb が MAX_FILE_SIZE 由来のMB表示になっていない"
        );
    }

    #[test]
    fn test_inline_jsは非整数mibのmax_file_sizeを切り上げ表示する() {
        let generated = inline_js(11_000_000);

        assert_eq!(
            max_file_size_mb_value(&generated),
            Some(11),
            "非整数MiBの maxFileSizeMb は過小表示を避けるため切り上げる"
        );
    }
```

- [ ] **Step 4: Run the JS module tests**

Run:

```bash
cargo test --all-targets --all-features template::assets::inline_script -- --nocapture
```

Expected:

- Existing innerHTML sink scanner tests still pass.
- New sentinel tests pass.
- `inline_js(11_000_000)` returns JS whose `maxFileSizeMb` value is `11`.
- If a future source file contains `__MAX_FILE_SIZE_MB__` outside `bootstrap.js`, this command should fail with a message naming the offending file.

- [ ] **Step 5: Commit the JS test change**

Run:

```bash
git add src/template/assets/inline_script.rs
git commit -m "test: JS sentinel契約を固定"
```

Expected:

- Commit succeeds.
- Only `src/template/assets/inline_script.rs` is included.

## Task 3: Full Verification and TODO Update

**Files:**
- Modify: `docs/todo/TODO.md`
- Verify: full repository

- [ ] **Step 1: Mark the TODO item complete**

In `docs/todo/TODO.md`, move or rewrite the item `assets バンドルの sentinel 衝突回避テストを追加` from unchecked Medium Priority to completed Done Summary. Use this completion text:

```markdown
- [x] assets バンドルの sentinel 衝突回避テストを追加
  - 完了根拠: `css_bundle.rs` で `__DARK_THEME_VARS__` が `base.css` の期待箇所以外に混入していないことを固定し、生成済み CSS に sentinel が残らないことを確認した。`inline_script.rs` では `__MAX_FILE_SIZE_MB__` が `bootstrap.js` の期待箇所以外に混入していないこと、生成済み JS に sentinel が残らず `MAX_FILE_SIZE` 由来の `maxFileSizeMb` へ置換されること、非整数 MiB の上限が過小表示を避けて切り上げられることを単体テストで固定した。現行10MiB上限の表示、CSP hash 計算、ユーザー向け文言は変更していない。
```

- [ ] **Step 2: Run formatting check**

Run:

```bash
cargo fmt --all -- --check
```

Expected:

- PASS.

- [ ] **Step 3: Run clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected:

- PASS with no warnings.

- [ ] **Step 4: Run full test suite**

Run:

```bash
cargo test --all-targets --all-features
```

Expected:

- PASS.

- [ ] **Step 5: Run repository verification script**

Run:

```bash
./verify.sh
```

Expected:

- PASS.
- This is the final required verification.

- [ ] **Step 6: Commit TODO update**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: sentinel契約TODOを完了扱いに更新"
```

Expected:

- Commit succeeds.
- Only `docs/todo/TODO.md` is included.

## Self-Review

- Spec coverage: The plan covers CSS sentinel location, generated CSS sentinel removal, JS sentinel location, generated JS sentinel removal, `MAX_FILE_SIZE` derived `maxFileSizeMb`, non-integer MiB rounding, full verification, security rationale, rollback, and TODO completion.
- Placeholder scan: No TBD, TODO, unspecified validation, or "similar to" steps are present.
- Type consistency: `TEMPLATE`, `css`, and `inline_js` are accessed from tests in their defining modules. `crate::server::MAX_FILE_SIZE` is the existing public re-export path used elsewhere in the repository.
