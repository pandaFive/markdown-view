# render_markdown Responsibility Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `render_markdown` の公開契約と出力を維持したまま、renderer 内部を `RenderOptions` / `RenderContext` / `Renderer` / `RenderState` と補助モジュールへ分割する。

**Architecture:** `render_markdown(input)` は既存 API として残し、内部で `render::Renderer` に委譲する。行番号、URL sanitize、コードハイライト、レンダリング状態を別モジュールへ切り出し、`render.rs` は pulldown-cmark のイベントディスパッチに集中させる。

**Tech Stack:** Rust, pulldown-cmark, syntect, cargo test, cargo clippy, repository `./verify.sh`

---

## File Structure

- Create: `src/renderer/line.rs`
  - `LineLookup`、`source_line_attrs`、`block_line_attrs`、`line_block_marker_with` を保持する。
- Create: `src/renderer/security.rs`
  - `html_escape`、`sanitize_link_href`、`sanitize_image_src`、`UrlPolicy` を保持する。
- Create: `src/renderer/highlight.rs`
  - `render_code_block_html` を保持し、syntect 失敗時の escaped fallback を担当する。
- Create: `src/renderer/state.rs`
  - `RenderState`、heading / code block / image / table の一時状態、HTML buffer 操作を保持する。
- Create: `src/renderer/render.rs`
  - `RenderOptions`、`RenderContext`、`Renderer`、pulldown-cmark event dispatch を保持する。
- Modify: `src/renderer/mod.rs`
  - 公開 API、`SanitizedHtml`、theme CSS、heading 抽出、slug/id 生成、`render_markdown` の委譲だけを残す。
- Modify: `tests/renderer_test.rs`
  - 責務分割後に壊れやすい複合境界テストを追加する。
- Modify after implementation: `docs/todo/BACKLOG.md`
  - 実装と検証が完了したら対象 backlog 項目を Done へ移す。

---

### Task 1: 境界固定テストを追加する

**Files:**
- Modify: `tests/renderer_test.rs`

- [ ] **Step 1: Write characterization tests**

`tests/renderer_test.rs` の `test_見出し内インライン装飾が見出し要素内に収まる` の近くへ次のテストを追加する。

```rust
#[test]
fn test_render_markdown_複合入力の公開api出力を固定する() {
    let md = "# Title `x`\n\n[link](https://example.com) ![img](https://example.com/pic.png)\n\n| L | R |\n|:--|--:|\n| <x> | `code` |\n\n```unknown-lang\n<a>\n```";
    let html = render_markdown(md);
    let html_str = html.as_str();

    assert!(html_str.contains(r#"<h1 id="title-x" data-line-block data-source-start-line="1" data-source-end-line="1">"#));
    assert!(html_str.contains(r#"<code data-source-start-line="1" data-source-end-line="1">x</code>"#));
    assert!(html_str.contains(r#"<a href="https://example.com">"#));
    assert!(html_str.contains(r##"<img src="#" alt="img" />"##));
    assert!(html_str.contains(r#"<th class="align-left">"#));
    assert!(html_str.contains(r#"<th class="align-right">"#));
    assert!(html_str.contains("&lt;x&gt;"));
    assert!(html_str.contains(r#"<code data-source-start-line="6" data-source-end-line="6">code</code>"#));
    assert!(html_str.contains(r#"<code class="syn-code language-unknown-lang">&lt;a&gt;"#));
    assert!(!html_str.contains("https://example.com/pic.png"));
    assert!(!html_str.contains("<x>"));
    assert!(!html_str.contains("<a>\n"));
}

#[test]
fn test_見出し内リンクと装飾のid生成とhtmlを固定する() {
    let md = "# A [Rust](https://www.rust-lang.org \"site\") *lang* `code`";
    let html = normalize_source_markup(render_markdown(md).as_str());

    assert!(html.contains(
        r#"<h1 id="a-rust-lang-code">A <a href="https://www.rust-lang.org" title="site">Rust</a> <em>lang</em> <code>code</code></h1>"#
    ));
}

#[test]
fn test_同一入力内でlinkとimageのurl_policy差分を固定する() {
    let html = render_markdown(
        "[safe](mailto:user@example.com) [bad](data:text/html,<script>x</script>) ![remote](https://example.com/p.png) ![local](./local.png)",
    );
    let html_str = html.as_str();

    assert!(html_str.contains(r#"href="mailto:user@example.com""#));
    assert!(html_str.contains(r##"<a href="#">bad</a>"##));
    assert!(html_str.contains(r##"<img src="#" alt="remote" />"##));
    assert!(html_str.contains(r#"<img src="./local.png" alt="local" />"#));
    assert!(!html_str.contains("data:text/html"));
    assert!(!html_str.contains("https://example.com/p.png"));
}
```

- [ ] **Step 2: Run the new tests before refactor**

Run:

```bash
cargo test --test renderer_test render_markdown_複合入力の公開api出力を固定する
cargo test --test renderer_test 見出し内リンクと装飾のid生成とhtmlを固定する
cargo test --test renderer_test 同一入力内でlinkとimageのurl_policy差分を固定する
```

Expected: all three tests pass on current implementation. If a line number assertion differs, inspect the rendered HTML and update only the expected `data-source-*` line numbers to match current behavior.

- [ ] **Step 3: Commit the tests**

```bash
git add tests/renderer_test.rs
git commit -m "test: render_markdown責務分割前の境界を固定"
```

---

### Task 2: line / security / highlight を抽出する

**Files:**
- Create: `src/renderer/line.rs`
- Create: `src/renderer/security.rs`
- Create: `src/renderer/highlight.rs`
- Modify: `src/renderer/mod.rs`

- [ ] **Step 1: Create `src/renderer/line.rs`**

```rust
use std::ops::Range;

pub(super) struct LineLookup {
    line_starts: Vec<usize>,
}

impl LineLookup {
    pub(super) fn new(input: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, byte) in input.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { line_starts }
    }

    pub(super) fn line_for_offset(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(index) => index + 1,
            Err(index) => index,
        }
    }

    pub(super) fn line_range(&self, range: &Range<usize>) -> (usize, usize) {
        if range.is_empty() {
            let line = self.line_for_offset(range.start);
            return (line, line);
        }
        let start_line = self.line_for_offset(range.start);
        let end_line = self.line_for_offset(range.end.saturating_sub(1));
        (start_line, end_line)
    }
}

pub(super) fn source_line_attrs(line_lookup: &LineLookup, range: &Range<usize>) -> String {
    let (start_line, end_line) = line_lookup.line_range(range);
    format!(
        " data-source-start-line=\"{}\" data-source-end-line=\"{}\"",
        start_line, end_line
    )
}

/// block-level コンテナ（<p>, <ul>, <ol>, <li>, <table>, <blockquote>）向けの行範囲属性。
///
/// `data-source-*` は memo quote の集計対象なので、コンテナには付与しない。
pub(super) fn block_line_attrs(line_lookup: &LineLookup, range: &Range<usize>) -> String {
    let (start_line, end_line) = line_lookup.line_range(range);
    format!(
        " data-line-block data-line-block-start=\"{}\" data-line-block-end=\"{}\"",
        start_line, end_line
    )
}

/// heading / code-block 用: 既存の `source_line_attrs` に `data-line-block` マーカーを前置。
pub(super) fn line_block_marker_with(source_attrs: String) -> String {
    format!(" data-line-block{}", source_attrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_lookup_line_for_offsetは改行境界を正しく返す() {
        let lookup = LineLookup::new("alpha\nbeta\ncharlie");

        assert_eq!(lookup.line_for_offset(0), 1);
        assert_eq!(lookup.line_for_offset(5), 1);
        assert_eq!(lookup.line_for_offset(6), 2);
        assert_eq!(lookup.line_for_offset(10), 2);
        assert_eq!(lookup.line_for_offset(11), 3);
    }

    #[test]
    fn test_line_lookup_line_rangeは複数行範囲を正しく返す() {
        let lookup = LineLookup::new("alpha\nbeta\ncharlie");

        assert_eq!(lookup.line_range(&(0..5)), (1, 1));
        assert_eq!(lookup.line_range(&(0..10)), (1, 2));
        assert_eq!(lookup.line_range(&(6..18)), (2, 3));
        assert_eq!(lookup.line_range(&(6..6)), (2, 2));
    }
}
```

- [ ] **Step 2: Create `src/renderer/security.rs`**

```rust
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum UrlPolicy {
    Link,
    Image,
}

impl UrlPolicy {
    fn allows_remote(self) -> bool {
        matches!(self, Self::Link)
    }
}

pub(super) fn sanitize_link_href(dest_url: &str) -> String {
    sanitize_url(dest_url, UrlPolicy::Link)
}

pub(super) fn sanitize_image_src(dest_url: &str) -> String {
    sanitize_url(dest_url, UrlPolicy::Image)
}

fn sanitize_url(dest_url: &str, policy: UrlPolicy) -> String {
    let trimmed = dest_url.trim();
    if is_safe_href(trimmed, policy) {
        trimmed.to_string()
    } else {
        "#".to_string()
    }
}

fn is_safe_href(dest_url: &str, policy: UrlPolicy) -> bool {
    if dest_url.is_empty() {
        return false;
    }

    if dest_url.starts_with("//") {
        return false;
    }

    if dest_url.starts_with('#')
        || dest_url.starts_with('/')
        || dest_url.starts_with("./")
        || dest_url.starts_with("../")
        || dest_url.starts_with('?')
    {
        return true;
    }

    let Some(colon_pos) = dest_url.find(':') else {
        return true;
    };

    if !policy.allows_remote() {
        return false;
    }

    let scheme = dest_url[..colon_pos].to_ascii_lowercase();
    matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel")
}

/// HTML特殊文字のエスケープ（属性値にも安全）
pub fn html_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
```

- [ ] **Step 3: Create `src/renderer/highlight.rs`**

```rust
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use super::security::html_escape;

pub(super) fn render_code_block_html(
    syntax_set: &SyntaxSet,
    language: Option<&str>,
    code: &str,
    line_attrs: &str,
    syntax_highlighting: bool,
) -> String {
    if let Some(lang) = language {
        if syntax_highlighting {
            if let Some(highlighted) = highlighted_code_html(syntax_set, lang, code) {
                return format!(
                    "<pre class=\"code-block\"{}><code class=\"syn-code language-{}\">{}</code></pre>\n",
                    line_attrs,
                    html_escape(lang),
                    highlighted
                );
            }
        }

        return format!(
            "<pre class=\"code-block\"{}><code class=\"syn-code language-{}\">{}</code></pre>\n",
            line_attrs,
            html_escape(lang),
            html_escape(code)
        );
    }

    format!(
        "<pre class=\"code-block\"{}><code class=\"syn-code\">{}</code></pre>\n",
        line_attrs,
        html_escape(code)
    )
}

fn highlighted_code_html(syntax_set: &SyntaxSet, language: &str, code: &str) -> Option<String> {
    let syntax = syntax_set
        .find_syntax_by_token(language)
        .or_else(|| syntax_set.find_syntax_by_extension(language))?;
    let mut generator = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        syntax_set,
        ClassStyle::SpacedPrefixed { prefix: "syn-" },
    );

    for line in LinesWithEndings::from(code) {
        if let Err(e) = generator.parse_html_for_line_which_includes_newline(line) {
            tracing::warn!(
                "[markdown-view] コードハイライトエラー (lang={}): {}",
                language,
                e
            );
            return None;
        }
    }

    Some(generator.finalize())
}
```

- [ ] **Step 4: Update `src/renderer/mod.rs` module declarations and imports**

At the top of `src/renderer/mod.rs`, replace renderer-only imports with:

```rust
mod highlight;
mod line;
mod security;

pub mod toc;

use std::sync::OnceLock;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use syntect::highlighting::ThemeSet;
use syntect::html::{css_for_theme_with_class_style, ClassStyle};
use syntect::parsing::SyntaxSet;

pub use security::html_escape;
```

Remove from `mod.rs`: `LineLookup`, `source_line_attrs`, `block_line_attrs`, `line_block_marker_with`, `UrlPolicy`, `sanitize_link_href`, `sanitize_image_src`, `sanitize_url`, `is_safe_href`, and the old local `html_escape`.

Update references in current `RenderState` and `render_markdown` to use:

```rust
use highlight::render_code_block_html;
use line::{block_line_attrs, line_block_marker_with, source_line_attrs, LineLookup};
use security::{sanitize_image_src, sanitize_link_href};
```

In `RenderState::finish_code_block`, replace the local syntect generation block with:

```rust
let rendered = render_code_block_html(
    ss,
    self.code_block_lang.as_deref(),
    &self.code_block_content,
    &line_attrs,
    true,
);
self.push_html(&rendered);
```

- [ ] **Step 5: Move line tests out of `mod.rs`**

Remove these tests from the `#[cfg(test)] mod tests` in `src/renderer/mod.rs` because they now live in `line.rs`:

```rust
test_line_lookup_line_for_offsetは改行境界を正しく返す
test_line_lookup_line_rangeは複数行範囲を正しく返す
```

Keep the `SanitizedHtml` serde tests in `mod.rs`.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test renderer::line renderer::tests::test_sanitized_html_serde_transparentで文字列として直列化される --lib
cargo test --test renderer_test
```

Expected: all selected tests pass.

- [ ] **Step 7: Commit extraction**

```bash
git add src/renderer/mod.rs src/renderer/line.rs src/renderer/security.rs src/renderer/highlight.rs
git commit -m "refactor: renderer補助責務を分離"
```

---

### Task 3: RenderState を抽出する

**Files:**
- Create: `src/renderer/state.rs`
- Modify: `src/renderer/mod.rs`

- [ ] **Step 1: Create `src/renderer/state.rs`**

```rust
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind};
use syntect::parsing::SyntaxSet;

use super::highlight::render_code_block_html;
use super::line::{line_block_marker_with, source_line_attrs, LineLookup};
use super::security::{html_escape, sanitize_image_src};

pub(super) struct RenderState {
    html_output: String,
    in_code_block: bool,
    code_block_lang: Option<String>,
    code_block_content: String,
    code_block_range: Option<Range<usize>>,
    heading_level: Option<u8>,
    heading_range: Option<Range<usize>>,
    heading_plain_text: String,
    heading_html: String,
    image_src: Option<String>,
    image_title: Option<String>,
    image_alt: String,
    in_table_head: bool,
    table_alignments: Vec<Alignment>,
    table_cell_index: usize,
}

impl RenderState {
    pub(super) fn new() -> Self {
        Self {
            html_output: String::new(),
            in_code_block: false,
            code_block_lang: None,
            code_block_content: String::new(),
            code_block_range: None,
            heading_level: None,
            heading_range: None,
            heading_plain_text: String::new(),
            heading_html: String::new(),
            image_src: None,
            image_title: None,
            image_alt: String::new(),
            in_table_head: false,
            table_alignments: Vec::new(),
            table_cell_index: 0,
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
        self.in_code_block
    }

    pub(super) fn in_image(&self) -> bool {
        self.image_src.is_some()
    }

    pub(super) fn in_heading(&self) -> bool {
        self.heading_level.is_some()
    }

    pub(super) fn push_code_text(&mut self, text: &str) {
        self.code_block_content.push_str(text);
    }

    pub(super) fn push_code_break(&mut self) {
        self.code_block_content.push('\n');
    }

    pub(super) fn push_image_alt_text(&mut self, text: &str) {
        self.image_alt.push_str(text);
    }

    pub(super) fn push_image_alt_space(&mut self) {
        self.image_alt.push(' ');
    }

    pub(super) fn push_heading_text(&mut self, text: &str, html: &str) {
        self.heading_plain_text.push_str(text);
        self.heading_html.push_str(html);
    }

    pub(super) fn push_heading_space(&mut self) {
        self.heading_plain_text.push(' ');
        self.heading_html.push(' ');
    }

    pub(super) fn push_heading_html(&mut self, html: &str) {
        self.heading_html.push_str(html);
    }

    pub(super) fn start_heading(&mut self, level: u8, range: Range<usize>) {
        self.heading_level = Some(level);
        self.heading_range = Some(range);
        self.heading_plain_text.clear();
        self.heading_html.clear();
    }

    pub(super) fn finish_heading(
        &mut self,
        line_lookup: &LineLookup,
        id: String,
    ) -> Option<String> {
        let level = self.heading_level?;
        let heading_attrs = self
            .heading_range
            .as_ref()
            .map(|heading_range| line_block_marker_with(source_line_attrs(line_lookup, heading_range)))
            .unwrap_or_default();
        let html = format!(
            "<h{} id=\"{}\"{}>{}</h{}>\n",
            level,
            html_escape(&id),
            heading_attrs,
            self.heading_html,
            level
        );
        self.heading_level = None;
        self.heading_range = None;
        Some(html)
    }

    pub(super) fn heading_plain_text(&self) -> &str {
        &self.heading_plain_text
    }

    pub(super) fn start_code_block(&mut self, kind: CodeBlockKind<'_>, range: Range<usize>) {
        self.in_code_block = true;
        self.code_block_range = Some(range);
        self.code_block_lang = match kind {
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
        self.code_block_content.clear();
    }

    pub(super) fn finish_code_block(
        &mut self,
        ss: &SyntaxSet,
        range: Range<usize>,
        line_lookup: &LineLookup,
        syntax_highlighting: bool,
    ) {
        let line_attrs = self
            .code_block_range
            .as_ref()
            .map(|start_range| Range {
                start: start_range.start,
                end: range.end,
            })
            .map(|full_range| line_block_marker_with(source_line_attrs(line_lookup, &full_range)))
            .unwrap_or_default();
        let rendered = render_code_block_html(
            ss,
            self.code_block_lang.as_deref(),
            &self.code_block_content,
            &line_attrs,
            syntax_highlighting,
        );
        self.push_html(&rendered);

        self.in_code_block = false;
        self.code_block_lang = None;
        self.code_block_content.clear();
        self.code_block_range = None;
    }

    pub(super) fn start_image(&mut self, dest_url: &str, title: &str) {
        self.image_src = Some(dest_url.to_string());
        self.image_title = if title.is_empty() {
            None
        } else {
            Some(title.to_string())
        };
        self.image_alt.clear();
    }

    pub(super) fn finish_image(&mut self) -> Option<String> {
        let src = self.image_src.take()?;
        let safe_src = sanitize_image_src(&src);
        let mut image_html = format!(
            "<img src=\"{}\" alt=\"{}\"",
            html_escape(&safe_src),
            html_escape(&self.image_alt)
        );
        if let Some(title) = self.image_title.take() {
            image_html.push_str(&format!(" title=\"{}\"", html_escape(&title)));
        }
        image_html.push_str(" />");
        self.image_title = None;
        self.image_alt.clear();
        Some(image_html)
    }

    pub(super) fn start_table(&mut self, alignments: Vec<Alignment>) {
        self.in_table_head = false;
        self.table_alignments = alignments;
        self.table_cell_index = 0;
    }

    pub(super) fn finish_table(&mut self) {
        self.in_table_head = false;
        self.table_alignments.clear();
        self.table_cell_index = 0;
    }

    pub(super) fn start_table_head(&mut self) {
        self.in_table_head = true;
    }

    pub(super) fn finish_table_head(&mut self) {
        self.in_table_head = false;
    }

    pub(super) fn reset_table_row(&mut self) {
        self.table_cell_index = 0;
    }

    pub(super) fn table_cell_start_tag(&mut self) -> String {
        let align_class = self
            .table_alignments
            .get(self.table_cell_index)
            .and_then(table_align_class_attr)
            .unwrap_or("");
        self.table_cell_index = self.table_cell_index.saturating_add(1);
        if self.in_table_head {
            format!("<th{}>", align_class)
        } else {
            format!("<td{}>", align_class)
        }
    }

    pub(super) fn table_cell_end_tag(&self) -> &'static str {
        if self.in_table_head {
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

- [ ] **Step 2: Update `src/renderer/mod.rs` to use extracted state**

Add:

```rust
mod state;
use state::RenderState;
```

Remove the old inline `RenderState` and `table_align_class_attr` definitions from `mod.rs`.

Replace standalone heading/table locals in `render_markdown`:

```rust
let mut heading_level: Option<u8> = None;
let mut heading_range: Option<Range<usize>> = None;
let mut heading_plain_text = String::new();
let mut heading_html = String::new();
let mut in_table_head = false;
let mut table_alignments: Vec<Alignment> = Vec::new();
let mut table_cell_index = 0usize;
```

with state method calls. The key substitutions are:

```rust
state.start_heading(level as u8, range);
let slug = slugify(state.heading_plain_text());
let id = generate_unique_id(&slug, &mut id_counts);
if let Some(heading_html) = state.finish_heading(&line_lookup, id) {
    state.push_html(&heading_html);
}
```

```rust
state.start_table(alignments);
state.finish_table();
state.start_table_head();
state.finish_table_head();
state.reset_table_row();
let tag = state.table_cell_start_tag();
state.push_html(&tag);
state.push_html(state.table_cell_end_tag());
```

For text/code/break handling, replace direct field access with:

```rust
state.in_code_block()
state.push_code_text(&text)
state.push_code_break()
state.in_image()
state.push_image_alt_text(&text)
state.push_image_alt_space()
state.in_heading()
state.push_heading_text(&text, &format!("<span{}>{}</span>", line_attrs, html_escape(&text)))
state.push_heading_html("<em>")
state.push_heading_space()
state.push_soft_break()
```

Return:

```rust
SanitizedHtml::from_sanitized_html(state.into_html())
```

- [ ] **Step 3: Run renderer tests**

Run:

```bash
cargo test --test renderer_test
cargo test --lib renderer::tests
```

Expected: all tests pass.

- [ ] **Step 4: Commit state extraction**

```bash
git add src/renderer/mod.rs src/renderer/state.rs
git commit -m "refactor: render状態管理を分離"
```

---

### Task 4: Renderer / RenderOptions / RenderContext を導入する

**Files:**
- Create: `src/renderer/render.rs`
- Modify: `src/renderer/mod.rs`

- [ ] **Step 1: Create `src/renderer/render.rs`**

Move the event loop from `render_markdown` into `Renderer`. The new file should define:

```rust
use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use syntect::parsing::SyntaxSet;

use super::line::{block_line_attrs, source_line_attrs, LineLookup};
use super::security::{html_escape, sanitize_link_href};
use super::state::RenderState;
use super::{generate_unique_id, markdown_options, slugify, syntax_set, SanitizedHtml};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RenderOptions {
    pub(super) track_source_lines: bool,
    pub(super) syntax_highlighting: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            track_source_lines: true,
            syntax_highlighting: true,
        }
    }
}

struct RenderContext<'a> {
    line_lookup: LineLookup,
    syntax_set: &'a SyntaxSet,
    id_counts: HashMap<String, usize>,
    options: RenderOptions,
}

impl<'a> RenderContext<'a> {
    fn new(input: &str, options: RenderOptions) -> Self {
        Self {
            line_lookup: LineLookup::new(input),
            syntax_set: syntax_set(),
            id_counts: HashMap::new(),
            options,
        }
    }

    fn source_line_attrs(&self, range: &Range<usize>) -> String {
        if self.options.track_source_lines {
            source_line_attrs(&self.line_lookup, range)
        } else {
            String::new()
        }
    }

    fn block_line_attrs(&self, range: &Range<usize>) -> String {
        if self.options.track_source_lines {
            block_line_attrs(&self.line_lookup, range)
        } else {
            String::new()
        }
    }

    fn next_heading_id(&mut self, text: &str) -> String {
        let slug = slugify(text);
        generate_unique_id(&slug, &mut self.id_counts)
    }
}

pub(super) struct Renderer<'a> {
    input: &'a str,
    context: RenderContext<'a>,
    state: RenderState,
}

impl<'a> Renderer<'a> {
    pub(super) fn new(input: &'a str, options: RenderOptions) -> Self {
        Self {
            input,
            context: RenderContext::new(input, options),
            state: RenderState::new(),
        }
    }

    pub(super) fn render(mut self) -> SanitizedHtml {
        for (event, range) in Parser::new_ext(self.input, markdown_options()).into_offset_iter() {
            self.dispatch_event(event, range);
        }

        SanitizedHtml::from_sanitized_html(self.state.into_html())
    }

    fn dispatch_event(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(tag) => self.handle_start_tag(tag, range),
            Event::End(tag) => self.handle_end_tag(tag, range),
            Event::Text(text) => self.handle_text(&text, &range),
            Event::Code(text) => self.handle_inline_code(&text, &range),
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::SoftBreak => self.handle_soft_break(),
            Event::HardBreak => self.handle_hard_break(),
            Event::Rule => self.state.push_html("<hr />\n"),
            Event::TaskListMarker(checked) => self.handle_task_list_marker(checked),
            other => {
                tracing::debug!(
                    "[markdown-view] 未処理のMarkdownイベントを無視: {:?}",
                    other
                );
            }
        }
    }

    fn handle_start_tag(&mut self, tag: Tag<'_>, range: Range<usize>) {
        match tag {
            Tag::CodeBlock(kind) => self.state.start_code_block(kind, range),
            Tag::Heading { level, .. } => self.state.start_heading(level as u8, range),
            Tag::Image { dest_url, title, .. } => self.state.start_image(&dest_url, &title),
            Tag::Paragraph => {
                let attrs = self.context.block_line_attrs(&range);
                self.state.push_html(&format!("<p{}>", attrs));
            }
            Tag::Emphasis => self.push_inline_container_start("<em>"),
            Tag::Strong => self.push_inline_container_start("<strong>"),
            Tag::Strikethrough => self.push_inline_container_start("<del>"),
            Tag::Link { dest_url, title, .. } => self.handle_link_start(&dest_url, &title),
            Tag::BlockQuote(_) => {
                let attrs = self.context.block_line_attrs(&range);
                self.state.push_html(&format!("<blockquote{}>\n", attrs));
            }
            Tag::List(Some(start)) => {
                let attrs = self.context.block_line_attrs(&range);
                self.state.push_html(&format!("<ol start=\"{}\"{}>\n", start, attrs));
            }
            Tag::List(None) => {
                let attrs = self.context.block_line_attrs(&range);
                self.state.push_html(&format!("<ul{}>\n", attrs));
            }
            Tag::Item => {
                let attrs = self.context.block_line_attrs(&range);
                self.state.push_html(&format!("<li{}>", attrs));
            }
            Tag::Table(alignments) => {
                let attrs = self.context.block_line_attrs(&range);
                self.state.push_html(&format!("<table{}>\n", attrs));
                self.state.start_table(alignments);
            }
            Tag::TableHead => {
                self.state.start_table_head();
                self.state.push_html("<thead>\n");
            }
            Tag::TableRow => {
                self.state.push_html("<tr>\n");
                self.state.reset_table_row();
            }
            Tag::TableCell => {
                let tag = self.state.table_cell_start_tag();
                self.state.push_html(&tag);
            }
            other => {
                tracing::debug!(
                    "[markdown-view] 未処理のMarkdown開始タグを無視: {:?}",
                    other
                );
            }
        }
    }

    fn handle_end_tag(&mut self, tag: TagEnd, range: Range<usize>) {
        match tag {
            TagEnd::CodeBlock => self.state.finish_code_block(
                self.context.syntax_set,
                range,
                &self.context.line_lookup,
                self.context.options.syntax_highlighting,
            ),
            TagEnd::Heading(_) => self.finish_heading(),
            TagEnd::Image => self.finish_image(),
            TagEnd::Paragraph => self.state.push_html("</p>\n"),
            TagEnd::Emphasis => self.push_inline_container_end("</em>"),
            TagEnd::Strong => self.push_inline_container_end("</strong>"),
            TagEnd::Strikethrough => self.push_inline_container_end("</del>"),
            TagEnd::Link => self.push_inline_container_end("</a>"),
            TagEnd::BlockQuote(_) => self.state.push_html("</blockquote>\n"),
            TagEnd::List(true) => self.state.push_html("</ol>\n"),
            TagEnd::List(false) => self.state.push_html("</ul>\n"),
            TagEnd::Item => self.state.push_html("</li>\n"),
            TagEnd::Table => {
                self.state.push_html("</table>\n");
                self.state.finish_table();
            }
            TagEnd::TableHead => {
                self.state.push_html("</thead>\n");
                self.state.finish_table_head();
            }
            TagEnd::TableRow => self.state.push_html("</tr>\n"),
            TagEnd::TableCell => {
                let tag = self.state.table_cell_end_tag();
                self.state.push_html(tag);
            }
            other => {
                tracing::debug!(
                    "[markdown-view] 未処理のMarkdown終了タグを無視: {:?}",
                    other
                );
            }
        }
    }
}
```

In the same file, add the helper methods used above:

```rust
impl<'a> Renderer<'a> {
    fn handle_text(&mut self, text: &str, range: &Range<usize>) {
        if self.state.in_code_block() {
            self.state.push_code_text(text);
            return;
        }

        if self.state.in_image() {
            self.state.push_image_alt_text(text);
            return;
        }

        let line_attrs = self.context.source_line_attrs(range);
        let html = format!("<span{}>{}</span>", line_attrs, html_escape(text));
        if self.state.in_heading() {
            self.state.push_heading_text(text, &html);
        } else {
            self.state.push_html(&html);
        }
    }

    fn handle_inline_code(&mut self, text: &str, range: &Range<usize>) {
        if self.state.in_image() {
            self.state.push_image_alt_text(text);
            return;
        }

        let line_attrs = self.context.source_line_attrs(range);
        let html = format!("<code{}>{}</code>", line_attrs, html_escape(text));
        if self.state.in_heading() {
            self.state.push_heading_text(text, &html);
        } else {
            self.state.push_html(&html);
        }
    }

    fn handle_soft_break(&mut self) {
        if self.state.in_code_block() {
            self.state.push_code_break();
        } else if self.state.in_image() {
            self.state.push_image_alt_space();
        } else if self.state.in_heading() {
            self.state.push_heading_space();
        } else {
            self.state.push_soft_break();
        }
    }

    fn handle_hard_break(&mut self) {
        if self.state.in_code_block() {
            self.state.push_code_break();
        } else if self.state.in_image() {
            self.state.push_image_alt_space();
        } else if self.state.in_heading() {
            self.state.push_heading_text(" ", "<br />");
        } else {
            self.state.push_html("<br />\n");
        }
    }

    fn handle_task_list_marker(&mut self, checked: bool) {
        if checked {
            self.state
                .push_html("<input type=\"checkbox\" checked=\"\" disabled=\"\" /> ");
        } else {
            self.state
                .push_html("<input type=\"checkbox\" disabled=\"\" /> ");
        }
    }

    fn handle_link_start(&mut self, dest_url: &str, title: &str) {
        if self.state.in_image() {
            return;
        }

        let safe_dest = sanitize_link_href(dest_url);
        let mut link_html = format!("<a href=\"{}\"", html_escape(&safe_dest));
        if !title.is_empty() {
            link_html.push_str(&format!(" title=\"{}\"", html_escape(title)));
        }
        link_html.push('>');
        self.push_inline_html(&link_html);
    }

    fn finish_heading(&mut self) {
        let id = self.context.next_heading_id(self.state.heading_plain_text());
        if let Some(html) = self
            .state
            .finish_heading(&self.context.line_lookup, id)
        {
            self.state.push_html(&html);
        }
    }

    fn finish_image(&mut self) {
        if let Some(image_html) = self.state.finish_image() {
            self.push_inline_html(&image_html);
        }
    }

    fn push_inline_container_start(&mut self, html: &str) {
        if self.state.in_image() {
            return;
        }
        self.push_inline_html(html);
    }

    fn push_inline_container_end(&mut self, html: &str) {
        if self.state.in_image() {
            return;
        }
        self.push_inline_html(html);
    }

    fn push_inline_html(&mut self, html: &str) {
        if self.state.in_heading() {
            self.state.push_heading_html(html);
        } else {
            self.state.push_html(html);
        }
    }
}
```

- [ ] **Step 2: Slim `render_markdown` in `src/renderer/mod.rs`**

Add module declaration:

```rust
mod render;
```

Replace `render_markdown` body with:

```rust
pub fn render_markdown(input: &str) -> SanitizedHtml {
    if input.is_empty() {
        return SanitizedHtml::from_sanitized_html(String::new());
    }

    render::Renderer::new(input, render::RenderOptions::default()).render()
}
```

Remove from `mod.rs` imports that are now only used by `render.rs`: `std::ops::Range`, `pulldown_cmark::Alignment`, and any direct `RenderState` import.

- [ ] **Step 3: Run full Rust tests**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: all tests pass.

- [ ] **Step 4: Commit renderer extraction**

```bash
git add src/renderer/mod.rs src/renderer/render.rs
git commit -m "refactor: render_markdownをrendererへ委譲"
```

---

### Task 5: Backlog を更新して最終検証する

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Move backlog item to Done**

In `docs/todo/BACKLOG.md`, remove the P3 unchecked item beginning with:

```markdown
- [ ] `render_markdown` の責務分割（大規模）
```

Add a Done entry under `## Done`:

```markdown
- [x] `render_markdown` の責務分割
  - ファイル: `src/renderer/{mod,render,state,line,security,highlight,toc}.rs`, `tests/renderer_test.rs`
  - 内容: `render_markdown` の公開契約を維持したまま、イベントディスパッチ、状態管理、行番号属性、URL sanitize、コードハイライトを renderer 内部モジュールへ分割した
  - 完了根拠: `render_markdown` 境界テスト追加、`cargo test --all-targets --all-features`、`./verify.sh`
  - 由来: PR #59 探索 (2026-04-18)
```

- [ ] **Step 2: Run formatting**

```bash
cargo fmt --all -- --check
```

Expected: command exits with status 0. If it fails, run `cargo fmt --all`, inspect the diff, then rerun the check.

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: command exits with status 0.

- [ ] **Step 4: Run all tests**

```bash
cargo test --all-targets --all-features
```

Expected: command exits with status 0.

- [ ] **Step 5: Run repository verification**

```bash
./verify.sh
```

Expected: command exits with status 0.

- [ ] **Step 6: Commit backlog and verification cleanup**

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: render_markdown責務分割を完了扱いに更新"
```

If `cargo fmt --all` changed Rust files during Step 2, include those files in the same commit only when they are formatting-only changes caused by this implementation:

```bash
git add src/renderer/mod.rs src/renderer/render.rs src/renderer/state.rs src/renderer/line.rs src/renderer/security.rs src/renderer/highlight.rs docs/todo/BACKLOG.md
git commit -m "docs: render_markdown責務分割を完了扱いに更新"
```

---

## Self-Review

- Spec coverage:
  - 公開 API 維持: Task 4 keeps `render_markdown(input) -> SanitizedHtml`.
  - イベントディスパッチ中心化: Task 4 creates `Renderer::dispatch_event`.
  - `RenderOptions` / `RenderContext`: Task 4 defines both internally.
  - phase handler split: Task 3 moves state, Task 4 splits handlers.
  - security boundary: Task 2 creates `security.rs`, Task 1 and Task 5 verification preserve policy.
  - tests and verification: Task 1 adds boundary tests, Task 5 runs required checks.
- Placeholder scan:
  - No unresolved placeholder markers or incomplete sections are intentionally left.
  - Every task has concrete files, commands, expected result, and commit command.
- Type consistency:
  - `RenderOptions`, `RenderContext`, `Renderer`, `RenderState`, `LineLookup`, and helper method names are consistent across tasks.
  - `RenderOptions` remains `pub(super)`, matching the design choice not to stabilize it as a public API.
