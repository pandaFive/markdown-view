use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Parser, Tag, TagEnd};
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
            event,
            range,
            &line_lookup,
            syntax_set,
            &mut id_counts,
            &mut state,
        );
    }

    SanitizedHtml::from_sanitized_html(state.into_html())
}

fn dispatch_event(
    event: Event<'_>,
    range: Range<usize>,
    line_lookup: &LineLookup,
    syntax_set: &SyntaxSet,
    id_counts: &mut HashMap<String, usize>,
    state: &mut RenderState,
) {
    match event {
        Event::Start(tag) => handle_start(tag, range, line_lookup, state),
        Event::End(tag) => handle_end(tag, range, line_lookup, syntax_set, id_counts, state),
        Event::Text(text) => handle_text(&text, &range, line_lookup, state),
        Event::Code(text) => handle_code(&text, &range, line_lookup, state),
        Event::Html(_) | Event::InlineHtml(_) => handle_html(),
        Event::SoftBreak => handle_soft_break(state),
        Event::HardBreak => handle_hard_break(state),
        Event::Rule => handle_rule(state),
        Event::TaskListMarker(checked) => handle_task_list_marker(checked, state),
        other => {
            let _ = log_ignored_markdown_event(&other);
        }
    }
}

fn handle_start(
    tag: Tag<'_>,
    range: Range<usize>,
    line_lookup: &LineLookup,
    state: &mut RenderState,
) {
    match tag {
        Tag::CodeBlock(kind) => handle_code_block_start(kind, range, state),
        Tag::Heading { level, .. } => handle_heading_start(level as u8, range, state),
        Tag::Image {
            dest_url, title, ..
        } => handle_image_start(&dest_url, &title, state),
        Tag::Paragraph => handle_paragraph_start(&range, line_lookup, state),
        Tag::Emphasis => handle_emphasis_start(state),
        Tag::Strong => handle_strong_start(state),
        Tag::Strikethrough => handle_strikethrough_start(state),
        Tag::Link {
            dest_url, title, ..
        } => handle_link_start(&dest_url, &title, state),
        Tag::BlockQuote(_) => handle_blockquote_start(&range, line_lookup, state),
        Tag::List(Some(start)) => handle_ordered_list_start(start, &range, line_lookup, state),
        Tag::List(None) => handle_unordered_list_start(&range, line_lookup, state),
        Tag::Item => handle_item_start(&range, line_lookup, state),
        Tag::Table(alignments) => handle_table_start(alignments, &range, line_lookup, state),
        Tag::TableHead => handle_table_head_start(state),
        Tag::TableRow => handle_table_row_start(state),
        Tag::TableCell => handle_table_cell_start(state),
        other => {
            let _ = log_ignored_markdown_start_tag(&other);
        }
    }
}

fn handle_end(
    tag: TagEnd,
    range: Range<usize>,
    line_lookup: &LineLookup,
    syntax_set: &SyntaxSet,
    id_counts: &mut HashMap<String, usize>,
    state: &mut RenderState,
) {
    match tag {
        TagEnd::CodeBlock => handle_code_block_end(range, line_lookup, syntax_set, state),
        TagEnd::Heading(_) => handle_heading_end(line_lookup, id_counts, state),
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
            let _ = log_ignored_markdown_end_tag(&other);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IgnoredMarkdownEventKind {
    Event,
    StartTag,
    EndTag,
}

fn log_ignored_markdown_event(event: &Event<'_>) -> IgnoredMarkdownEventKind {
    tracing::debug!(
        "[markdown-view] 未処理のMarkdownイベントを無視: {:?}",
        event
    );
    IgnoredMarkdownEventKind::Event
}

fn log_ignored_markdown_start_tag(tag: &Tag<'_>) -> IgnoredMarkdownEventKind {
    tracing::debug!(
        "[markdown-view] 未処理のMarkdown開始タグを無視: {:?}",
        tag
    );
    IgnoredMarkdownEventKind::StartTag
}

fn log_ignored_markdown_end_tag(tag: &TagEnd) -> IgnoredMarkdownEventKind {
    tracing::debug!(
        "[markdown-view] 未処理のMarkdown終了タグを無視: {:?}",
        tag
    );
    IgnoredMarkdownEventKind::EndTag
}

fn handle_text(
    text: &str,
    range: &Range<usize>,
    line_lookup: &LineLookup,
    state: &mut RenderState,
) {
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

fn handle_code(
    text: &str,
    range: &Range<usize>,
    line_lookup: &LineLookup,
    state: &mut RenderState,
) {
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

fn handle_task_list_marker(checked: bool, state: &mut RenderState) {
    if checked {
        state.push_html("<input type=\"checkbox\" checked=\"\" disabled=\"\" /> ");
    } else {
        state.push_html("<input type=\"checkbox\" disabled=\"\" /> ");
    }
}

fn handle_code_block_start(kind: CodeBlockKind<'_>, range: Range<usize>, state: &mut RenderState) {
    state.start_code_block(kind, range);
}

fn handle_code_block_end(
    range: Range<usize>,
    line_lookup: &LineLookup,
    syntax_set: &SyntaxSet,
    state: &mut RenderState,
) {
    let line_attrs = code_block_line_attrs(&range, line_lookup, state);
    state.finish_code_block(syntax_set, line_attrs);
}

fn handle_heading_start(level: u8, range: Range<usize>, state: &mut RenderState) {
    state.start_heading(level, range);
}

fn handle_heading_end(
    line_lookup: &LineLookup,
    id_counts: &mut HashMap<String, usize>,
    state: &mut RenderState,
) {
    let slug = slugify(state.heading_plain_text());
    let id = generate_unique_id(&slug, id_counts);
    let heading_attrs = heading_line_attrs(line_lookup, state);
    if let Some(heading_html) = state.finish_heading(id, heading_attrs) {
        state.push_html(&heading_html);
    }
}

fn handle_image_start(dest_url: &str, title: &str, state: &mut RenderState) {
    state.start_image(dest_url, title);
}

fn handle_image_end(state: &mut RenderState) {
    let image_html = state.finish_image();
    if state.in_heading() {
        state.push_heading_rendered_html_fragment(&image_html);
    } else {
        state.push_html(&image_html);
    }
}

fn handle_paragraph_start(range: &Range<usize>, line_lookup: &LineLookup, state: &mut RenderState) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<p{}>", attrs));
}

fn handle_paragraph_end(state: &mut RenderState) {
    state.push_html("</p>\n");
}

fn handle_emphasis_start(state: &mut RenderState) {
    push_inline_tag("<em>", state);
}

fn handle_emphasis_end(state: &mut RenderState) {
    push_inline_tag("</em>", state);
}

fn handle_strong_start(state: &mut RenderState) {
    push_inline_tag("<strong>", state);
}

fn handle_strong_end(state: &mut RenderState) {
    push_inline_tag("</strong>", state);
}

fn handle_strikethrough_start(state: &mut RenderState) {
    push_inline_tag("<del>", state);
}

fn handle_strikethrough_end(state: &mut RenderState) {
    push_inline_tag("</del>", state);
}

fn handle_link_start(dest_url: &str, title: &str, state: &mut RenderState) {
    if state.in_image() {
        return;
    }

    let safe_dest = sanitize_link_href(dest_url);
    let mut link_html = format!("<a href=\"{}\"", html_escape(&safe_dest));
    if !title.is_empty() {
        link_html.push_str(&format!(" title=\"{}\"", html_escape(title)));
    }
    link_html.push('>');
    push_rendered_inline(&link_html, state);
}

fn handle_link_end(state: &mut RenderState) {
    push_inline_tag("</a>", state);
}

fn handle_blockquote_start(
    range: &Range<usize>,
    line_lookup: &LineLookup,
    state: &mut RenderState,
) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<blockquote{}>\n", attrs));
}

fn handle_blockquote_end(state: &mut RenderState) {
    state.push_html("</blockquote>\n");
}

fn handle_ordered_list_start(
    start: u64,
    range: &Range<usize>,
    line_lookup: &LineLookup,
    state: &mut RenderState,
) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<ol start=\"{}\"{}>\n", start, attrs));
}

fn handle_ordered_list_end(state: &mut RenderState) {
    state.push_html("</ol>\n");
}

fn handle_unordered_list_start(
    range: &Range<usize>,
    line_lookup: &LineLookup,
    state: &mut RenderState,
) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<ul{}>\n", attrs));
}

fn handle_unordered_list_end(state: &mut RenderState) {
    state.push_html("</ul>\n");
}

fn handle_item_start(range: &Range<usize>, line_lookup: &LineLookup, state: &mut RenderState) {
    let attrs = block_line_attrs(line_lookup, range);
    state.push_html(&format!("<li{}>", attrs));
}

fn handle_item_end(state: &mut RenderState) {
    state.push_html("</li>\n");
}

fn handle_table_start(
    alignments: Vec<Alignment>,
    range: &Range<usize>,
    line_lookup: &LineLookup,
    state: &mut RenderState,
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

fn push_inline_tag(html: &'static str, state: &mut RenderState) {
    if state.in_image() {
        return;
    }
    push_rendered_inline(html, state);
}

fn push_rendered_inline(html: &str, state: &mut RenderState) {
    if state.in_heading() {
        state.push_heading_rendered_html_fragment(html);
    } else {
        state.push_html(html);
    }
}

fn heading_line_attrs(line_lookup: &LineLookup, state: &RenderState) -> String {
    let range = state.heading_range();
    debug_assert!(
        range.is_some(),
        "heading_line_attrs: アクティブな見出しがない状態で呼ばれた"
    );
    range
        .map(|range| line_block_marker_with(source_line_attrs(line_lookup, range)))
        .unwrap_or_default()
}

fn code_block_line_attrs(
    end_range: &Range<usize>,
    line_lookup: &LineLookup,
    state: &RenderState,
) -> String {
    let range = state.code_block_full_range(end_range);
    debug_assert!(
        range.is_some(),
        "code_block_line_attrs: アクティブなコードブロックがない状態で呼ばれた"
    );
    range
        .map(|range| line_block_marker_with(source_line_attrs(line_lookup, &range)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "heading_line_attrs: アクティブな見出し")]
    fn test_heading_line_attrsは見出し開始なしならdebug_assertで検知する() {
        let line_lookup = LineLookup::new("# title");
        let state = RenderState::new();

        let _ = heading_line_attrs(&line_lookup, &state);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "code_block_line_attrs: アクティブなコードブロック")]
    fn test_code_block_line_attrsはコードブロック開始なしならdebug_assertで検知する() {
        let line_lookup = LineLookup::new("```rust\nfn main() {}\n```");
        let state = RenderState::new();

        let _ = code_block_line_attrs(&(0..0), &line_lookup, &state);
    }

    #[test]
    fn test_未処理markdown_eventは観測対象として分類される() {
        let event = Event::InlineMath(pulldown_cmark::CowStr::from("x"));

        assert_eq!(
            log_ignored_markdown_event(&event),
            IgnoredMarkdownEventKind::Event
        );
    }

    #[test]
    fn test_未処理markdown_start_tagは観測対象として分類される() {
        assert_eq!(
            log_ignored_markdown_start_tag(&Tag::HtmlBlock),
            IgnoredMarkdownEventKind::StartTag
        );
    }

    #[test]
    fn test_未処理markdown_end_tagは観測対象として分類される() {
        assert_eq!(
            log_ignored_markdown_end_tag(&TagEnd::HtmlBlock),
            IgnoredMarkdownEventKind::EndTag
        );
    }
}
