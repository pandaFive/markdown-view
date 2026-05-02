# HeadingInfo Single-Pass Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 本文 HTML の見出し `id` と TOC の `href` を同じ `HeadingInfo` から生成し、サーバーの通常表示経路では render/toc 用の Markdown 解析を単一パスにする。

**Architecture:** `render_document(input)` を追加し、1 回の pulldown-cmark 走査で `SanitizedHtml` と `Vec<HeadingInfo>` を作る。TOC はその `HeadingInfo` から生成し、既存の `render_markdown` / `toc::generate_toc` / `extract_headings` は互換 wrapper として維持する。

**Tech Stack:** Rust, pulldown-cmark, syntect, axum server update path, `cargo test`, `./verify.sh`

---

## File Structure

- Modify: `src/renderer/mod.rs`
  - `RenderedDocument` と `render_document` を追加する。
  - `extract_headings` の独自 parser 実装を削除し、統合 API の wrapper にする。
  - `render_markdown` は既存互換の wrapper として残す。
- Modify: `src/renderer/render.rs`
  - 既存 HTML 生成 loop に `Vec<HeadingInfo>` 収集を追加する。
  - `RenderOutput { content, headings }` を返す。
- Modify: `src/renderer/state.rs`
  - 見出し終了前に level を読める `heading_level()` を追加する。
- Modify: `src/renderer/toc.rs`
  - `HeadingInfo` から TOC を作る関数を renderer 内から呼べるようにする。
  - `generate_toc` は `render_document(input).toc` の wrapper にする。
- Modify: `src/server/files/content.rs`
  - `read_and_render_file()` を `render_document(&markdown)` 直呼びにする。
- Modify: `tests/renderer_test.rs`
  - `render_document` の境界テストを追加する。
  - 既存の render/toc 一致テストを統合 API 前提で強める。

## Task 1: Failing Tests For Shared HeadingInfo

**Files:**
- Modify: `tests/renderer_test.rs`

- [ ] **Step 1: Add `render_document` import**

Change the top import to include `render_document`:

```rust
use markdown_view::renderer::{
    generate_unique_id, render_document, render_markdown, slugify, syntax_theme_css, validate_theme,
};
```

- [ ] **Step 2: Write failing tests for `render_document`**

Append these tests near the existing heading ID / TOC consistency tests in `tests/renderer_test.rs`:

```rust
#[test]
fn test_render_documentは見出し画像code_softbreakで本文とtocのidを共有する() {
    let md = concat!(
        "# ![logo](x.png) Title `code`\n",
        "continued\n",
        "\n",
        "# ![logo](x.png) Title `code` continued\n",
    );

    let document = render_document(md);

    assert_eq!(document.headings.len(), 2);
    assert_eq!(document.headings[0].text, "Title code continued");
    assert_eq!(document.headings[0].id, "title-code-continued");
    assert_eq!(document.headings[1].id, "title-code-continued-1");
    assert!(document.content.as_str().contains(r##"id="title-code-continued""##));
    assert!(document.content.as_str().contains(r##"id="title-code-continued-1""##));
    assert!(document.toc.as_str().contains(r##"href="#title-code-continued""##));
    assert!(document.toc.as_str().contains(r##"href="#title-code-continued-1""##));
    assert!(!document.toc.as_str().contains("logo-title"));
}

#[test]
fn test_render_documentはhardbreak見出しでも本文とtocのidを共有する() {
    let md = "First  \nSecond\n====";
    let document = render_document(md);

    assert_eq!(document.headings.len(), 1);
    assert_eq!(document.headings[0].text, "First Second");
    assert_eq!(document.headings[0].id, "first-second");
    assert!(document.content.as_str().contains(r##"id="first-second""##));
    assert!(document.toc.as_str().contains(r##"href="#first-second""##));
}

#[test]
fn test_render_documentはraw_htmlを破棄しtocをescapeする() {
    let md = "# Hello & World\n\n<script>alert(1)</script>";
    let document = render_document(md);

    assert!(!document.content.as_str().contains("<script>"));
    assert!(!document.toc.as_str().contains("<script>"));
    assert!(document.toc.as_str().contains("Hello &amp; World"));
    assert!(document.toc.as_str().contains(r##"href="#hello-world""##));
}
```

- [ ] **Step 3: Run renderer tests and confirm the new API is missing**

Run:

```bash
cargo test --test renderer_test render_document
```

Expected: FAIL with an unresolved import or missing function error for `render_document`.

- [ ] **Step 4: Commit the failing tests**

```bash
git add tests/renderer_test.rs
git commit -m "test: 見出しID共有の境界テストを追加"
```

## Task 2: Add Single-Pass Render Output

**Files:**
- Modify: `src/renderer/state.rs`
- Modify: `src/renderer/render.rs`
- Modify: `src/renderer/mod.rs`
- Modify: `src/renderer/toc.rs`

- [ ] **Step 1: Add heading level accessor**

In `src/renderer/state.rs`, add this method next to `heading_plain_text()`:

```rust
    pub(super) fn heading_level(&self) -> Option<u8> {
        self.heading.as_ref().map(|heading| heading.level)
    }
```

- [ ] **Step 2: Change render output shape**

In `src/renderer/render.rs`, change imports and add `RenderOutput`:

```rust
use super::{generate_unique_id, markdown_options, slugify, syntax_set, HeadingInfo, SanitizedHtml};

pub(super) struct RenderOutput {
    pub(super) content: SanitizedHtml,
    pub(super) headings: Vec<HeadingInfo>,
}
```

Then replace `pub(super) fn render(input: &str) -> SanitizedHtml` with:

```rust
pub(super) fn render(input: &str) -> RenderOutput {
    let line_lookup = LineLookup::new(input);
    let syntax_set = syntax_set();
    let mut id_counts = HashMap::new();
    let mut state = RenderState::new();
    let mut headings = Vec::new();

    let parser = Parser::new_ext(input, markdown_options()).into_offset_iter();
    for (event, range) in parser {
        dispatch_event(
            event,
            range,
            &line_lookup,
            syntax_set,
            &mut id_counts,
            &mut headings,
            &mut state,
        );
    }

    RenderOutput {
        content: SanitizedHtml::from_sanitized_html(state.into_html()),
        headings,
    }
}
```

- [ ] **Step 3: Thread headings through dispatch**

In `src/renderer/render.rs`, update `dispatch_event` and `handle_end` signatures:

```rust
fn dispatch_event(
    event: Event<'_>,
    range: Range<usize>,
    line_lookup: &LineLookup,
    syntax_set: &SyntaxSet,
    id_counts: &mut HashMap<String, usize>,
    headings: &mut Vec<HeadingInfo>,
    state: &mut RenderState,
)
```

```rust
fn handle_end(
    tag: TagEnd,
    range: Range<usize>,
    line_lookup: &LineLookup,
    syntax_set: &SyntaxSet,
    id_counts: &mut HashMap<String, usize>,
    headings: &mut Vec<HeadingInfo>,
    state: &mut RenderState,
)
```

Change the calls so `dispatch_event` passes `headings` into `handle_end`, and `handle_end` calls:

```rust
TagEnd::Heading(_) => handle_heading_end(line_lookup, id_counts, headings, state),
```

- [ ] **Step 4: Record `HeadingInfo` at heading end**

Replace `handle_heading_end` in `src/renderer/render.rs` with:

```rust
fn handle_heading_end(
    line_lookup: &LineLookup,
    id_counts: &mut HashMap<String, usize>,
    headings: &mut Vec<HeadingInfo>,
    state: &mut RenderState,
) {
    let text = state.heading_plain_text().to_string();
    let level = state.heading_level();
    let slug = slugify(&text);
    let id = generate_unique_id(&slug, id_counts);
    let heading_attrs = heading_line_attrs(line_lookup, state);
    if let Some(heading_html) = state.finish_heading(id.clone(), heading_attrs) {
        if let Some(level) = level {
            headings.push(HeadingInfo {
                level,
                text,
                id,
            });
        }
        state.push_html(&heading_html);
    }
}
```

- [ ] **Step 5: Add `RenderedDocument` and wrappers**

In `src/renderer/mod.rs`, remove the now-unused `pulldown_cmark::{Event, Parser, Tag, TagEnd}` import. Add `RenderedDocument` below `SanitizedHtml`:

```rust
/// Markdown変換結果一式。
///
/// `content` と `toc` は同じ parser 走査で確定した `headings` から生成される。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDocument {
    pub content: SanitizedHtml,
    pub toc: SanitizedHtml,
    pub headings: Vec<HeadingInfo>,
}
```

Replace `render_markdown` with:

```rust
pub fn render_markdown(input: &str) -> SanitizedHtml {
    render_document(input).content
}
```

Add this function near `render_markdown`:

```rust
/// Markdownテキストを HTML と TOC に変換する。
///
/// 本文見出しIDとTOCリンクは同じ `HeadingInfo` から生成される。
pub fn render_document(input: &str) -> RenderedDocument {
    if input.is_empty() {
        return RenderedDocument {
            content: SanitizedHtml::from_sanitized_html(String::new()),
            toc: SanitizedHtml::from_sanitized_html(String::new()),
            headings: Vec::new(),
        };
    }

    let rendered = render::render(input);
    let toc = toc::generate_toc_from_headings(&rendered.headings);
    RenderedDocument {
        content: rendered.content,
        toc,
        headings: rendered.headings,
    }
}
```

Replace `extract_headings` with:

```rust
/// Markdownから見出し情報を抽出する。
///
/// 互換用 API。本文 HTML と TOC を同時に必要とする経路では `render_document` を使う。
pub fn extract_headings(input: &str) -> Vec<HeadingInfo> {
    render_document(input).headings
}
```

- [ ] **Step 6: Add TOC helper from headings**

In `src/renderer/toc.rs`, change imports and add helper:

```rust
use super::{html_escape, render_document, HeadingInfo, SanitizedHtml};
```

Replace `generate_toc` with:

```rust
pub fn generate_toc(input: &str) -> SanitizedHtml {
    render_document(input).toc
}

pub(in crate::renderer) fn generate_toc_from_headings(headings: &[HeadingInfo]) -> SanitizedHtml {
    if headings.is_empty() {
        return SanitizedHtml::from_sanitized_html(String::new());
    }

    SanitizedHtml::from_sanitized_html(build_toc_html(headings))
}
```

- [ ] **Step 7: Run focused renderer tests**

Run:

```bash
cargo test --test renderer_test render_document
```

Expected: PASS.

- [ ] **Step 8: Commit renderer integration**

```bash
git add src/renderer/mod.rs src/renderer/render.rs src/renderer/state.rs src/renderer/toc.rs tests/renderer_test.rs
git commit -m "refactor: 見出しID生成をrender_documentへ統合"
```

## Task 3: Switch Server Rendering Path

**Files:**
- Modify: `src/server/files/content.rs`

- [ ] **Step 1: Replace imports**

In `src/server/files/content.rs`, replace:

```rust
use crate::renderer::render_markdown;
use crate::toc::generate_toc;
```

with:

```rust
use crate::renderer::render_document;
```

- [ ] **Step 2: Use `render_document` in `read_and_render_file`**

Replace `read_and_render_file()` with:

```rust
/// ファイルを読み込み、Markdown→HTML変換とTOC生成を行いUpdateMessageとして返す
async fn read_and_render_file(file_path: &Path) -> Result<UpdateMessage, ReadMarkdownError> {
    let markdown = read_markdown_with_limit(file_path).await?;
    let document = render_document(&markdown);
    Ok(UpdateMessage::new(document.content, document.toc, None))
}
```

- [ ] **Step 3: Run content-related tests**

Run:

```bash
cargo test --all-targets --all-features server::files::content
```

Expected: PASS or zero tests matched with no compile errors. The important check here is that server code compiles with the new import and `UpdateMessage::new` call.

- [ ] **Step 4: Run integration test smoke for API content**

Run:

```bash
cargo test --test integration_test api_content
```

Expected: PASS or zero tests matched with no compile errors. If zero tests match, continue to the full verification task.

- [ ] **Step 5: Commit server path switch**

```bash
git add src/server/files/content.rs
git commit -m "refactor: サーバー描画経路でrender_documentを使う"
```

## Task 4: Compatibility And Regression Coverage

**Files:**
- Modify: `tests/toc_test.rs`
- Modify: `tests/renderer_test.rs`

- [ ] **Step 1: Add compatibility test for `extract_headings`**

In `tests/renderer_test.rs`, extend the top import:

```rust
use markdown_view::renderer::{
    extract_headings, generate_unique_id, render_document, render_markdown, slugify,
    syntax_theme_css, validate_theme,
};
```

Add this test near the `render_document` tests:

```rust
#[test]
fn test_extract_headingsはrender_documentのheadingsと一致する() {
    let md = "# A `code`\n\n## ![logo](x.png) B\n\n# A `code`";

    let document = render_document(md);
    let headings = extract_headings(md);

    assert_eq!(headings, document.headings);
    assert_eq!(headings[0].id, "a-code");
    assert_eq!(headings[1].id, "b");
    assert_eq!(headings[2].id, "a-code-1");
}
```

- [ ] **Step 2: Add TOC wrapper compatibility test**

Append this test to `tests/toc_test.rs`:

```rust
#[test]
fn test_generate_tocはrender_documentのtocと一致する() {
    let md = "# A `code`\n\n## ![logo](x.png) B\n\n# A `code`";

    let toc = generate_toc(md);
    let document = markdown_view::renderer::render_document(md);

    assert_eq!(toc, document.toc);
    assert!(toc.as_str().contains(r##"href="#a-code""##));
    assert!(toc.as_str().contains(r##"href="#b""##));
    assert!(toc.as_str().contains(r##"href="#a-code-1""##));
}
```

- [ ] **Step 3: Run renderer and TOC tests**

Run:

```bash
cargo test --test renderer_test --test toc_test
```

Expected: PASS.

- [ ] **Step 4: Commit compatibility coverage**

```bash
git add tests/renderer_test.rs tests/toc_test.rs
git commit -m "test: 見出し抽出とtoc互換性を固定"
```

## Task 5: Full Verification And TODO Closeout

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Run formatter check**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS. If it fails, run `cargo fmt --all`, inspect the diff, then rerun the check.

- [ ] **Step 2: Run clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 3: Run full test suite**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 4: Run repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 5: Mark the TODO item complete**

In `docs/todo/TODO.md`, change the target item from:

```markdown
- [ ] 見出し ID 生成を単一パス化し render と toc で `HeadingInfo` を共有する
```

to:

```markdown
- [x] 見出し ID 生成を単一パス化し render と toc で `HeadingInfo` を共有する
```

Do not edit unrelated TODO items.

- [ ] **Step 6: Commit verification closeout**

```bash
git add docs/todo/TODO.md
git commit -m "docs: 見出しID単一パス化TODOを完了"
```

## Rollback Path

If implementation fails or behavior regresses:

```bash
git revert <task-5-commit> <task-4-commit> <task-3-commit> <task-2-commit> <task-1-commit>
```

The public API wrappers keep call sites stable, so rollback should be limited to renderer, server content, tests, and the TODO checkbox.

## Security Notes

- Do not expose `SanitizedHtml::from_sanitized_html` outside `crate::renderer`.
- Do not change raw HTML / inline HTML handling; those events must remain discarded.
- Do not weaken `sanitize_link_href` or image source sanitization.
- TOC output must continue escaping both `heading.id` and `heading.text`.
- Treat Markdown input as untrusted even when it comes from a local file.
