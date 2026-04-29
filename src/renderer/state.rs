use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind};
use syntect::parsing::SyntaxSet;

use super::highlight::render_code_block_html;
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

    pub(super) fn push_heading_escaped_text_html(&mut self, text: &str, html: &str) {
        self.heading_plain_text.push_str(text);
        self.heading_html.push_str(html);
    }

    pub(super) fn push_heading_space(&mut self) {
        self.heading_plain_text.push(' ');
        self.heading_html.push(' ');
    }

    pub(super) fn push_heading_safe_html(&mut self, html: &str) {
        self.heading_html.push_str(html);
    }

    pub(super) fn start_heading(&mut self, level: u8, range: Range<usize>) {
        self.heading_level = Some(level);
        self.heading_range = Some(range);
        self.heading_plain_text.clear();
        self.heading_html.clear();
    }

    pub(super) fn finish_heading(&mut self, id: String, heading_attrs: String) -> Option<String> {
        let level = self.heading_level?;
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
        self.heading_plain_text.clear();
        self.heading_html.clear();
        Some(html)
    }

    pub(super) fn heading_plain_text(&self) -> &str {
        &self.heading_plain_text
    }

    pub(super) fn heading_range(&self) -> Option<&Range<usize>> {
        self.heading_range.as_ref()
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

    pub(super) fn code_block_full_range(&self, end_range: &Range<usize>) -> Option<Range<usize>> {
        self.code_block_range.as_ref().map(|start_range| Range {
            start: start_range.start,
            end: end_range.end,
        })
    }

    pub(super) fn finish_code_block(
        &mut self,
        ss: &SyntaxSet,
        line_attrs: String,
        syntax_highlighting: bool,
    ) {
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
