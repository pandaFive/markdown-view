# Renderer Type Simplification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** renderer の外部出力を変えずに、`Renderer` 所有者 struct を削除し、`RenderState` の一時状態を専用型へ分ける。

**Architecture:** `render.rs` は `render(input)` と private helper 関数中心にし、`LineLookup` / `SyntaxSet` / `id_counts` はローカル変数として明示的に渡す。`state.rs` は HTML buffer と `Option<CodeBlockState>` / `Option<HeadingState>` / `Option<ImageState>` / `Option<TableState>` の集約にして、`bool + Option` の重複状態を減らす。

**Tech Stack:** Rust, pulldown-cmark, syntect, cargo test, cargo clippy, `./verify.sh`

---

## Files

- Modify: `tests/renderer_test.rs`
  - renderer state 境界の characterization test を 1 件追加する。
- Modify: `src/renderer/state.rs`
  - `RenderState` の並列 `Option` 群を専用 state struct へ分ける。
- Modify: `src/renderer/render.rs`
  - `Renderer` / data-owning `RenderContext` を削除し、関数中心へ変更する。
- Modify: `src/renderer/mod.rs`
  - `render_markdown` の委譲先を `render::Renderer::render(input)` から `render::render(input)` へ変更する。

No changes expected:

- `src/server/`
- `src/template/`
- `src/template/assets/js/`
- `src/renderer/security.rs`
- `src/renderer/highlight.rs`
- `src/renderer/line.rs`

## Acceptance Criteria

- `Renderer` struct が `src/renderer/render.rs` から消えている。
- `RenderContext` が `line_lookup` / `syntax_set` / `id_counts` の所有者ではなくなっている。最終形では削除する。
- `RenderState` が `CodeBlockState` / `HeadingState` / `ImageState` / `TableState` を持つ。
- `in_code_block: bool`、`code_block_range: Option<_>`、`heading_level: Option<_>`、`heading_range: Option<_>`、`image_src: Option<_>` のような重複状態が消えている。
- `render_markdown(input)` の公開 API と HTML 出力が変わらない。
- renderer の XSS 不変条件が維持される。
- `./verify.sh` が通る。

---

### Task 1: renderer state 境界テストを追加する

**Files:**
- Modify: `tests/renderer_test.rs`

- [ ] **Step 1: Add characterization test**

Insert this test after `test_同一入力内でlinkとimageのurl_policy差分を固定する`.

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

    assert!(html.contains(
        r#"<h1 id="head-code">Head <img src="./logo.png" alt="logo" title="caption" /> <code>code</code></h1>"#
    ));
    assert!(html.contains(r##"<p><img src="#" alt="remote" /></p>"##));
    assert!(html.contains(r#"<th class="align-left">L</th>"#));
    assert!(html.contains(r#"<th class="align-right">R</th>"#));
    assert!(html.contains(r#"<td class="align-left">a</td>"#));
    assert!(html.contains(r#"<td class="align-right">b</td>"#));
    assert!(html.contains(
        r#"<pre class="code-block"><code class="syn-code language-unknown-lang">&lt;x&gt;"#
    ));
    assert!(html.contains("<p>After</p>"));
    assert!(!html.contains("https://example.com/p.png"));
    assert!(!html.contains("<x>"));
}
```

- [ ] **Step 2: Run the new test**

Run:

```bash
cargo test --test renderer_test test_renderer_state境界が連続構文で漏れない
```

Expected: PASS. This is a characterization test for a refactor, so it should pass before implementation.

- [ ] **Step 3: Run nearby renderer tests**

Run:

```bash
cargo test --test renderer_test
```

Expected: PASS. The existing renderer suite includes `test_render_markdown_複合入力の公開api出力を固定する`, `test_render_markdown_主要event_dispatchの出力を固定する`, `test_画像titleと画像内リンクはimg属性とaltに閉じる`, `test_連続テーブルでalignmentが次のテーブルへ漏れない`, and `test_同一入力内でlinkとimageのurl_policy差分を固定する`.

- [ ] **Step 4: Commit characterization test**

```bash
git add tests/renderer_test.rs
git commit -m "test: renderer状態境界を固定"
```

---

### Task 2: RenderState を専用 state struct へ分ける

**Files:**
- Modify: `src/renderer/state.rs`

- [ ] **Step 1: Replace `src/renderer/state.rs`**

Replace the full file with this code.

```rust
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind};
use syntect::parsing::SyntaxSet;

use super::highlight::render_code_block_html;
use super::security::{html_escape, sanitize_image_src};

pub(super) struct RenderState {
    html_output: String,
    code_block: Option<CodeBlockState>,
    heading: Option<HeadingState>,
    image: Option<ImageState>,
    table: Option<TableState>,
}

struct CodeBlockState {
    language: Option<String>,
    content: String,
    start_range: Range<usize>,
}

struct HeadingState {
    level: u8,
    range: Range<usize>,
    plain_text: String,
    html: String,
}

struct ImageState {
    src: String,
    title: Option<String>,
    alt: String,
}

struct TableState {
    in_head: bool,
    alignments: Vec<Alignment>,
    cell_index: usize,
}

impl RenderState {
    pub(super) fn new() -> Self {
        Self {
            html_output: String::new(),
            code_block: None,
            heading: None,
            image: None,
            table: None,
        }
    }

    pub(super) fn into_html(self) -> String {
        self.html_output
    }

    pub(super) fn push_html(&mut self, html: &str) {
        self.html_output.push_str(html);
    }

    pub(super) fn push_soft_break(&mut self) {
        self.html_output.push('\n');
    }

    pub(super) fn in_code_block(&self) -> bool {
        self.code_block.is_some()
    }

    pub(super) fn in_image(&self) -> bool {
        self.image.is_some()
    }

    pub(super) fn in_heading(&self) -> bool {
        self.heading.is_some()
    }

    pub(super) fn push_code_text(&mut self, text: &str) {
        if let Some(code_block) = &mut self.code_block {
            code_block.content.push_str(text);
        }
    }

    pub(super) fn push_code_break(&mut self) {
        if let Some(code_block) = &mut self.code_block {
            code_block.content.push('\n');
        }
    }

    pub(super) fn push_image_alt_text(&mut self, text: &str) {
        if let Some(image) = &mut self.image {
            image.alt.push_str(text);
        }
    }

    pub(super) fn push_image_alt_space(&mut self) {
        if let Some(image) = &mut self.image {
            image.alt.push(' ');
        }
    }

    pub(super) fn push_heading_escaped_text_html(&mut self, text: &str, html: &str) {
        if let Some(heading) = &mut self.heading {
            heading.plain_text.push_str(text);
            heading.html.push_str(html);
        }
    }

    pub(super) fn push_heading_space(&mut self) {
        if let Some(heading) = &mut self.heading {
            heading.plain_text.push(' ');
            heading.html.push(' ');
        }
    }

    /// 見出し内へ、組み立て済みのHTML断片を追加する。
    ///
    /// 許容する断片は、静的タグ、`html_escape` / URL sanitize 済みの `format!` 結果、
    /// または `finish_image` が返すエスケープ済み `<img>` に限る。
    /// 生テキストは `push_heading_escaped_text_html` を使う。
    pub(super) fn push_heading_rendered_html_fragment(&mut self, html: &str) {
        if let Some(heading) = &mut self.heading {
            heading.html.push_str(html);
        }
    }

    pub(super) fn start_heading(&mut self, level: u8, range: Range<usize>) {
        self.heading = Some(HeadingState {
            level,
            range,
            plain_text: String::new(),
            html: String::new(),
        });
    }

    pub(super) fn finish_heading(&mut self, id: String, heading_attrs: String) -> Option<String> {
        let heading = self.heading.take()?;
        Some(format!(
            "<h{} id=\"{}\"{}>{}</h{}>\n",
            heading.level,
            html_escape(&id),
            heading_attrs,
            heading.html,
            heading.level
        ))
    }

    pub(super) fn heading_plain_text(&self) -> &str {
        self.heading
            .as_ref()
            .map(|heading| heading.plain_text.as_str())
            .unwrap_or("")
    }

    pub(super) fn heading_range(&self) -> Option<&Range<usize>> {
        self.heading.as_ref().map(|heading| &heading.range)
    }

    pub(super) fn start_code_block(&mut self, kind: CodeBlockKind<'_>, range: Range<usize>) {
        let language = match kind {
            CodeBlockKind::Fenced(lang) => {
                let lang_str = lang.to_string();
                if lang_str.is_empty() {
                    None
                } else {
                    Some(lang_str)
                }
            }
            _ => None,
        };
        self.code_block = Some(CodeBlockState {
            language,
            content: String::new(),
            start_range: range,
        });
    }

    pub(super) fn code_block_full_range(&self, end_range: &Range<usize>) -> Option<Range<usize>> {
        self.code_block.as_ref().map(|code_block| Range {
            start: code_block.start_range.start,
            end: end_range.end,
        })
    }

    pub(super) fn finish_code_block(&mut self, ss: &SyntaxSet, line_attrs: String) {
        let Some(code_block) = self.code_block.take() else {
            return;
        };
        let rendered = render_code_block_html(
            ss,
            code_block.language.as_deref(),
            &code_block.content,
            &line_attrs,
        );
        self.push_html(&rendered);
    }

    pub(super) fn start_image(&mut self, dest_url: &str, title: &str) {
        self.image = Some(ImageState {
            src: dest_url.to_string(),
            title: if title.is_empty() {
                None
            } else {
                Some(title.to_string())
            },
            alt: String::new(),
        });
    }

    pub(super) fn finish_image(&mut self) -> Option<String> {
        let image = self.image.take()?;
        let safe_src = sanitize_image_src(&image.src);
        let mut image_html = format!(
            "<img src=\"{}\" alt=\"{}\"",
            html_escape(&safe_src),
            html_escape(&image.alt)
        );
        if let Some(title) = image.title {
            image_html.push_str(&format!(" title=\"{}\"", html_escape(&title)));
        }
        image_html.push_str(" />");
        Some(image_html)
    }

    pub(super) fn start_table(&mut self, alignments: Vec<Alignment>) {
        self.table = Some(TableState {
            in_head: false,
            alignments,
            cell_index: 0,
        });
    }

    pub(super) fn finish_table(&mut self) {
        self.table = None;
    }

    pub(super) fn start_table_head(&mut self) {
        if let Some(table) = &mut self.table {
            table.in_head = true;
        }
    }

    pub(super) fn finish_table_head(&mut self) {
        if let Some(table) = &mut self.table {
            table.in_head = false;
        }
    }

    pub(super) fn reset_table_row(&mut self) {
        if let Some(table) = &mut self.table {
            table.cell_index = 0;
        }
    }

    pub(super) fn table_cell_start_tag(&mut self) -> String {
        let Some(table) = &mut self.table else {
            return "<td>".to_string();
        };
        let align_class = table
            .alignments
            .get(table.cell_index)
            .and_then(table_align_class_attr)
            .unwrap_or("");
        table.cell_index = table.cell_index.saturating_add(1);
        if table.in_head {
            format!("<th{}>", align_class)
        } else {
            format!("<td{}>", align_class)
        }
    }

    pub(super) fn table_cell_end_tag(&self) -> &'static str {
        if self
            .table
            .as_ref()
            .map(|table| table.in_head)
            .unwrap_or(false)
        {
            "</th>\n"
        } else {
            "</td>\n"
        }
    }
}

fn table_align_class_attr(alignment: &Alignment) -> Option<&'static str> {
    match alignment {
        Alignment::Left => Some(" class=\"align-left\""),
        Alignment::Center => Some(" class=\"align-center\""),
        Alignment::Right => Some(" class=\"align-right\""),
        Alignment::None => None,
    }
}
```

- [ ] **Step 2: Format**

Run:

```bash
cargo fmt --all
```

Expected: command exits 0.

- [ ] **Step 3: Run focused renderer tests**

Run:

```bash
cargo test --test renderer_test
```

Expected: PASS. This checks the new state boundary test plus existing public output, dispatch, table alignment, and heading-inline tests.

- [ ] **Step 4: Run renderer test suite**

Run:

```bash
cargo test --test renderer_test
```

Expected: PASS.

- [ ] **Step 5: Commit RenderState refactor**

```bash
git add src/renderer/state.rs
git commit -m "refactor: RenderStateの一時状態を専用型へ分離"
```

---

### Task 3: Renderer / RenderContext を削除する

**Files:**
- Modify: `src/renderer/render.rs`
- Modify: `src/renderer/mod.rs`

- [ ] **Step 1: Replace `src/renderer/render.rs`**

Replace the full file with this code.

```rust
use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use syntect::parsing::SyntaxSet;

use super::line::{block_line_attrs, line_block_marker_with, source_line_attrs, LineLookup};
use super::security::{html_escape, sanitize_link_href};
use super::state::RenderState;
use super::{generate_unique_id, markdown_options, slugify, syntax_set, SanitizedHtml};

pub(super) fn render(input: &str) -> SanitizedHtml {
    let line_lookup = LineLookup::new(input);
    let syntax_set = syntax_set();
    let mut id_counts = HashMap::new();
    let mut state = RenderState::new();

    let parser = Parser::new_ext(input, markdown_options()).into_offset_iter();
    for (event, range) in parser {
        dispatch_event(
            &mut state,
            &line_lookup,
            syntax_set,
            &mut id_counts,
            event,
            range,
        );
    }

    SanitizedHtml::from_sanitized_html(state.into_html())
}

fn dispatch_event<'a>(
    state: &mut RenderState,
    line_lookup: &LineLookup,
    syntax_set: &'static SyntaxSet,
    id_counts: &mut HashMap<String, usize>,
    event: Event<'a>,
    range: Range<usize>,
) {
    match event {
        Event::Start(tag) => handle_start(state, line_lookup, tag, range),
        Event::End(tag) => handle_end(state, line_lookup, syntax_set, id_counts, tag, range),
        Event::Text(text) => handle_text(state, line_lookup, &text, &range),
        Event::Code(text) => handle_code(state, line_lookup, &text, &range),
        Event::Html(_) | Event::InlineHtml(_) => handle_html(),
        Event::SoftBreak => handle_soft_break(state),
        Event::HardBreak => handle_hard_break(state),
        Event::Rule => handle_rule(state),
        Event::TaskListMarker(checked) => handle_task_list_marker(state, checked),
        other => {
            tracing::debug!(
                "[markdown-view] 未処理のMarkdownイベントを無視: {:?}",
                other
            );
        }
    }
}

fn handle_start<'a>(
    state: &mut RenderState,
    line_lookup: &LineLookup,
    tag: Tag<'a>,
    range: Range<usize>,
) {
    match tag {
        Tag::CodeBlock(kind) => handle_code_block_start(state, kind, range),
        Tag::Heading { level, .. } => handle_heading_start(state, level as u8, range),
        Tag::Image {
            dest_url, title, ..
        } => handle_image_start(state, &dest_url, &title),
        Tag::Paragraph => handle_paragraph_start(state, line_lookup, &range),
        Tag::Emphasis => handle_emphasis_start(state),
        Tag::Strong => handle_strong_start(state),
        Tag::Strikethrough => handle_strikethrough_start(state),
        Tag::Link {
            dest_url, title, ..
        } => handle_link_start(state, &dest_url, &title),
        Tag::BlockQuote(_) => handle_blockquote_start(state, line_lookup, &range),
        Tag::List(Some(start)) => handle_ordered_list_start(state, line_lookup, start, &range),
        Tag::List(None) => handle_unordered_list_start(state, line_lookup, &range),
        Tag::Item => handle_item_start(state, line_lookup, &range),
        Tag::Table(alignments) => handle_table_start(state, line_lookup, alignments, &range),
        Tag::TableHead => handle_table_head_start(state),
        Tag::TableRow => handle_table_row_start(state),
        Tag::TableCell => handle_table_cell_start(state),
        other => {
            tracing::debug!(
                "[markdown-view] 未処理のMarkdown開始タグを無視: {:?}",
                other
            );
        }
    }
}

fn handle_end(
    state: &mut RenderState,
    line_lookup: &LineLookup,
    syntax_set: &'static SyntaxSet,
    id_counts: &mut HashMap<String, usize>,
    tag: TagEnd,
    range: Range<usize>,
) {
    match tag {
        TagEnd::CodeBlock => handle_code_block_end(state, line_lookup, syntax_set, range),
        TagEnd::Heading(_) => handle_heading_end(state, line_lookup, id_counts),
        TagEnd::Image => handle_image_end(state),
        TagEnd::Paragraph => handle_paragraph_end(state),
        TagEnd::Emphasis => handle_emphasis_end(state),
        TagEnd::Strong => handle_strong_end(state),
        TagEnd::Strikethrough => handle_strikethrough_end(state),
        TagEnd::Link => handle_link_end(state),
        TagEnd::BlockQuote(_) => handle_blockquote_end(state),
        TagEnd::List(true) => handle_ordered_list_end(state),
        TagEnd::List(false) => handle_unordered_list_end(state),
        TagEnd::Item => handle_item_end(state),
        TagEnd::Table => handle_table_end(state),
        TagEnd::TableHead => handle_table_head_end(state),
        TagEnd::TableRow => handle_table_row_end(state),
        TagEnd::TableCell => handle_table_cell_end(state),
        other => {
            tracing::debug!(
                "[markdown-view] 未処理のMarkdown終了タグを無視: {:?}",
                other
            );
        }
    }
}

fn handle_text(state: &mut RenderState, line_lookup: &LineLookup, text: &str, range: &Range<usize>) {
    if state.in_code_block() {
        state.push_code_text(text);
        return;
    }

    if state.in_image() {
        state.push_image_alt_text(text);
        return;
    }

    let line_attrs = source_line_attrs(line_lookup, range);
    let html = format!("<span{}>{}</span>", line_attrs, html_escape(text));
    if state.in_heading() {
        state.push_heading_escaped_text_html(text, &html);
    } else {
        state.push_html(&html);
    }
}

fn handle_code(state: &mut RenderState, line_lookup: &LineLookup, text: &str, range: &Range<usize>) {
    if state.in_image() {
        state.push_image_alt_text(text);
        return;
    }

    let line_attrs = source_line_attrs(line_lookup, range);
    let html = format!("<code{}>{}</code>", line_attrs, html_escape(text));
    if state.in_heading() {
        state.push_heading_escaped_text_html(text, &html);
    } else {
        state.push_html(&html);
    }
}

/// pulldown_cmark::Event::Html / Event::InlineHtml をまとめて破棄する（XSS防止）。
fn handle_html() {
    // raw HTMLイベントは出力せず破棄する（XSS防止）
}

fn handle_soft_break(state: &mut RenderState) {
    if state.in_code_block() {
        state.push_code_break();
    } else if state.in_image() {
        state.push_image_alt_space();
    } else if state.in_heading() {
        state.push_heading_space();
    } else {
        state.push_soft_break();
    }
}

fn handle_hard_break(state: &mut RenderState) {
    if state.in_code_block() {
        state.push_code_break();
    } else if state.in_image() {
        state.push_image_alt_space();
    } else if state.in_heading() {
        state.push_heading_escaped_text_html(" ", "<br />");
    } else {
        state.push_html("<br />\n");
    }
}

fn handle_rule(state: &mut RenderState) {
    state.push_html("<hr />\n");
}

fn handle_task_list_marker(state: &mut RenderState, checked: bool) {
    if checked {
        state.push_html("<input type=\"checkbox\" checked=\"\" disabled=\"\" /> ");
    } else {
        state.push_html("<input type=\"checkbox\" disabled=\"\" /> ");
    }
}

fn handle_code_block_start(
    state: &mut RenderState,
    kind: pulldown_cmark::CodeBlockKind<'_>,
    range: Range<usize>,
) {
    state.start_code_block(kind, range);
}

fn handle_code_block_end(
    state: &mut RenderState,
    line_lookup: &LineLookup,
    syntax_set: &'static SyntaxSet,
    range: Range<usize>,
) {
    let line_attrs = code_block_line_attrs(state, line_lookup, &range);
    state.finish_code_block(syntax_set, line_attrs);
}

fn handle_heading_start(state: &mut RenderState, level: u8, range: Range<usize>) {
    state.start_heading(level, range);
}

fn handle_heading_end(
    state: &mut RenderState,
    line_lookup: &LineLookup,
    id_counts: &mut HashMap<String, usize>,
) {
    let slug = slugify(state.heading_plain_text());
    let id = generate_unique_id(&slug, id_counts);
    let heading_attrs = heading_line_attrs(state, line_lookup);
    if let Some(heading_html) = state.finish_heading(id, heading_attrs) {
        state.push_html(&heading_html);
    }
}

fn handle_image_start(state: &mut RenderState, dest_url: &str, title: &str) {
    state.start_image(dest_url, title);
}

fn handle_image_end(state: &mut RenderState) {
    if let Some(image_html) = state.finish_image() {
        if state.in_heading() {
            state.push_heading_rendered_html_fragment(&image_html);
        } else {
            state.push_html(&image_html);
        }
    }
}

fn handle_paragraph_start(state: &mut RenderState, line_lookup: &LineLookup, range: &Range<usize>) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<p{}>", attrs));
}

fn handle_paragraph_end(state: &mut RenderState) {
    state.push_html("</p>\n");
}

fn handle_emphasis_start(state: &mut RenderState) {
    push_inline_tag(state, "<em>");
}

fn handle_emphasis_end(state: &mut RenderState) {
    push_inline_tag(state, "</em>");
}

fn handle_strong_start(state: &mut RenderState) {
    push_inline_tag(state, "<strong>");
}

fn handle_strong_end(state: &mut RenderState) {
    push_inline_tag(state, "</strong>");
}

fn handle_strikethrough_start(state: &mut RenderState) {
    push_inline_tag(state, "<del>");
}

fn handle_strikethrough_end(state: &mut RenderState) {
    push_inline_tag(state, "</del>");
}

fn handle_link_start(state: &mut RenderState, dest_url: &str, title: &str) {
    if state.in_image() {
        return;
    }

    let safe_dest = sanitize_link_href(dest_url);
    let mut link_html = format!("<a href=\"{}\"", html_escape(&safe_dest));
    if !title.is_empty() {
        link_html.push_str(&format!(" title=\"{}\"", html_escape(title)));
    }
    link_html.push('>');
    push_rendered_inline(state, &link_html);
}

fn handle_link_end(state: &mut RenderState) {
    push_inline_tag(state, "</a>");
}

fn handle_blockquote_start(
    state: &mut RenderState,
    line_lookup: &LineLookup,
    range: &Range<usize>,
) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<blockquote{}>\n", attrs));
}

fn handle_blockquote_end(state: &mut RenderState) {
    state.push_html("</blockquote>\n");
}

fn handle_ordered_list_start(
    state: &mut RenderState,
    line_lookup: &LineLookup,
    start: u64,
    range: &Range<usize>,
) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<ol start=\"{}\"{}>\n", start, attrs));
}

fn handle_ordered_list_end(state: &mut RenderState) {
    state.push_html("</ol>\n");
}

fn handle_unordered_list_start(
    state: &mut RenderState,
    line_lookup: &LineLookup,
    range: &Range<usize>,
) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<ul{}>\n", attrs));
}

fn handle_unordered_list_end(state: &mut RenderState) {
    state.push_html("</ul>\n");
}

fn handle_item_start(state: &mut RenderState, line_lookup: &LineLookup, range: &Range<usize>) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<li{}>", attrs));
}

fn handle_item_end(state: &mut RenderState) {
    state.push_html("</li>\n");
}

fn handle_table_start(
    state: &mut RenderState,
    line_lookup: &LineLookup,
    alignments: Vec<pulldown_cmark::Alignment>,
    range: &Range<usize>,
) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<table{}>\n", attrs));
    state.start_table(alignments);
}

fn handle_table_end(state: &mut RenderState) {
    state.push_html("</table>\n");
    state.finish_table();
}

fn handle_table_head_start(state: &mut RenderState) {
    state.start_table_head();
    state.push_html("<thead>\n");
}

fn handle_table_head_end(state: &mut RenderState) {
    state.push_html("</thead>\n");
    state.finish_table_head();
}

fn handle_table_row_start(state: &mut RenderState) {
    state.push_html("<tr>\n");
    state.reset_table_row();
}

fn handle_table_row_end(state: &mut RenderState) {
    state.push_html("</tr>\n");
}

fn handle_table_cell_start(state: &mut RenderState) {
    let tag = state.table_cell_start_tag();
    state.push_html(&tag);
}

fn handle_table_cell_end(state: &mut RenderState) {
    state.push_html(state.table_cell_end_tag());
}

fn push_inline_tag(state: &mut RenderState, html: &'static str) {
    if state.in_image() {
        return;
    }
    push_rendered_inline(state, html);
}

fn push_rendered_inline(state: &mut RenderState, html: &str) {
    if state.in_heading() {
        state.push_heading_rendered_html_fragment(html);
    } else {
        state.push_html(html);
    }
}

fn heading_line_attrs(state: &RenderState, line_lookup: &LineLookup) -> String {
    state
        .heading_range()
        .map(|range| line_block_marker_with(source_line_attrs(line_lookup, range)))
        .unwrap_or_default()
}

fn code_block_line_attrs(
    state: &RenderState,
    line_lookup: &LineLookup,
    end_range: &Range<usize>,
) -> String {
    state
        .code_block_full_range(end_range)
        .map(|range| line_block_marker_with(source_line_attrs(line_lookup, &range)))
        .unwrap_or_default()
}
```

- [ ] **Step 2: Update `src/renderer/mod.rs` delegation**

In `render_markdown`, replace this line:

```rust
    render::Renderer::render(input)
```

with:

```rust
    render::render(input)
```

- [ ] **Step 3: Format**

Run:

```bash
cargo fmt --all
```

Expected: command exits 0.

- [ ] **Step 4: Confirm owner structs are gone**

Run:

```bash
rg -n "struct RenderContext|struct Renderer|Renderer::" src/renderer
```

Expected: no output and exit code 1.

- [ ] **Step 5: Run focused renderer tests**

Run:

```bash
cargo test --test renderer_test
```

Expected: PASS. This checks the new state boundary test, representative output tests, and full pipeline XSS coverage.

- [ ] **Step 6: Run renderer and toc tests**

Run:

```bash
cargo test --test renderer_test
cargo test --test toc_test
```

Expected: PASS for both commands.

- [ ] **Step 7: Commit renderer removal**

```bash
git add src/renderer/render.rs src/renderer/mod.rs
git commit -m "refactor: Renderer所有者structを削除"
```

---

### Task 4: Full verification and cleanup

**Files:**
- Inspect: `src/renderer/render.rs`
- Inspect: `src/renderer/state.rs`
- Inspect: `tests/renderer_test.rs`

- [ ] **Step 1: Run format check**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS.

- [ ] **Step 2: Run clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 3: Run full Rust test suite**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 4: Run required project verification**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 5: Confirm final scope**

Run:

```bash
git diff --stat HEAD~3..HEAD
```

Expected output shape:

```text
 src/renderer/mod.rs
 src/renderer/render.rs
 src/renderer/state.rs
 tests/renderer_test.rs
```

No `src/server/`, `src/template/`, or JavaScript files should appear.

Run:

```bash
git status --short
```

Expected: no output.

## Security Review Notes

- `handle_html()` must continue to drop `Event::Html` and `Event::InlineHtml`.
- `handle_text()` and `handle_code()` must continue to use `html_escape`.
- `handle_link_start()` must continue to call `sanitize_link_href` before writing `href`.
- `RenderState::finish_image()` must continue to call `sanitize_image_src` before writing `src`.
- `SanitizedHtml::from_sanitized_html` must remain `pub(in crate::renderer)` in `src/renderer/mod.rs`.
- New helper functions must not accept attacker-controlled text and append it to `html_output` without escaping.

## Rollback

If Task 2 fails after reasonable local fixes, revert only the Task 2 commit:

```bash
git revert <task-2-commit>
```

If Task 3 fails after reasonable local fixes, revert only the Task 3 commit:

```bash
git revert <task-3-commit>
```

After either revert, run:

```bash
cargo test --test renderer_test
./verify.sh
```
