# Markdown Profile Options Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Markdown parser options を共通 profile API に集約し、表示・TOC は Render profile、検索は Search profile を使うことをコードとテストで明示する。

**Architecture:** `src/markdown.rs` に `MarkdownProfile` と `markdown_options()` を追加する。Search profile は Render profile を土台に検索専用拡張を足し、renderer と search はローカル option 関数を持たない。TOC は既存どおり `render_document()` 由来なので Render profile に従う。

**Tech Stack:** Rust, pulldown-cmark `Options`, cargo test, repository `./verify.sh`

---

## File Structure

- Create: `src/markdown.rs`
  - Markdown 方言 profile と `pulldown_cmark::Options` 構築を担当する。
  - profile 差分の単体テストを同じファイルに置く。
- Modify: `src/lib.rs`
  - `pub(crate) mod markdown;` を追加し、crate 内の renderer/server から共通 profile を使えるようにする。
- Modify: `src/renderer/mod.rs`
  - 既存の private `markdown_options()` と `pulldown_cmark::Options` import を削除する。
- Modify: `src/renderer/render.rs`
  - `crate::markdown::{markdown_options, MarkdownProfile}` を import し、parser 作成時に `MarkdownProfile::Render` を指定する。
- Modify: `src/server/files/search.rs`
  - 既存の private `markdown_options()` と `pulldown_cmark::Options` import を削除する。
  - `crate::markdown::{markdown_options, MarkdownProfile}` を import し、parser 作成時に `MarkdownProfile::Search` を指定する。
  - 検索 profile の現行挙動を示す境界テストを追加する。
- Modify: `tests/renderer_test.rs`
  - 表示・TOC が footnote/heading attributes を表示拡張として扱わないことを固定するテストを追加する。

---

### Task 1: 共通 Markdown profile API を追加する

**Files:**
- Create: `src/markdown.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write the failing profile tests**

Create `src/markdown.rs` with the tests first:

```rust
use pulldown_cmark::Options;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_contains_all(options: Options, expected: &[Options]) {
        for option in expected {
            assert!(
                options.contains(*option),
                "expected options to contain {:?}",
                option
            );
        }
    }

    #[test]
    fn test_render_profileは表示用gfm_subsetだけを有効化する() {
        let options = markdown_options(MarkdownProfile::Render);

        assert_contains_all(
            options,
            &[
                Options::ENABLE_TABLES,
                Options::ENABLE_TASKLISTS,
                Options::ENABLE_STRIKETHROUGH,
            ],
        );
        assert!(!options.contains(Options::ENABLE_FOOTNOTES));
        assert!(!options.contains(Options::ENABLE_HEADING_ATTRIBUTES));
        assert!(!options.contains(Options::ENABLE_GFM));
    }

    #[test]
    fn test_search_profileはrender_profileの上位互換として検索用拡張を有効化する() {
        let render_options = markdown_options(MarkdownProfile::Render);
        let search_options = markdown_options(MarkdownProfile::Search);

        assert_eq!(search_options & render_options, render_options);
        assert_contains_all(
            search_options,
            &[
                Options::ENABLE_TABLES,
                Options::ENABLE_TASKLISTS,
                Options::ENABLE_STRIKETHROUGH,
                Options::ENABLE_FOOTNOTES,
                Options::ENABLE_HEADING_ATTRIBUTES,
                Options::ENABLE_GFM,
            ],
        );
    }
}
```

- [ ] **Step 2: Register the module and run tests to verify they fail**

Modify `src/lib.rs`:

```rust
pub mod cli;
pub(crate) mod markdown;
pub mod renderer;
pub mod server;
pub mod template;
pub use renderer::toc;
pub mod watcher;
```

Run:

```bash
cargo test --lib markdown::tests
```

Expected: FAIL with unresolved items such as `cannot find function markdown_options` and `cannot find type MarkdownProfile`.

- [ ] **Step 3: Implement the minimal profile API**

Replace the top of `src/markdown.rs` with the implementation plus the tests:

```rust
use pulldown_cmark::Options;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownProfile {
    Render,
    Search,
}

pub(crate) fn markdown_options(profile: MarkdownProfile) -> Options {
    match profile {
        MarkdownProfile::Render => render_options(),
        MarkdownProfile::Search => {
            let mut options = render_options();
            options.insert(Options::ENABLE_FOOTNOTES);
            options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
            options.insert(Options::ENABLE_GFM);
            options
        }
    }
}

fn render_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_contains_all(options: Options, expected: &[Options]) {
        for option in expected {
            assert!(
                options.contains(*option),
                "expected options to contain {:?}",
                option
            );
        }
    }

    #[test]
    fn test_render_profileは表示用gfm_subsetだけを有効化する() {
        let options = markdown_options(MarkdownProfile::Render);

        assert_contains_all(
            options,
            &[
                Options::ENABLE_TABLES,
                Options::ENABLE_TASKLISTS,
                Options::ENABLE_STRIKETHROUGH,
            ],
        );
        assert!(!options.contains(Options::ENABLE_FOOTNOTES));
        assert!(!options.contains(Options::ENABLE_HEADING_ATTRIBUTES));
        assert!(!options.contains(Options::ENABLE_GFM));
    }

    #[test]
    fn test_search_profileはrender_profileの上位互換として検索用拡張を有効化する() {
        let render_options = markdown_options(MarkdownProfile::Render);
        let search_options = markdown_options(MarkdownProfile::Search);

        assert_eq!(search_options & render_options, render_options);
        assert_contains_all(
            search_options,
            &[
                Options::ENABLE_TABLES,
                Options::ENABLE_TASKLISTS,
                Options::ENABLE_STRIKETHROUGH,
                Options::ENABLE_FOOTNOTES,
                Options::ENABLE_HEADING_ATTRIBUTES,
                Options::ENABLE_GFM,
            ],
        );
    }
}
```

- [ ] **Step 4: Run profile tests to verify they pass**

Run:

```bash
cargo test --lib markdown::tests
```

Expected: PASS for both `markdown::tests::*` tests.

- [ ] **Step 5: Commit Task 1**

```bash
git add src/lib.rs src/markdown.rs
git commit -m "refactor: Markdown方言profileを追加"
```

---

### Task 2: renderer を Render profile に差し替える

**Files:**
- Modify: `src/renderer/mod.rs`
- Modify: `src/renderer/render.rs`
- Modify: `tests/renderer_test.rs`

- [ ] **Step 1: Write failing renderer boundary tests**

Append these tests to `tests/renderer_test.rs`:

```rust
#[test]
fn test_render_profileは脚注定義を表示用ブロックとして扱わない() {
    let md = "本文です。[^note]\n\n[^note]: 検索専用の脚注本文";
    let document = render_document(md);
    let html = normalize_source_markup(document.content.as_str());

    assert!(html.contains("<p>本文です。[^note]</p>"));
    assert!(html.contains("<p>[^note]: 検索専用の脚注本文</p>"));
    assert!(document.toc.as_str().is_empty());
}

#[test]
fn test_render_profileはheading_attributesを表示idに採用しない() {
    let md = "# 表示見出し {#custom-id}";
    let document = render_document(md);

    assert!(document.content.as_str().contains(r#"id="表示見出し-custom-id""#));
    assert!(!document.content.as_str().contains(r#"id="custom-id""#));
    assert!(document.toc.as_str().contains(r##"href="#表示見出し-custom-id""##));
    assert!(!document.toc.as_str().contains(r##"href="#custom-id""##));
}
```

- [ ] **Step 2: Run tests to verify current renderer behavior is explicit**

Run:

```bash
cargo test --test renderer_test test_render_profile
```

Expected: PASS. The footnote definition remains a literal paragraph because Render profile does not enable footnote parsing. Do not enable footnotes in Render profile.

- [ ] **Step 3: Replace renderer option imports**

Modify the imports in `src/renderer/mod.rs` by removing:

```rust
use pulldown_cmark::Options;
```

Delete this function from `src/renderer/mod.rs`:

```rust
fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options
}
```

Modify the imports in `src/renderer/render.rs`.

Replace:

```rust
use super::{
    generate_unique_id, markdown_options, slugify, syntax_set, HeadingInfo, SanitizedHtml,
};
```

With:

```rust
use crate::markdown::{markdown_options, MarkdownProfile};

use super::{
    generate_unique_id, slugify, syntax_set, HeadingInfo, SanitizedHtml,
};
```

Replace the parser line in `src/renderer/render.rs`:

```rust
let parser = Parser::new_ext(input, markdown_options()).into_offset_iter();
```

With:

```rust
let parser = Parser::new_ext(input, markdown_options(MarkdownProfile::Render)).into_offset_iter();
```

- [ ] **Step 4: Run renderer tests**

Run:

```bash
cargo test --test renderer_test test_render_profile
cargo test --test renderer_test test_gfmテーブル
cargo test --test renderer_test test_タスクリスト
cargo test --test renderer_test test_取消線
cargo test --test toc_test test_generate_tocはrender_documentのtocと一致する
```

Expected: targeted renderer and TOC tests PASS.

- [ ] **Step 5: Commit Task 2**

```bash
git add src/renderer/mod.rs src/renderer/render.rs tests/renderer_test.rs
git commit -m "refactor: rendererでMarkdown Render profileを使う"
```

---

### Task 3: search を Search profile に差し替える

**Files:**
- Modify: `src/server/files/search.rs`

- [ ] **Step 1: Write failing/guard search tests**

Add these tests inside `#[cfg(test)] mod tests` in `src/server/files/search.rs`:

```rust
#[test]
fn test_search_profileは脚注定義本文を検索ブロック化する() {
    let blocks = extract_search_blocks(
        "本文です。[^note]\n\n[^note]: 検索専用の脚注本文",
    );

    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].text, "本文です。");
    assert_eq!(blocks[1].text, "検索専用の脚注本文");
}

#[test]
fn test_search_profileはheading_attributes付き見出しの本文を検索対象にする() {
    let blocks = extract_search_blocks("# 表示見出し {#custom-id}");

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].text, "表示見出し");
}

#[test]
fn test_search_profileでもraw_htmlとinline_htmlは検索対象にしない() {
    let blocks = extract_search_blocks(
        "<section>hidden html</section>\n\n本文 visible <span>hidden inline</span>",
    );

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].text, "本文 visible");
}
```

- [ ] **Step 2: Run search tests before refactor**

Run:

```bash
cargo test --lib server::files::search::tests::test_search_profile
```

Expected: PASS. These tests document the current Search profile behavior before moving the option construction.

- [ ] **Step 3: Replace search option imports and function**

Modify the imports at the top of `src/server/files/search.rs`.

Replace:

```rust
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
```

With:

```rust
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::markdown::{markdown_options, MarkdownProfile};
```

Delete this function from `src/server/files/search.rs`:

```rust
fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    options.insert(Options::ENABLE_GFM);
    options
}
```

Replace the parser line:

```rust
for event in Parser::new_ext(markdown, markdown_options()) {
```

With:

```rust
for event in Parser::new_ext(markdown, markdown_options(MarkdownProfile::Search)) {
```

- [ ] **Step 4: Run search tests**

Run:

```bash
cargo test --lib server::files::search::tests
```

Expected: all `server::files::search::tests::*` tests PASS.

- [ ] **Step 5: Commit Task 3**

```bash
git add src/server/files/search.rs
git commit -m "refactor: 検索でMarkdown Search profileを使う"
```

---

### Task 4: 統合検証と TODO 完了更新

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Run full Rust tests**

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

Expected: PASS for format, clippy, and tests.

- [ ] **Step 3: Mark TODO item complete**

Modify `docs/todo/TODO.md`.

Replace:

```markdown
- [ ] Markdown 方言オプションを共通化し、表示・TOC・検索の差分を明示する
```

With:

```markdown
- [x] Markdown 方言オプションを共通化し、表示・TOC・検索の差分を明示する
```

Do not edit unrelated TODO items.

- [ ] **Step 4: Run a focused docs/status check**

Run:

```bash
git diff -- docs/todo/TODO.md
git status --short
```

Expected: only the intended TODO checkbox change remains uncommitted after prior code commits.

- [ ] **Step 5: Commit Task 4**

```bash
git add docs/todo/TODO.md
git commit -m "docs: Markdown方言profile共通化TODOを完了"
```

---

## Final Verification

- [ ] Run:

```bash
git status --short --branch
```

Expected: clean working tree on the implementation branch.

- [ ] Run:

```bash
git log --oneline -4
```

Expected: recent commits include:

```text
docs: Markdown方言profile共通化TODOを完了
refactor: 検索でMarkdown Search profileを使う
refactor: rendererでMarkdown Render profileを使う
refactor: Markdown方言profileを追加
```

- [ ] Report changed files, verification results, dependent files, residual risk, and rollback path.

## Rollback Path

Revert the implementation commits in reverse order:

```bash
git revert <todo-commit>
git revert <search-commit>
git revert <renderer-commit>
git revert <profile-api-commit>
```

This returns renderer/search to their local option functions and removes `src/markdown.rs`.
