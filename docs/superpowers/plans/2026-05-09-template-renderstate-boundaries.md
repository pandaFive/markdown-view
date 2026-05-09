# Template / RenderState Boundaries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `template` のテスト・HTML 組み立て境界と `RenderState` の open/close 状態境界を整理し、通常 Markdown / page rendering の既存出力互換を維持する。

**Architecture:** 先に `src/template/` の責務別テスト移動と `render_page` helper 分割を行う。次に `src/renderer/state.rs` に `BlockContext` stack と `RenderStateMismatch` を導入し、`src/renderer/render.rs` は mismatch を明示的に recover する。公開 API は変えず、通常入力の HTML 出力互換をテストで固定する。

**Tech Stack:** Rust, pulldown-cmark, syntect, serde_json, cargo test, cargo clippy, `./verify.sh`

---

## File Structure

- Modify: `src/template/mod.rs`
  - Keep module declarations and `pub use`.
  - Keep only thin public contract tests.
- Modify: `src/template/page.rs`
  - Split `render_page` into focused helpers.
  - Add `html_attr(name, value)` for escaped attribute fragments.
  - Add page-level tests for shell structure, memo degraded output, sanitized content insertion, and attribute escaping.
- Modify: `src/template/tree.rs`
  - Move file tree construction and tree HTML tests here.
- Modify: `src/template/assets.rs`
  - Move asset / CSP hash tests here.
- Modify: `src/template/message.rs`
  - Keep existing memo/update JSON tests.
- Modify: `src/renderer/state.rs`
  - Replace independent `Option` fields with `contexts: Vec<BlockContext>`.
  - Add `RenderStateMismatch`.
  - Convert state finish/start methods to `Result` where they can mismatch.
- Modify: `src/renderer/render.rs`
  - Match `Result` from `RenderState` methods.
  - `warn!` and skip malformed context output in release-compatible mismatch paths.
- Modify: `tests/renderer_test.rs`
  - Keep public renderer output regression tests and add the combined state-boundary compatibility case shown in Task 4.

## Task 1: Template Tests Move To Responsible Modules

**Files:**
- Modify: `src/template/mod.rs`
- Modify: `src/template/tree.rs`
- Modify: `src/template/assets.rs`
- Modify: `src/template/page.rs`
- Test: module-local `#[cfg(test)]` tests in those files

- [ ] **Step 1: Inspect current template tests**

Run:

```bash
cargo test template --all-targets --all-features
```

Expected: PASS. If this filter runs zero tests, run:

```bash
cargo test --lib template --all-features
```

Expected: existing template-related tests pass before moving them.

- [ ] **Step 2: Move tree tests into `src/template/tree.rs`**

Move these tests from `src/template/mod.rs` into a new `#[cfg(test)] mod tests` at the bottom of `src/template/tree.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_フラットファイルリストからツリーを構築() {
        let files = vec![
            "README.md".to_string(),
            "docs/api.md".to_string(),
            "docs/guide/intro.md".to_string(),
        ];
        let tree = build_file_tree(&files);

        assert_eq!(tree.len(), 2);
        assert!(matches!(&tree[1], FileTreeNode::File { name, full_path } if name == "README.md" && full_path == "README.md"));
    }

    #[test]
    fn test_空のファイルリストからツリーを構築() {
        let files: Vec<String> = vec![];
        let tree = build_file_tree(&files);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_ファイル名のエスケープがツリーhtmlで維持される() {
        let files = vec!["A&B \"<notes>\".md".to_string()];
        let tree = build_file_tree(&files);
        let html = render_file_tree_html(&tree, None);

        assert!(html.contains("A&amp;B &quot;&lt;notes&gt;&quot;.md"));
        assert!(!html.contains("data-file=\"A&B \"<notes>\".md\""));
        assert!(html.contains("data-file=\"A&amp;B &quot;&lt;notes&gt;&quot;.md\""));
    }
}
```

Keep the existing richer assertions from `mod.rs` when moving; the snippet above shows the minimum shape and imports.

- [ ] **Step 3: Move asset/CSP tests into `src/template/assets.rs`**

Add this module to the bottom of `src/template/assets.rs`, moving equivalent tests out of `mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combined_cssはsyntax_cssを末尾に結合する() {
        let css = combined_css(".syn-test { color: red; }");

        assert!(css.contains(".syn-test { color: red; }"));
        assert!(css.len() > ".syn-test { color: red; }".len());
    }

    #[test]
    fn test_csp_hash_sourcesはscriptとstyle_hashを返す() {
        let (script_hash, style_hash) = csp_hash_sources(".syn-test { color: red; }");

        assert!(script_hash.starts_with("'sha256-"));
        assert!(script_hash.ends_with('\''));
        assert!(style_hash.starts_with("'sha256-"));
        assert!(style_hash.ends_with('\''));
        assert_ne!(script_hash, style_hash);
    }
}
```

- [ ] **Step 4: Move page rendering tests into `src/template/page.rs`**

Add test helpers and page tests at the bottom of `src/template/page.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{render_markdown, SanitizedHtml};
    use crate::toc::generate_toc;

    fn test_content() -> SanitizedHtml {
        render_markdown("content")
    }

    fn test_toc() -> SanitizedHtml {
        generate_toc("# toc")
    }

    fn test_memo() -> MemoResponse {
        MemoResponse::empty(None)
    }

    #[test]
    fn test_single_file_modeで基本shellを描画する() {
        let html = render_page(RenderPageParams {
            title: "Title",
            content: &test_content(),
            toc: &test_toc(),
            memo: &test_memo(),
            dark_mode: false,
            syntax_css: "",
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains("<html lang=\"ja\" data-theme=\"light\""));
        assert!(html.contains("<main id=\"content\""));
        assert!(html.contains("Annotations"));
        assert!(html.contains("Research Notes"));
    }

    #[test]
    fn test_directory_modeでcurrent_file属性をescapeする() {
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let files = vec!["docs/a.md".to_string()];
        let html = render_page(RenderPageParams {
            title: "T",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: true,
            syntax_css: "",
            sidebar: SidebarParams::Directory {
                directory_name: "docs",
                file_list: &files,
                current_file: Some("docs/a\" onclick=\"x.md"),
            },
        });

        assert!(html.contains("data-dir-mode=\"true\""));
        assert!(html.contains("data-current-file=\"docs/a&quot; onclick=&quot;x.md\""));
        assert!(!html.contains("data-current-file=\"docs/a\" onclick=\"x.md\""));
    }
}
```

- [ ] **Step 5: Reduce `src/template/mod.rs` tests to public contract smoke tests**

Leave `mod.rs` with imports only needed for public API tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{render_markdown, syntax_theme_css};
    use crate::toc::generate_toc;

    #[test]
    fn test_public_apiでページとcsp_hashを組み合わせられる() {
        let content = render_markdown("content");
        let toc = generate_toc("# toc");
        let memo = MemoResponse::empty(None);
        let syntax_css = syntax_theme_css(None);

        let html = render_page(RenderPageParams {
            title: "Public API",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });
        let (script_hash, style_hash) = csp_hash_sources(&syntax_css);

        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Public API"));
        assert!(script_hash.starts_with("'sha256-"));
        assert!(style_hash.starts_with("'sha256-"));
    }
}
```

- [ ] **Step 6: Run moved template tests**

Run:

```bash
cargo test --lib template --all-features
```

Expected: PASS.

- [ ] **Step 7: Commit template test move**

Run:

```bash
git add src/template/mod.rs src/template/tree.rs src/template/assets.rs src/template/page.rs
git commit -m "test: templateテストを責務別モジュールへ移動"
```

Expected: commit succeeds.

## Task 2: Split `render_page` And Centralize Attribute Escaping

**Files:**
- Modify: `src/template/page.rs`
- Test: `src/template/page.rs`

- [ ] **Step 1: Add failing attribute helper tests**

Add tests to `src/template/page.rs`:

```rust
#[test]
fn test_html_attrは属性値をescapeする() {
    assert_eq!(
        html_attr("data-file", "a\" onclick=\"x & <y>"),
        " data-file=\"a&quot; onclick=&quot;x &amp; &lt;y&gt;\""
    );
}

#[test]
fn test_contentとtocは二重escapeしない() {
    let content = SanitizedHtml::from_sanitized_html("<p><strong>ok</strong></p>".to_string());
    let toc = SanitizedHtml::from_sanitized_html("<ul><li>toc</li></ul>".to_string());
    let memo = MemoResponse::empty(None);

    let html = render_page(RenderPageParams {
        title: "Escape",
        content: &content,
        toc: &toc,
        memo: &memo,
        dark_mode: false,
        syntax_css: "",
        sidebar: SidebarParams::SingleFile,
    });

    assert!(html.contains("<p><strong>ok</strong></p>"));
    assert!(html.contains("<ul><li>toc</li></ul>"));
    assert!(!html.contains("&lt;strong&gt;ok&lt;/strong&gt;"));
}
```

Run:

```bash
cargo test --lib template::page::tests::test_html_attrは属性値をescapeする --all-features
```

Expected: FAIL because `html_attr` is not defined yet.

- [ ] **Step 2: Add `html_attr` helper**

Add near `DocumentMeta` in `src/template/page.rs`:

```rust
fn html_attr(name: &'static str, value: &str) -> String {
    format!(" {}=\"{}\"", name, html_escape(value))
}
```

- [ ] **Step 3: Split top-level document helpers**

Replace the body of `render_page` with:

```rust
pub fn render_page(params: RenderPageParams<'_>) -> String {
    let escaped_title = html_escape(params.title);
    let (dir_mode_attr, sidebar_inner, meta) =
        render_sidebar(&params.sidebar, params.toc, params.memo);
    let memo_file_attr = params
        .memo
        .file()
        .map(|file| html_attr("data-memo-file", file))
        .unwrap_or_else(|| html_attr("data-memo-file", ""));

    render_html_document(HtmlDocumentParts {
        theme: if params.dark_mode { "dark" } else { "light" },
        dir_mode_attr,
        memo_file_attr,
        title: escaped_title,
        css: combined_css(params.syntax_css),
        sidebar_inner,
        content: params.content.as_str(),
        js: inline_js(),
        mode_label: meta.mode_label,
        file_count_label: meta.file_count_label,
    })
}

struct HtmlDocumentParts {
    theme: &'static str,
    dir_mode_attr: String,
    memo_file_attr: String,
    title: String,
    css: String,
    sidebar_inner: String,
    content: String,
    js: String,
    mode_label: String,
    file_count_label: String,
}
```

Then add `render_html_document`, `render_head`, and `render_workspace_body` helpers by moving existing HTML sections without changing literal markup. Keep this exact root shape:

```rust
fn render_html_document(parts: HtmlDocumentParts) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="ja" data-theme="{theme}"{dir_mode_attr}{memo_file_attr}>
{head}
{body}
</html>"##,
        theme = parts.theme,
        dir_mode_attr = parts.dir_mode_attr,
        memo_file_attr = parts.memo_file_attr,
        head = render_head(&parts.title, &parts.css),
        body = render_workspace_body(&parts),
    )
}
```

- [ ] **Step 4: Update directory mode attribute generation**

In `render_sidebar`, replace the directory attribute construction with:

```rust
let dir_mode_attr = format!(
    "{}{}",
    html_attr("data-dir-mode", "true"),
    html_attr("data-current-file", current_file.unwrap_or(""))
);
```

Do not escape `toc.as_str()` or `memo_editor`; both are already HTML contract outputs.

- [ ] **Step 5: Run page tests**

Run:

```bash
cargo test --lib template::page --all-features
```

Expected: PASS.

- [ ] **Step 6: Run all template tests**

Run:

```bash
cargo test --lib template --all-features
```

Expected: PASS.

- [ ] **Step 7: Commit render_page split**

Run:

```bash
git add src/template/page.rs
git commit -m "refactor: render_pageのHTML組み立て境界を分割"
```

Expected: commit succeeds.

## Task 3: Introduce `BlockContext` And Mismatch Type

**Files:**
- Modify: `src/renderer/state.rs`

- [ ] **Step 1: Replace old panic/fallback tests with mismatch tests**

In `src/renderer/state.rs`, replace the existing `#[should_panic]` / release fallback tests with:

```rust
#[test]
fn test_finish_headingは開始なしならmismatchを返す() {
    let mut state = RenderState::new();

    assert!(matches!(
        state.finish_heading("heading".to_string(), String::new()),
        Err(RenderStateMismatch::ExpectedHeading)
    ));
}

#[test]
fn test_finish_code_blockは開始なしならmismatchを返す() {
    let mut state = RenderState::new();
    let syntax_set = SyntaxSet::load_defaults_newlines();

    assert!(matches!(
        state.finish_code_block(&syntax_set, String::new()),
        Err(RenderStateMismatch::ExpectedCodeBlock)
    ));
}

#[test]
fn test_finish_imageは開始なしならmismatchを返す() {
    let mut state = RenderState::new();

    assert!(matches!(
        state.finish_image(),
        Err(RenderStateMismatch::ExpectedImage)
    ));
}

#[test]
fn test_table操作はtable開始なしならmismatchを返す() {
    let mut state = RenderState::new();

    assert!(matches!(state.start_table_head(), Err(RenderStateMismatch::ExpectedTable)));
    assert!(matches!(state.finish_table_head(), Err(RenderStateMismatch::ExpectedTable)));
    assert!(matches!(state.reset_table_row(), Err(RenderStateMismatch::ExpectedTable)));
    assert!(matches!(state.table_cell_start_tag(), Err(RenderStateMismatch::ExpectedTable)));
    assert!(matches!(state.table_cell_end_tag(), Err(RenderStateMismatch::ExpectedTable)));
}
```

Run:

```bash
cargo test --lib renderer::state --all-features
```

Expected: FAIL because `RenderStateMismatch` and `Result` signatures are not implemented.

- [ ] **Step 2: Add `BlockContext` and `RenderStateMismatch`**

At the top of `src/renderer/state.rs`, change `RenderState` and add the enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenderStateMismatch {
    ExpectedHeading,
    ExpectedCodeBlock,
    ExpectedImage,
    ExpectedTable,
}

pub(super) struct RenderState {
    html_output: String,
    contexts: Vec<BlockContext>,
}

enum BlockContext {
    CodeBlock(CodeBlockState),
    Heading(HeadingState),
    Image(ImageState),
    Table(TableState),
}
```

Update `RenderState::new()`:

```rust
pub(super) fn new() -> Self {
    Self {
        html_output: String::new(),
        contexts: Vec::new(),
    }
}
```

- [ ] **Step 3: Add context accessors**

Add these private helpers inside `impl RenderState`:

```rust
fn heading(&self) -> Option<&HeadingState> {
    self.contexts.iter().rev().find_map(|context| match context {
        BlockContext::Heading(heading) => Some(heading),
        _ => None,
    })
}

fn heading_mut(&mut self) -> Option<&mut HeadingState> {
    self.contexts.iter_mut().rev().find_map(|context| match context {
        BlockContext::Heading(heading) => Some(heading),
        _ => None,
    })
}

fn code_block_mut(&mut self) -> Option<&mut CodeBlockState> {
    self.contexts.iter_mut().rev().find_map(|context| match context {
        BlockContext::CodeBlock(code_block) => Some(code_block),
        _ => None,
    })
}

fn image_mut(&mut self) -> Option<&mut ImageState> {
    self.contexts.iter_mut().rev().find_map(|context| match context {
        BlockContext::Image(image) => Some(image),
        _ => None,
    })
}

fn table_mut(&mut self) -> Option<&mut TableState> {
    self.contexts.iter_mut().rev().find_map(|context| match context {
        BlockContext::Table(table) => Some(table),
        _ => None,
    })
}
```

- [ ] **Step 4: Convert status predicates and push methods**

Update `in_code_block`, `in_image`, `in_heading`, `push_*`, `heading_*`, and `code_block_full_range` to use the helpers. Example:

```rust
pub(super) fn in_code_block(&self) -> bool {
    self.contexts
        .iter()
        .any(|context| matches!(context, BlockContext::CodeBlock(_)))
}

pub(super) fn push_code_text(&mut self, text: &str) {
    if let Some(code_block) = self.code_block_mut() {
        code_block.content.push_str(text);
    }
}
```

- [ ] **Step 5: Convert start/finish methods to stack push/pop**

Update start methods to push contexts:

```rust
pub(super) fn start_heading(&mut self, level: u8, range: Range<usize>) {
    self.contexts.push(BlockContext::Heading(HeadingState {
        level,
        range,
        plain_text: String::new(),
        html: String::new(),
    }));
}
```

Update finish methods to remove the matching active context:

```rust
pub(super) fn finish_heading(
    &mut self,
    id: String,
    heading_attrs: String,
) -> Result<String, RenderStateMismatch> {
    let Some(index) = self
        .contexts
        .iter()
        .rposition(|context| matches!(context, BlockContext::Heading(_)))
    else {
        return Err(RenderStateMismatch::ExpectedHeading);
    };
    let BlockContext::Heading(heading) = self.contexts.remove(index) else {
        unreachable!("rposition matched heading");
    };

    Ok(format!(
        "<h{} id=\"{}\"{}>{}</h{}>\n",
        heading.level,
        html_escape(&id),
        heading_attrs,
        heading.html,
        heading.level
    ))
}
```

Use the same pattern for `finish_code_block` and `finish_image`.

- [ ] **Step 6: Convert table methods to `Result`**

Change table methods:

```rust
pub(super) fn start_table_head(&mut self) -> Result<(), RenderStateMismatch> {
    let table = self.table_mut().ok_or(RenderStateMismatch::ExpectedTable)?;
    table.in_head = true;
    Ok(())
}

pub(super) fn table_cell_end_tag(&self) -> Result<&'static str, RenderStateMismatch> {
    let table = self
        .contexts
        .iter()
        .rev()
        .find_map(|context| match context {
            BlockContext::Table(table) => Some(table),
            _ => None,
        })
        .ok_or(RenderStateMismatch::ExpectedTable)?;
    Ok(if table.in_head { "</th>\n" } else { "</td>\n" })
}
```

- [ ] **Step 7: Run state tests**

Run:

```bash
cargo test --lib renderer::state --all-features
```

Expected: PASS for `renderer::state` tests; compile errors may remain in `render.rs` until Task 4.

## Task 4: Update Renderer Dispatch To Handle Mismatches

**Files:**
- Modify: `src/renderer/render.rs`
- Modify: `tests/renderer_test.rs`

- [ ] **Step 1: Import mismatch type**

Change import:

```rust
use super::state::{RenderState, RenderStateMismatch};
```

- [ ] **Step 2: Add mismatch logger**

Add near ignored event log helpers:

```rust
fn log_render_state_mismatch(operation: &'static str, mismatch: RenderStateMismatch) {
    tracing::warn!(
        "[markdown-view] Markdown描画状態の不整合を回復: operation={} mismatch={:?}",
        operation,
        mismatch
    );
}
```

- [ ] **Step 3: Update heading and code block end handling**

Change `handle_code_block_end`:

```rust
fn handle_code_block_end(
    range: Range<usize>,
    line_lookup: &LineLookup,
    syntax_set: &SyntaxSet,
    state: &mut RenderState,
) {
    let line_attrs = code_block_line_attrs(&range, line_lookup, state);
    if let Err(mismatch) = state.finish_code_block(syntax_set, line_attrs) {
        log_render_state_mismatch("finish_code_block", mismatch);
    }
}
```

Change `handle_heading_end`:

```rust
fn handle_heading_end(line_lookup: &LineLookup, context: &mut RenderContext) {
    let state = &mut context.state;
    let text = state.heading_plain_text().to_string();
    let level = state.heading_level();
    let slug = slugify(&text);
    let id = generate_unique_id(&slug, &mut context.id_counts);
    let heading_attrs = heading_line_attrs(line_lookup, state);
    match state.finish_heading(id.clone(), heading_attrs) {
        Ok(heading_html) => {
            if let Some(level) = level {
                context.headings.push(HeadingInfo { level, text, id });
            }
            state.push_html(&heading_html);
        }
        Err(mismatch) => log_render_state_mismatch("finish_heading", mismatch),
    }
}
```

- [ ] **Step 4: Update image handling**

Change `handle_image_end`:

```rust
fn handle_image_end(state: &mut RenderState) {
    match state.finish_image() {
        Ok(image_html) => {
            if state.in_heading() {
                state.push_heading_rendered_html_fragment(&image_html);
            } else {
                state.push_html(&image_html);
            }
        }
        Err(mismatch) => log_render_state_mismatch("finish_image", mismatch),
    }
}
```

- [ ] **Step 5: Update table handling**

Change table handlers:

```rust
fn handle_table_end(state: &mut RenderState) {
    state.push_html("</table>\n");
    if let Err(mismatch) = state.finish_table() {
        log_render_state_mismatch("finish_table", mismatch);
    }
}

fn handle_table_head_start(state: &mut RenderState) {
    if let Err(mismatch) = state.start_table_head() {
        log_render_state_mismatch("start_table_head", mismatch);
        return;
    }
    state.push_html("<thead>\n");
}

fn handle_table_cell_start(state: &mut RenderState) {
    match state.table_cell_start_tag() {
        Ok(tag) => state.push_html(&tag),
        Err(mismatch) => log_render_state_mismatch("table_cell_start_tag", mismatch),
    }
}

fn handle_table_cell_end(state: &mut RenderState) {
    match state.table_cell_end_tag() {
        Ok(tag) => state.push_html(tag),
        Err(mismatch) => log_render_state_mismatch("table_cell_end_tag", mismatch),
    }
}
```

Apply the same pattern to `finish_table_head` and `reset_table_row`.

- [ ] **Step 6: Add renderer compatibility regression**

Add this public regression to `tests/renderer_test.rs`. If the exact same test already exists, leave the existing test unchanged and record that no file edit was needed for this step in the task notes.

```rust
#[test]
fn test_renderer_state境界が連続構文で漏れない() {
    let md = concat!(
        "# Head ![logo](./logo.png \"caption\") `code`\n",
        "\n",
        "![remote](https://example.com/p.png)\n",
        "\n",
        "| L | R |\n",
        "|:--|--:|\n",
        "| a | b |\n",
        "\n",
        "```unknown-lang\n",
        "<x>\n",
        "```\n",
        "\n",
        "After",
    );
    let html = normalize_source_markup(render_markdown(md).as_str());

    assert!(html.contains(r#"<h1 id="head-code">Head <img src="./logo.png" alt="logo" title="caption" /> <code>code</code></h1>"#));
    assert!(html.contains(r##"<p><img src="#" alt="remote" /></p>"##));
    assert!(html.contains(r#"<td class="align-left">a</td>"#));
    assert!(html.contains(r#"<td class="align-right">b</td>"#));
    assert!(html.contains(r#"<pre class="code-block"><code class="syn-code language-unknown-lang">&lt;x&gt;"#));
    assert!(html.contains("<p>After</p>"));
}
```

- [ ] **Step 7: Run renderer tests**

Run:

```bash
cargo test --test renderer_test --all-features
```

Expected: PASS.

- [ ] **Step 8: Run renderer module tests**

Run:

```bash
cargo test --lib renderer --all-features
```

Expected: PASS.

- [ ] **Step 9: Commit renderer state typing**

Run:

```bash
git add src/renderer/state.rs src/renderer/render.rs tests/renderer_test.rs
git commit -m "refactor: RenderStateのopen close対応を型化"
```

Expected: commit succeeds.

## Task 5: Full Verification And TODO Update

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Run full verification**

Run:

```bash
./verify.sh
```

Expected: PASS for format, clippy, and tests.

- [ ] **Step 2: Update TODO completion**

In `docs/todo/TODO.md`, remove both completed Medium items from `## Medium Priority` and add these entries at the top of `## Done Summary`:

```markdown
- [x] `template/mod.rs` のテストをサブモジュールへ分割し、`render_page` の 62 行 `format!` を関数分割する
  - 完了根拠: `template` のテストを `page` / `tree` / `assets` / `message` へ局所化し、`mod.rs` は公開 API smoke test 中心へ戻した。`render_page` は head/body/sidebar/topbar helper へ分割し、属性値 escape を `html_attr` に集約した。`SanitizedHtml` の本文/TOCは二重 escape せず、memo degraded と directory mode の属性契約をテストで固定した。

- [x] `RenderState` を `enum BlockContext` スタックに置き換えて open/close 対応を型化する
  - 完了根拠: `RenderState` は `Vec<BlockContext>` と `RenderStateMismatch` で Heading / CodeBlock / Image / Table の active context を扱う構成になった。`render.rs` は mismatch を `warn!` して malformed context の HTML 確定を skip する。通常 Markdown の見出し、コードブロック、画像、テーブル、複合入力の出力互換を既存・追加テストで固定した。
```

- [ ] **Step 3: Run docs diff check**

Run:

```bash
git diff --check
```

Expected: no trailing whitespace or conflict markers.

- [ ] **Step 4: Commit TODO update**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: templateとRenderState整理の完了を記録"
```

Expected: commit succeeds.

- [ ] **Step 5: Final status check**

Run:

```bash
git status --short --branch
```

Expected: clean worktree on feature/fix branch, ahead of base by implementation commits.

## Self-Review Checklist

- Spec coverage:
  - `template/mod.rs` slimming: Task 1.
  - `render_page` helper split and `html_attr`: Task 2.
  - `RenderState` `BlockContext` stack and mismatch type: Task 3.
  - `render.rs` recover behavior: Task 4.
  - tests and full verification: Tasks 1-5.
  - TODO completion record: Task 5.
- Security:
  - Attribute values are escaped through `html_attr`.
  - `SanitizedHtml` content/toc are not double escaped or converted back to raw untrusted HTML.
  - link/image sanitizers remain in renderer flow.
  - Host/Origin/path/CSP policies are not weakened.
- Rollback:
  - Template changes are isolated in Tasks 1-2 commits.
  - Renderer changes are isolated in Task 4 commit.
  - TODO docs update is isolated in Task 5 commit.
