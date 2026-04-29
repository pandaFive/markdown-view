use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use syntect::parsing::SyntaxSet;

use super::line::{block_line_attrs, source_line_attrs, LineLookup};
use super::security::{html_escape, sanitize_link_href};
use super::state::RenderState;
use super::{generate_unique_id, markdown_options, slugify, syntax_set, SanitizedHtml};

pub(super) struct RenderOptions {
    track_source_lines: bool,
    syntax_highlighting: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            track_source_lines: true,
            syntax_highlighting: true,
        }
    }
}

struct RenderContext {
    line_lookup: LineLookup,
    syntax_set: &'static SyntaxSet,
    id_counts: HashMap<String, usize>,
    options: RenderOptions,
}

pub(super) struct Renderer<'a> {
    input: &'a str,
    context: RenderContext,
    state: RenderState,
}

impl<'a> Renderer<'a> {
    pub(super) fn new(input: &'a str, options: RenderOptions) -> Self {
        Self {
            input,
            context: RenderContext {
                line_lookup: LineLookup::new(input),
                syntax_set: syntax_set(),
                id_counts: HashMap::new(),
                options,
            },
            state: RenderState::new(),
        }
    }

    pub(super) fn render(mut self) -> SanitizedHtml {
        let parser = Parser::new_ext(self.input, markdown_options()).into_offset_iter();
        for (event, range) in parser {
            self.dispatch_event(event, range);
        }
        SanitizedHtml::from_sanitized_html(self.state.into_html())
    }

    fn dispatch_event(&mut self, event: Event<'a>, range: Range<usize>) {
        match event {
            Event::Start(tag) => self.handle_start(tag, range),
            Event::End(tag) => self.handle_end(tag, range),
            Event::Text(text) => self.handle_text(&text, &range),
            Event::Code(text) => self.handle_code(&text, &range),
            Event::Html(_) | Event::InlineHtml(_) => self.handle_html(),
            Event::SoftBreak => self.handle_soft_break(),
            Event::HardBreak => self.handle_hard_break(),
            Event::Rule => self.handle_rule(),
            Event::TaskListMarker(checked) => self.handle_task_list_marker(checked),
            other => {
                tracing::debug!(
                    "[markdown-view] 未処理のMarkdownイベントを無視: {:?}",
                    other
                );
            }
        }
    }

    fn handle_start(&mut self, tag: Tag<'a>, range: Range<usize>) {
        match tag {
            Tag::CodeBlock(kind) => self.handle_code_block_start(kind, range),
            Tag::Heading { level, .. } => self.handle_heading_start(level as u8, range),
            Tag::Image {
                dest_url, title, ..
            } => self.handle_image_start(&dest_url, &title),
            Tag::Paragraph => self.handle_paragraph_start(&range),
            Tag::Emphasis => self.handle_emphasis_start(),
            Tag::Strong => self.handle_strong_start(),
            Tag::Strikethrough => self.handle_strikethrough_start(),
            Tag::Link {
                dest_url, title, ..
            } => self.handle_link_start(&dest_url, &title),
            Tag::BlockQuote(_) => self.handle_blockquote_start(&range),
            Tag::List(Some(start)) => self.handle_ordered_list_start(start, &range),
            Tag::List(None) => self.handle_unordered_list_start(&range),
            Tag::Item => self.handle_item_start(&range),
            Tag::Table(alignments) => self.handle_table_start(alignments, &range),
            Tag::TableHead => self.handle_table_head_start(),
            Tag::TableRow => self.handle_table_row_start(),
            Tag::TableCell => self.handle_table_cell_start(),
            other => {
                tracing::debug!(
                    "[markdown-view] 未処理のMarkdown開始タグを無視: {:?}",
                    other
                );
            }
        }
    }

    fn handle_end(&mut self, tag: TagEnd, range: Range<usize>) {
        match tag {
            TagEnd::CodeBlock => self.handle_code_block_end(range),
            TagEnd::Heading(_) => self.handle_heading_end(),
            TagEnd::Image => self.handle_image_end(),
            TagEnd::Paragraph => self.handle_paragraph_end(),
            TagEnd::Emphasis => self.handle_emphasis_end(),
            TagEnd::Strong => self.handle_strong_end(),
            TagEnd::Strikethrough => self.handle_strikethrough_end(),
            TagEnd::Link => self.handle_link_end(),
            TagEnd::BlockQuote(_) => self.handle_blockquote_end(),
            TagEnd::List(true) => self.handle_ordered_list_end(),
            TagEnd::List(false) => self.handle_unordered_list_end(),
            TagEnd::Item => self.handle_item_end(),
            TagEnd::Table => self.handle_table_end(),
            TagEnd::TableHead => self.handle_table_head_end(),
            TagEnd::TableRow => self.handle_table_row_end(),
            TagEnd::TableCell => self.handle_table_cell_end(),
            other => {
                tracing::debug!(
                    "[markdown-view] 未処理のMarkdown終了タグを無視: {:?}",
                    other
                );
            }
        }
    }

    fn handle_text(&mut self, text: &str, range: &Range<usize>) {
        if self.state.in_code_block() {
            self.state.push_code_text(text);
            return;
        }

        if self.state.in_image() {
            self.state.push_image_alt_text(text);
            return;
        }

        let line_attrs = self.source_line_attrs(range);
        let html = format!("<span{}>{}</span>", line_attrs, html_escape(text));
        if self.state.in_heading() {
            self.state.push_heading_escaped_text_html(text, &html);
        } else {
            self.state.push_html(&html);
        }
    }

    fn handle_code(&mut self, text: &str, range: &Range<usize>) {
        if self.state.in_image() {
            self.state.push_image_alt_text(text);
            return;
        }

        let line_attrs = self.source_line_attrs(range);
        let html = format!("<code{}>{}</code>", line_attrs, html_escape(text));
        if self.state.in_heading() {
            self.state.push_heading_escaped_text_html(text, &html);
        } else {
            self.state.push_html(&html);
        }
    }

    fn handle_html(&mut self) {
        // raw HTMLイベントは出力せず破棄する（XSS防止）
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
            self.state.push_heading_escaped_text_html(" ", "<br />");
        } else {
            self.state.push_html("<br />\n");
        }
    }

    fn handle_rule(&mut self) {
        self.state.push_html("<hr />\n");
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

    fn handle_code_block_start(
        &mut self,
        kind: pulldown_cmark::CodeBlockKind<'a>,
        range: Range<usize>,
    ) {
        self.state.start_code_block(kind, range);
    }

    fn handle_code_block_end(&mut self, range: Range<usize>) {
        self.state.finish_code_block(
            self.context.syntax_set,
            range,
            &self.context.line_lookup,
            self.context.options.syntax_highlighting,
        );
    }

    fn handle_heading_start(&mut self, level: u8, range: Range<usize>) {
        self.state.start_heading(level, range);
    }

    fn handle_heading_end(&mut self) {
        let slug = slugify(self.state.heading_plain_text());
        let id = generate_unique_id(&slug, &mut self.context.id_counts);
        if let Some(heading_html) = self.state.finish_heading(&self.context.line_lookup, id) {
            self.state.push_html(&heading_html);
        }
    }

    fn handle_image_start(&mut self, dest_url: &str, title: &str) {
        self.state.start_image(dest_url, title);
    }

    fn handle_image_end(&mut self) {
        if let Some(image_html) = self.state.finish_image() {
            if self.state.in_heading() {
                self.state.push_heading_safe_html(&image_html);
            } else {
                self.state.push_html(&image_html);
            }
        }
    }

    fn handle_paragraph_start(&mut self, range: &Range<usize>) {
        let attrs = self.block_line_attrs(range);
        self.state.push_html(&format!("<p{}>", attrs));
    }

    fn handle_paragraph_end(&mut self) {
        self.state.push_html("</p>\n");
    }

    fn handle_emphasis_start(&mut self) {
        self.push_inline_tag("<em>");
    }

    fn handle_emphasis_end(&mut self) {
        self.push_inline_tag("</em>");
    }

    fn handle_strong_start(&mut self) {
        self.push_inline_tag("<strong>");
    }

    fn handle_strong_end(&mut self) {
        self.push_inline_tag("</strong>");
    }

    fn handle_strikethrough_start(&mut self) {
        self.push_inline_tag("<del>");
    }

    fn handle_strikethrough_end(&mut self) {
        self.push_inline_tag("</del>");
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
        self.push_rendered_inline(&link_html);
    }

    fn handle_link_end(&mut self) {
        self.push_inline_tag("</a>");
    }

    fn handle_blockquote_start(&mut self, range: &Range<usize>) {
        let attrs = self.block_line_attrs(range);
        self.state.push_html(&format!("<blockquote{}>\n", attrs));
    }

    fn handle_blockquote_end(&mut self) {
        self.state.push_html("</blockquote>\n");
    }

    fn handle_ordered_list_start(&mut self, start: u64, range: &Range<usize>) {
        let attrs = self.block_line_attrs(range);
        self.state
            .push_html(&format!("<ol start=\"{}\"{}>\n", start, attrs));
    }

    fn handle_ordered_list_end(&mut self) {
        self.state.push_html("</ol>\n");
    }

    fn handle_unordered_list_start(&mut self, range: &Range<usize>) {
        let attrs = self.block_line_attrs(range);
        self.state.push_html(&format!("<ul{}>\n", attrs));
    }

    fn handle_unordered_list_end(&mut self) {
        self.state.push_html("</ul>\n");
    }

    fn handle_item_start(&mut self, range: &Range<usize>) {
        let attrs = self.block_line_attrs(range);
        self.state.push_html(&format!("<li{}>", attrs));
    }

    fn handle_item_end(&mut self) {
        self.state.push_html("</li>\n");
    }

    fn handle_table_start(
        &mut self,
        alignments: Vec<pulldown_cmark::Alignment>,
        range: &Range<usize>,
    ) {
        let attrs = self.block_line_attrs(range);
        self.state.push_html(&format!("<table{}>\n", attrs));
        self.state.start_table(alignments);
    }

    fn handle_table_end(&mut self) {
        self.state.push_html("</table>\n");
        self.state.finish_table();
    }

    fn handle_table_head_start(&mut self) {
        self.state.start_table_head();
        self.state.push_html("<thead>\n");
    }

    fn handle_table_head_end(&mut self) {
        self.state.push_html("</thead>\n");
        self.state.finish_table_head();
    }

    fn handle_table_row_start(&mut self) {
        self.state.push_html("<tr>\n");
        self.state.reset_table_row();
    }

    fn handle_table_row_end(&mut self) {
        self.state.push_html("</tr>\n");
    }

    fn handle_table_cell_start(&mut self) {
        let tag = self.state.table_cell_start_tag();
        self.state.push_html(&tag);
    }

    fn handle_table_cell_end(&mut self) {
        self.state.push_html(self.state.table_cell_end_tag());
    }

    fn push_inline_tag(&mut self, html: &'static str) {
        if self.state.in_image() {
            return;
        }
        self.push_rendered_inline(html);
    }

    fn push_rendered_inline(&mut self, html: &str) {
        if self.state.in_heading() {
            self.state.push_heading_safe_html(html);
        } else {
            self.state.push_html(html);
        }
    }

    fn source_line_attrs(&self, range: &Range<usize>) -> String {
        if self.context.options.track_source_lines {
            source_line_attrs(&self.context.line_lookup, range)
        } else {
            String::new()
        }
    }

    fn block_line_attrs(&self, range: &Range<usize>) -> String {
        if self.context.options.track_source_lines {
            block_line_attrs(&self.context.line_lookup, range)
        } else {
            String::new()
        }
    }
}
