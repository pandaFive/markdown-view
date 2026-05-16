# Syntax Theme CSS Fallback No UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `syntax_theme_css` 失敗時に UI fallback CSS を注入せず、warn log と空文字戻り値で構文ハイライト無効化を表す。

**Architecture:** 失敗時の CSS 生成は `src/renderer/mod.rs` に閉じ、テーマ解決失敗と syntect CSS 生成失敗の両方を `String::new()` に揃える。`src/template/assets.rs` の `combined_css("")` と `csp_hash_sources` の既存契約に合流させ、ベース CSS と CSP hash の関係は変更しない。

**Tech Stack:** Rust, syntect, cargo test, repository `./verify.sh`

---

## File Structure

- Modify: `tests/renderer_test.rs`
  - 無効テーマ時に `syntax_theme_css` が空文字を返し、`body::before` や通知文言を含まないことを固定する。
- Modify: `src/renderer/mod.rs`
  - `syntax_theme_css` の失敗経路を `String::new()` に変更する。
  - 無効な明示テーマ名の warn log は `resolve_theme` に寄せ、`syntax_theme_css` 側の汎用 warn と二重化させない。
  - `highlight_disabled_notice_css()` を削除する。
  - doc コメントで「空文字は構文ハイライト無効、UI 通知 CSS は注入しない」契約を明記する。
- Modify: `docs/todo/TODO.md`
  - Medium Priority の該当項目を Done Summary へ移す。
- Read-only dependency: `src/template/assets.rs`
  - `combined_css("")` と `csp_hash_sources` の既存テストで CSP 整合性を確認する。
- Read-only dependency: `src/template/assets/css/base.css`
  - 既存 `body::before` 背景レイヤーが fallback CSS に上書きされなくなる前提の確認対象。

## Task 1: 無効テーマ時の契約テストを追加

**Files:**
- Modify: `tests/renderer_test.rs`
- Test: `tests/renderer_test.rs`

- [ ] **Step 1: Write the failing renderer test**

In `tests/renderer_test.rs`, place this test after `test_syntax_theme_css_noneはデフォルトテーマで非空cssを返す`:

```rust
#[test]
fn test_syntax_theme_css_無効テーマは空文字を返しui_cssを注入しない() {
    let css = syntax_theme_css(Some("nonexistent-theme"));

    assert_eq!(css, "");
    assert!(
        !css.contains("body::before"),
        "無効テーマ時に global pseudo-element CSS を注入してはいけない"
    );
    assert!(
        !css.contains("構文ハイライトを無効化しました"),
        "無効テーマ時に画面通知 CSS を注入してはいけない"
    );
}
```

- [ ] **Step 2: Run the new test and verify it fails**

Run:

```bash
cargo test --all-targets --all-features test_syntax_theme_css_無効テーマは空文字を返しui_cssを注入しない -- --nocapture
```

Expected:

```text
FAILED
assertion `left == right` failed
```

The failure should show that the current implementation returns fallback CSS instead of `""`.

## Task 2: `syntax_theme_css` の失敗経路を空文字へ変更

**Files:**
- Modify: `src/renderer/mod.rs`
- Test: `tests/renderer_test.rs`

- [ ] **Step 1: Replace the `syntax_theme_css` doc comment and failure returns**

In `src/renderer/mod.rs`, replace the current `syntax_theme_css` function and delete `highlight_disabled_notice_css()` so this section reads:

```rust
/// syntectテーマからクラスベースのCSSを生成する
///
/// テーマが見つからない場合やCSS生成に失敗した場合は空文字列を返す。
/// 空文字列の場合、構文ハイライトは無効化される。
/// 失敗時は warn log で通知し、画面上の通知 CSS は注入しない。
pub fn syntax_theme_css(theme_name: Option<&str>) -> String {
    let ts = theme_set();
    let Some(theme) = resolve_theme(ts, theme_name) else {
        if theme_name.is_none() {
            tracing::warn!(
                "[markdown-view] テーマが見つからないため構文ハイライトCSSを生成できません"
            );
        }
        return String::new();
    };

    match css_for_theme_with_class_style(theme, ClassStyle::SpacedPrefixed { prefix: "syn-" }) {
        Ok(css) => css,
        Err(e) => {
            tracing::warn!(
                "[markdown-view] 構文ハイライトCSS生成に失敗したため無効化します: {}",
                e
            );
            String::new()
        }
    }
}
```

Also update `resolve_theme` so an invalid explicit theme logs the theme name and available themes, then returns `None` instead of falling back to the default theme:

```rust
if let Some(name) = theme_name {
    if let Some(theme) = theme_set.themes.get(name) {
        return Some(theme);
    }
    let available: Vec<&str> = theme_set.themes.keys().map(|s| s.as_str()).collect();
    tracing::warn!(
        "[markdown-view] 警告: テーマ '{}' が見つからないため構文ハイライトCSSを生成できません。構文ハイライトを無効化します。利用可能: {:?}",
        name,
        available
    );
    return None;
}
```

- [ ] **Step 2: Run the focused renderer test and verify it passes**

Run:

```bash
cargo test --all-targets --all-features test_syntax_theme_css_無効テーマは空文字を返しui_cssを注入しない -- --nocapture
```

Expected:

```text
test result: ok
```

- [ ] **Step 3: Run adjacent renderer theme tests**

Run:

```bash
cargo test --all-targets --all-features test_テーマ指定でハイライト出力が変わる
cargo test --all-targets --all-features test_syntax_theme_css_noneはデフォルトテーマで非空cssを返す
cargo test --all-targets --all-features test_有効なテーマ名の検証が成功する
cargo test --all-targets --all-features test_無効なテーマ名の検証が利用可能テーマ一覧を返す
```

Expected:

```text
test result: ok
```

Each command should end with `test result: ok`.

## Task 3: CSP と CSS 結合契約を確認

**Files:**
- Read-only dependency: `src/template/assets.rs`
- Test: `src/template/assets.rs`

- [ ] **Step 1: Run the template asset tests**

Run:

```bash
cargo test --all-targets --all-features template::assets -- --nocapture
```

Expected:

```text
test result: ok
```

This confirms `combined_css("")` still returns base CSS only, and `csp_hash_sources` still hashes the CSS that `render_page` embeds.

## Task 4: TODO を完了整理する

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Remove the Medium Priority item from the active list**

In `docs/todo/TODO.md`, remove this active item from `## Medium Priority`:

```markdown
- [ ] CSP/syntax_theme_css フォールバック CSS の副作用設計判断を doc 化
  - ファイル: `src/renderer/mod.rs` L91-108, `src/template/assets.rs` L42-61
  - 現状: `syntax_theme_css` 失敗時に `highlight_disabled_notice_css()`（`body::before` グローバル CSS）を返し、`combined_css` に連結される。CSP ハッシュは fallback ベースで再計算されるため整合性は保たれるが、Markdown 側で `body::before` を期待する CSS が無いという暗黙前提がドキュメントに無い
  - 対応: `body::before` 衝突を許容しない旨を doc コメントに明記。または fallback CSS のセレクタを `.markdown-view-fallback-notice` 等の局所スコープに変更する
  - 昇格理由: CSP と fallback CSS の契約を明示し、将来の renderer/template 変更時の判断材料にするため Medium とする
  - 由来: アーキテクチャレビュー (2026-04-30)
```

- [ ] **Step 2: Add this Done Summary entry at the top of `## Done Summary`**

Insert this entry immediately after the `## Done Summary` heading:

```markdown
- [x] CSP/syntax_theme_css フォールバック CSS の副作用設計判断を doc 化
  - 完了根拠: `syntax_theme_css` のテーマ解決失敗時と syntect CSS 生成失敗時は warn log を残して空文字を返す契約に整理し、画面上の fallback 通知 CSS は注入しない方針にした。これにより `combined_css("")` の既存契約に合流し、ページへ埋め込まれる CSS はベース CSS のみになる。既存 `base.css` の `body::before` 背景レイヤーを fallback CSS で上書きしないことを、無効テーマ時の unit test で固定した。CSP hash は実際に埋め込まれる CSS から計算する既存方式を維持している。
```

- [ ] **Step 3: Inspect the edited TODO file**

Run:

```bash
sed -n '1,80p' docs/todo/TODO.md
```

Expected:

```text
## High Priority
## Medium Priority
## Done Summary
- [x] CSP/syntax_theme_css フォールバック CSS の副作用設計判断を doc 化
```

The active Medium item should no longer appear before `## Done Summary`.

## Task 5: Full verification and commit

**Files:**
- Modify: `src/renderer/mod.rs`
- Modify: `tests/renderer_test.rs`
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Run full repository verification**

Run:

```bash
./verify.sh
```

Expected:

```text
All checks passed
```

If the script uses different success wording, accept exit code 0 with completed format, clippy, and test stages.

- [ ] **Step 2: Review the final diff**

Run:

```bash
git diff -- src/renderer/mod.rs tests/renderer_test.rs docs/todo/TODO.md
```

Expected:

- `highlight_disabled_notice_css()` is removed.
- `syntax_theme_css` returns `String::new()` on both failure paths.
- Invalid explicit theme names log once in `resolve_theme` and do not also emit the generic `syntax_theme_css` warning.
- One renderer test covers invalid theme fallback behavior.
- The TODO item moved from active Medium Priority to Done Summary.

- [ ] **Step 3: Commit the implementation**

Run:

```bash
git add src/renderer/mod.rs tests/renderer_test.rs docs/todo/TODO.md
git commit -m "fix: syntax_theme_css失敗時のUI CSS注入をやめる"
```

Expected:

```text
[branch-name <commit>] fix: syntax_theme_css失敗時のUI CSS注入をやめる
```

## Self-Review

- Spec coverage: 失敗時の空文字戻り値、UI CSS 非注入、CSP hash 既存方式維持、TODO 完了整理、`./verify.sh` 実行を各タスクで扱う。
- Placeholder scan: 未確定の実装指示はない。コマンド、対象ファイル、期待結果、追加テスト本文、置換する関数本文を明記している。
- Type consistency: `syntax_theme_css` の戻り値は既存どおり `String`。失敗時は `String::new()` に統一し、`combined_css("")` の既存契約に接続する。
