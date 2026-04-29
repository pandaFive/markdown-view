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

    /// アクティブなコードブロックがある状態でのみ呼ぶ。
    pub(super) fn finish_code_block(&mut self, ss: &SyntaxSet, line_attrs: String) {
        let Some(code_block) = self.code_block.take() else {
            unreachable!("finish_code_block: アクティブなコードブロックがない状態で呼ばれた");
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

    /// アクティブなテーブルがある状態でのみ呼ぶ。
    pub(super) fn start_table_head(&mut self) {
        let Some(table) = &mut self.table else {
            unreachable!("start_table_head: アクティブなテーブルがない状態で呼ばれた");
        };
        table.in_head = true;
    }

    /// アクティブなテーブルがある状態でのみ呼ぶ。
    pub(super) fn finish_table_head(&mut self) {
        let Some(table) = &mut self.table else {
            unreachable!("finish_table_head: アクティブなテーブルがない状態で呼ばれた");
        };
        table.in_head = false;
    }

    /// アクティブなテーブルがある状態でのみ呼ぶ。
    pub(super) fn reset_table_row(&mut self) {
        let Some(table) = &mut self.table else {
            unreachable!("reset_table_row: アクティブなテーブルがない状態で呼ばれた");
        };
        table.cell_index = 0;
    }

    /// アクティブなテーブルがある状態でのみ呼ぶ。
    pub(super) fn table_cell_start_tag(&mut self) -> String {
        let Some(table) = &mut self.table else {
            unreachable!("table_cell_start_tag: アクティブなテーブルがない状態で呼ばれた");
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

    /// アクティブなテーブルがある状態でのみ呼ぶ。
    pub(super) fn table_cell_end_tag(&self) -> &'static str {
        let Some(table) = &self.table else {
            unreachable!("table_cell_end_tag: アクティブなテーブルがない状態で呼ばれた");
        };
        if table.in_head {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "アクティブなコードブロック")]
    fn test_finish_code_blockは開始なしならpanicする() {
        let mut state = RenderState::new();
        let syntax_set = SyntaxSet::load_defaults_newlines();

        state.finish_code_block(&syntax_set, String::new());
    }

    #[test]
    #[should_panic(expected = "アクティブなテーブル")]
    fn test_start_table_headはtable開始なしならpanicする() {
        let mut state = RenderState::new();

        state.start_table_head();
    }

    #[test]
    #[should_panic(expected = "アクティブなテーブル")]
    fn test_finish_table_headはtable開始なしならpanicする() {
        let mut state = RenderState::new();

        state.finish_table_head();
    }

    #[test]
    #[should_panic(expected = "アクティブなテーブル")]
    fn test_reset_table_rowはtable開始なしならpanicする() {
        let mut state = RenderState::new();

        state.reset_table_row();
    }

    #[test]
    #[should_panic(expected = "アクティブなテーブル")]
    fn test_table_cell_start_tagはtable開始なしならpanicする() {
        let mut state = RenderState::new();

        let _ = state.table_cell_start_tag();
    }

    #[test]
    #[should_panic(expected = "アクティブなテーブル")]
    fn test_table_cell_end_tagはtable開始なしならpanicする() {
        let state = RenderState::new();

        let _ = state.table_cell_end_tag();
    }
}
