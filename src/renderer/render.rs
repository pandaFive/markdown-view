use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Parser, Tag, TagEnd};
use syntect::parsing::SyntaxSet;

use crate::markdown::{markdown_options, MarkdownProfile};

use super::line::{block_line_attrs, line_block_marker_with, source_line_attrs, LineLookup};
use super::security::{html_escape, sanitize_link_href};
use super::state::{RenderState, RenderStateMismatch};
use super::{generate_unique_id, slugify, syntax_set, HeadingInfo, SanitizedHtml};

pub(super) struct RenderOutput {
    pub(super) content: SanitizedHtml,
    pub(super) headings: Vec<HeadingInfo>,
}

type MarkdownEvent<'a> = (Event<'a>, Range<usize>);

pub(super) fn render(input: &str) -> RenderOutput {
    let line_lookup = LineLookup::new(input);
    let syntax_set = syntax_set();
    let mut context = RenderContext::new();

    let parser = Parser::new_ext(input, markdown_options(MarkdownProfile::Render))
        .into_offset_iter()
        .collect();
    for (event, range) in normalize_cjk_adjacent_strong(input, parser) {
        dispatch_event(event, range, &line_lookup, syntax_set, &mut context);
    }

    let RenderContext {
        state, headings, ..
    } = context;

    // 通常の pulldown-cmark 経路では起きない。発火時は parser 更新か state machine の不変条件違反。
    debug_assert!(
        !state.has_mismatches(),
        "render: Markdown描画状態の不整合が記録された: {:?}",
        state.first_mismatch()
    );

    RenderOutput {
        content: SanitizedHtml::from_sanitized_html(state.into_html_with_recovery_warning()),
        headings,
    }
}

fn normalize_cjk_adjacent_strong<'a>(
    input: &str,
    events: Vec<MarkdownEvent<'a>>,
) -> Vec<MarkdownEvent<'a>> {
    let mut normalized = Vec::with_capacity(events.len());
    let mut index = 0;

    while index < events.len() {
        if is_cjk_adjacent_strong_sequence(input, &events, index) {
            normalized.push((Event::Start(Tag::Strong), events[index].1.clone()));
            normalized.push(events[index + 2].clone());
            normalized.push((Event::End(TagEnd::Strong), events[index + 3].1.clone()));
            index += 5;
            continue;
        }

        normalized.push(events[index].clone());
        index += 1;
    }

    normalized
}

fn is_cjk_adjacent_strong_sequence(
    input: &str,
    events: &[MarkdownEvent<'_>],
    index: usize,
) -> bool {
    let Some(window) = events.get(index..index + 6) else {
        return false;
    };
    is_source_text_marker(input, &window[0], "*")
        && is_source_text_marker(input, &window[1], "*")
        && text_has_non_whitespace_edges(&window[2].0)
        && is_source_text_marker(input, &window[3], "*")
        && is_source_text_marker(input, &window[4], "*")
        && text_starts_with_cjk(&window[5].0)
}

fn is_source_text_marker(input: &str, event: &MarkdownEvent<'_>, marker: &str) -> bool {
    matches!(&event.0, Event::Text(text) if text.as_ref() == marker)
        && input.get(event.1.clone()) == Some(marker)
}

fn text_has_non_whitespace_edges(event: &Event<'_>) -> bool {
    match event {
        Event::Text(text) => {
            text.chars().next().is_some_and(|c| !c.is_whitespace())
                && text.chars().next_back().is_some_and(|c| !c.is_whitespace())
        }
        _ => false,
    }
}

fn text_starts_with_cjk(event: &Event<'_>) -> bool {
    match event {
        Event::Text(text) => text.chars().next().is_some_and(is_cjk_char),
        _ => false,
    }
}

fn is_cjk_char(c: char) -> bool {
    matches!(
        c,
        '\u{3040}'..='\u{30ff}'
            | '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{ac00}'..='\u{d7af}'
            | '\u{f900}'..='\u{faff}'
            | '\u{ff00}'..='\u{ffef}'
            | '\u{20000}'..='\u{323af}'
    )
}

struct RenderContext {
    id_counts: HashMap<String, usize>,
    headings: Vec<HeadingInfo>,
    state: RenderState,
}

impl RenderContext {
    fn new() -> Self {
        Self {
            id_counts: HashMap::new(),
            headings: Vec::new(),
            state: RenderState::new(),
        }
    }
}

fn dispatch_event(
    event: Event<'_>,
    range: Range<usize>,
    line_lookup: &LineLookup,
    syntax_set: &SyntaxSet,
    context: &mut RenderContext,
) {
    match event {
        Event::Start(tag) => handle_start(tag, range, line_lookup, &mut context.state),
        Event::End(tag) => handle_end(tag, range, line_lookup, syntax_set, context),
        Event::Text(text) => handle_text(&text, &range, line_lookup, &mut context.state),
        Event::Code(text) => handle_code(&text, &range, line_lookup, &mut context.state),
        Event::Html(_) | Event::InlineHtml(_) => handle_html(),
        Event::SoftBreak => handle_soft_break(&mut context.state),
        Event::HardBreak => handle_hard_break(&mut context.state),
        Event::Rule => handle_rule(&mut context.state),
        Event::TaskListMarker(checked) => {
            handle_task_list_marker(checked, &mut context.state);
        }
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
    context: &mut RenderContext,
) {
    match tag {
        TagEnd::CodeBlock => {
            handle_code_block_end(range, line_lookup, syntax_set, &mut context.state)
        }
        TagEnd::Heading(_) => handle_heading_end(line_lookup, context),
        TagEnd::Image => handle_image_end(&mut context.state),
        TagEnd::Paragraph => handle_paragraph_end(&mut context.state),
        TagEnd::Emphasis => handle_emphasis_end(&mut context.state),
        TagEnd::Strong => handle_strong_end(&mut context.state),
        TagEnd::Strikethrough => handle_strikethrough_end(&mut context.state),
        TagEnd::Link => handle_link_end(&mut context.state),
        TagEnd::BlockQuote(_) => handle_blockquote_end(&mut context.state),
        TagEnd::List(true) => handle_ordered_list_end(&mut context.state),
        TagEnd::List(false) => handle_unordered_list_end(&mut context.state),
        TagEnd::Item => handle_item_end(&mut context.state),
        TagEnd::Table => handle_table_end(&mut context.state),
        TagEnd::TableHead => handle_table_head_end(&mut context.state),
        TagEnd::TableRow => handle_table_row_end(&mut context.state),
        TagEnd::TableCell => handle_table_cell_end(&mut context.state),
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
    tracing::debug!("[markdown-view] 未処理のMarkdown開始タグを無視: {:?}", tag);
    IgnoredMarkdownEventKind::StartTag
}

fn log_ignored_markdown_end_tag(tag: &TagEnd) -> IgnoredMarkdownEventKind {
    tracing::debug!("[markdown-view] 未処理のMarkdown終了タグを無視: {:?}", tag);
    IgnoredMarkdownEventKind::EndTag
}

fn recover_render_state_mismatch(
    state: &mut RenderState,
    operation: &'static str,
    mismatch: RenderStateMismatch,
    range: Option<Range<usize>>,
) {
    state.record_mismatch(operation, mismatch, range.clone());
    tracing::warn!(
        "[markdown-view] Markdown描画状態の不整合を回復: operation={} mismatch={:?} range={:?}",
        operation,
        mismatch,
        range
    );
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
    let Some(line_attrs) = code_block_line_attrs(&range, line_lookup, state) else {
        recover_render_state_mismatch(
            state,
            "finish_code_block",
            RenderStateMismatch::ExpectedCodeBlock,
            Some(range),
        );
        return;
    };
    if let Err(mismatch) = state.finish_code_block(syntax_set, line_attrs) {
        recover_render_state_mismatch(state, "finish_code_block", mismatch, Some(range));
    }
}

fn handle_heading_start(level: u8, range: Range<usize>, state: &mut RenderState) {
    state.start_heading(level, range);
}

fn handle_heading_end(line_lookup: &LineLookup, context: &mut RenderContext) {
    let state = &mut context.state;
    if !state.can_finish_heading() {
        recover_render_state_mismatch(
            state,
            "finish_heading",
            RenderStateMismatch::ExpectedHeading,
            None,
        );
        return;
    }

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
        Err(mismatch) => recover_render_state_mismatch(state, "finish_heading", mismatch, None),
    }
}

fn handle_image_start(dest_url: &str, title: &str, state: &mut RenderState) {
    state.start_image(dest_url, title);
}

fn handle_image_end(state: &mut RenderState) {
    match state.finish_image() {
        Ok(image_html) => {
            if state.in_heading() {
                state.push_heading_rendered_html_fragment(&image_html);
            } else {
                state.push_html(&image_html);
            }
        }
        Err(mismatch) => recover_render_state_mismatch(state, "finish_image", mismatch, None),
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
    match state.finish_table() {
        Ok(()) => state.push_html("</table>\n"),
        Err(mismatch) => recover_render_state_mismatch(state, "finish_table", mismatch, None),
    }
}

fn handle_table_head_start(state: &mut RenderState) {
    match state.start_table_head() {
        Ok(()) => state.push_html("<thead>\n"),
        Err(mismatch) => recover_render_state_mismatch(state, "start_table_head", mismatch, None),
    }
}

fn handle_table_head_end(state: &mut RenderState) {
    match state.finish_table_head() {
        Ok(()) => state.push_html("</thead>\n"),
        Err(mismatch) => recover_render_state_mismatch(state, "finish_table_head", mismatch, None),
    }
}

fn handle_table_row_start(state: &mut RenderState) {
    match state.reset_table_row() {
        Ok(()) => state.push_html("<tr>\n"),
        Err(mismatch) => recover_render_state_mismatch(state, "reset_table_row", mismatch, None),
    }
}

fn handle_table_row_end(state: &mut RenderState) {
    match state.finish_table_row() {
        Ok(()) => state.push_html("</tr>\n"),
        Err(mismatch) => recover_render_state_mismatch(state, "finish_table_row", mismatch, None),
    }
}

fn handle_table_cell_start(state: &mut RenderState) {
    match state.table_cell_start_tag() {
        Ok(tag) => state.push_html(&tag),
        Err(mismatch) => {
            recover_render_state_mismatch(state, "table_cell_start_tag", mismatch, None)
        }
    }
}

fn handle_table_cell_end(state: &mut RenderState) {
    match state.table_cell_end_tag() {
        Ok(tag) => state.push_html(tag),
        Err(mismatch) => recover_render_state_mismatch(state, "table_cell_end_tag", mismatch, None),
    }
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
) -> Option<String> {
    let range = state.code_block_full_range(end_range);
    range.map(|range| line_block_marker_with(source_line_attrs(line_lookup, &range)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_cjk_charはhangulとcjk拡張漢字を含む() {
        assert!(is_cjk_char('は'));
        assert!(is_cjk_char('漢'));
        assert!(is_cjk_char('각'));
        assert!(is_cjk_char('𠀋'));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "heading_line_attrs: アクティブな見出し")]
    fn test_heading_line_attrsは見出し開始なしならdebug_assertで検知する() {
        let line_lookup = LineLookup::new("# title");
        let state = RenderState::new();

        let _ = heading_line_attrs(&line_lookup, &state);
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn test_heading_line_attrsはrelease_fallbackで空属性を返す() {
        let line_lookup = LineLookup::new("# title");
        let state = RenderState::new();

        assert_eq!(heading_line_attrs(&line_lookup, &state), "");
    }

    #[test]
    fn test_code_block_line_attrsはコードブロック開始なしならnoneを返す() {
        let line_lookup = LineLookup::new("```rust\nfn main() {}\n```");
        let state = RenderState::new();

        assert_eq!(code_block_line_attrs(&(0..0), &line_lookup, &state), None);
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

    #[test]
    fn test_table_row_endはtableなしなら閉じタグを出さない() {
        let mut state = RenderState::new();

        handle_table_row_end(&mut state);

        assert_eq!(state.mismatch_count(), 1);
        assert_eq!(state.into_html(), "");
    }

    #[test]
    fn test_table_row_endはrow未開始なら閉じタグを出さない() {
        let mut state = RenderState::new();

        state.start_table(vec![Alignment::Left]);
        handle_table_row_end(&mut state);

        assert_eq!(
            state.first_mismatch().unwrap().operation,
            "finish_table_row"
        );
        assert_eq!(
            state.first_mismatch().unwrap().mismatch,
            RenderStateMismatch::ExpectedTableRow
        );
        assert_eq!(state.into_html(), "");
    }

    #[test]
    fn test_table系handlerはwrong_topならhtmlを出さずmismatchを記録する() {
        let mut state = RenderState::new();

        state.start_table(vec![Alignment::Left]);
        state.start_image("image.png", "");

        handle_table_head_start(&mut state);
        handle_table_head_end(&mut state);
        handle_table_row_start(&mut state);
        handle_table_cell_start(&mut state);
        handle_table_cell_end(&mut state);
        handle_table_end(&mut state);

        assert_eq!(state.mismatch_count(), 6);
        assert_eq!(state.into_html(), "");
    }

    #[test]
    fn test_mismatchはoperation_mismatch_rangeを記録する() {
        let line_lookup = LineLookup::new("```rust\ncode\n```");
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let mut state = RenderState::new();

        state.start_image("image.png", "");
        handle_code_block_end(3..9, &line_lookup, &syntax_set, &mut state);

        let mismatch = state.first_mismatch().expect("mismatchが記録される");
        assert_eq!(mismatch.operation, "finish_code_block");
        assert_eq!(mismatch.mismatch, RenderStateMismatch::ExpectedCodeBlock);
        assert_eq!(mismatch.range, Some(3..9));
    }

    #[test]
    fn test_mismatchがあれば本文末尾に回復warningを追加する() {
        let mut state = RenderState::new();

        handle_table_row_end(&mut state);

        let html = state.into_html_with_recovery_warning();
        assert!(html.contains("render-recovery-warning"));
        assert!(html.contains("role=\"status\""));
    }

    #[test]
    fn test_heading_endは上位imageがあるならid_counterを進めない() {
        let line_lookup = LineLookup::new("# title");
        let mut context = RenderContext::new();

        context.state.start_heading(1, 0..7);
        context
            .state
            .push_heading_escaped_text_html("title", "title");
        context.state.start_image("image.png", "");

        handle_heading_end(&line_lookup, &mut context);

        assert_eq!(context.state.mismatch_count(), 1);
        assert!(context.headings.is_empty());
        let image_html = context.state.finish_image().unwrap();
        context
            .state
            .push_heading_rendered_html_fragment(&image_html);

        handle_heading_end(&line_lookup, &mut context);

        assert_eq!(context.headings.len(), 1);
        assert_eq!(context.headings[0].id, "title");
        let html = context.state.into_html();
        assert!(html.contains("id=\"title\""));
        assert!(!html.contains("id=\"title-1\""));
    }

    #[test]
    fn test_code_block_endは上位imageがあるならline_attrs評価前に回復する() {
        let line_lookup = LineLookup::new("```rust\ncode\n```");
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let mut state = RenderState::new();

        state.start_code_block(CodeBlockKind::Indented, 0..14);
        state.push_code_text("code");
        state.start_image("image.png", "");

        handle_code_block_end(0..14, &line_lookup, &syntax_set, &mut state);

        assert_eq!(state.mismatch_count(), 1);
        assert!(state.finish_image().is_ok());
        assert!(state.finish_code_block(&syntax_set, String::new()).is_ok());
    }
}
