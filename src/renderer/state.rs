use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind};
use syntect::parsing::SyntaxSet;

use super::highlight::render_code_block_html;
use super::security::{html_escape, sanitize_image_src};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// mismatchログとテストの読みやすさを揃えるため、Expected* の名前で統一する。
#[allow(clippy::enum_variant_names)]
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
            contexts: Vec::new(),
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

    fn top_context(&self) -> Option<&BlockContext> {
        self.contexts.last()
    }

    fn top_context_mut(&mut self) -> Option<&mut BlockContext> {
        self.contexts.last_mut()
    }

    fn heading(&self) -> Option<&HeadingState> {
        match self.top_context() {
            Some(BlockContext::Heading(heading)) => Some(heading),
            _ => None,
        }
    }

    fn heading_mut(&mut self) -> Option<&mut HeadingState> {
        match self.top_context_mut() {
            Some(BlockContext::Heading(heading)) => Some(heading),
            _ => None,
        }
    }

    fn code_block(&self) -> Option<&CodeBlockState> {
        match self.top_context() {
            Some(BlockContext::CodeBlock(code_block)) => Some(code_block),
            _ => None,
        }
    }

    fn code_block_mut(&mut self) -> Option<&mut CodeBlockState> {
        match self.top_context_mut() {
            Some(BlockContext::CodeBlock(code_block)) => Some(code_block),
            _ => None,
        }
    }

    fn image_mut(&mut self) -> Option<&mut ImageState> {
        match self.top_context_mut() {
            Some(BlockContext::Image(image)) => Some(image),
            _ => None,
        }
    }

    fn table(&self) -> Option<&TableState> {
        match self.top_context() {
            Some(BlockContext::Table(table)) => Some(table),
            _ => None,
        }
    }

    fn table_mut(&mut self) -> Option<&mut TableState> {
        match self.top_context_mut() {
            Some(BlockContext::Table(table)) => Some(table),
            _ => None,
        }
    }

    fn pop_heading(&mut self) -> Result<HeadingState, RenderStateMismatch> {
        match self.contexts.pop() {
            Some(BlockContext::Heading(heading)) => Ok(heading),
            Some(context) => {
                self.contexts.push(context);
                Err(RenderStateMismatch::ExpectedHeading)
            }
            None => Err(RenderStateMismatch::ExpectedHeading),
        }
    }

    fn pop_code_block(&mut self) -> Result<CodeBlockState, RenderStateMismatch> {
        match self.contexts.pop() {
            Some(BlockContext::CodeBlock(code_block)) => Ok(code_block),
            Some(context) => {
                self.contexts.push(context);
                Err(RenderStateMismatch::ExpectedCodeBlock)
            }
            None => Err(RenderStateMismatch::ExpectedCodeBlock),
        }
    }

    fn pop_image(&mut self) -> Result<ImageState, RenderStateMismatch> {
        match self.contexts.pop() {
            Some(BlockContext::Image(image)) => Ok(image),
            Some(context) => {
                self.contexts.push(context);
                Err(RenderStateMismatch::ExpectedImage)
            }
            None => Err(RenderStateMismatch::ExpectedImage),
        }
    }

    fn pop_table(&mut self) -> Result<TableState, RenderStateMismatch> {
        match self.contexts.pop() {
            Some(BlockContext::Table(table)) => Ok(table),
            Some(context) => {
                self.contexts.push(context);
                Err(RenderStateMismatch::ExpectedTable)
            }
            None => Err(RenderStateMismatch::ExpectedTable),
        }
    }

    pub(super) fn in_code_block(&self) -> bool {
        matches!(self.top_context(), Some(BlockContext::CodeBlock(_)))
    }

    pub(super) fn in_image(&self) -> bool {
        matches!(self.top_context(), Some(BlockContext::Image(_)))
    }

    pub(super) fn in_heading(&self) -> bool {
        self.heading().is_some()
    }

    pub(super) fn can_finish_heading(&self) -> bool {
        matches!(self.top_context(), Some(BlockContext::Heading(_)))
    }

    pub(super) fn in_table(&self) -> bool {
        matches!(self.top_context(), Some(BlockContext::Table(_)))
    }

    pub(super) fn push_code_text(&mut self, text: &str) {
        if let Some(code_block) = self.code_block_mut() {
            code_block.content.push_str(text);
        }
    }

    pub(super) fn push_code_break(&mut self) {
        if let Some(code_block) = self.code_block_mut() {
            code_block.content.push('\n');
        }
    }

    pub(super) fn push_image_alt_text(&mut self, text: &str) {
        if let Some(image) = self.image_mut() {
            image.alt.push_str(text);
        }
    }

    pub(super) fn push_image_alt_space(&mut self) {
        if let Some(image) = self.image_mut() {
            image.alt.push(' ');
        }
    }

    pub(super) fn push_heading_escaped_text_html(&mut self, text: &str, html: &str) {
        if let Some(heading) = self.heading_mut() {
            heading.plain_text.push_str(text);
            heading.html.push_str(html);
        }
    }

    pub(super) fn push_heading_space(&mut self) {
        if let Some(heading) = self.heading_mut() {
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
        if let Some(heading) = self.heading_mut() {
            heading.html.push_str(html);
        }
    }

    pub(super) fn start_heading(&mut self, level: u8, range: Range<usize>) {
        self.contexts.push(BlockContext::Heading(HeadingState {
            level,
            range,
            plain_text: String::new(),
            html: String::new(),
        }));
    }

    pub(super) fn finish_heading(
        &mut self,
        id: String,
        heading_attrs: String,
    ) -> Result<String, RenderStateMismatch> {
        let heading = self.pop_heading()?;
        Ok(format!(
            "<h{} id=\"{}\"{}>{}</h{}>\n",
            heading.level,
            html_escape(&id),
            heading_attrs,
            heading.html,
            heading.level
        ))
    }

    pub(super) fn heading_plain_text(&self) -> &str {
        self.heading()
            .map(|heading| heading.plain_text.as_str())
            .unwrap_or("")
    }

    pub(super) fn heading_level(&self) -> Option<u8> {
        self.heading().map(|heading| heading.level)
    }

    pub(super) fn heading_range(&self) -> Option<&Range<usize>> {
        self.heading().map(|heading| &heading.range)
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
        self.contexts.push(BlockContext::CodeBlock(CodeBlockState {
            language,
            content: String::new(),
            start_range: range,
        }));
    }

    pub(super) fn code_block_full_range(&self, end_range: &Range<usize>) -> Option<Range<usize>> {
        self.code_block().map(|code_block| Range {
            start: code_block.start_range.start,
            end: end_range.end,
        })
    }

    pub(super) fn finish_code_block(
        &mut self,
        ss: &SyntaxSet,
        line_attrs: String,
    ) -> Result<(), RenderStateMismatch> {
        let code_block = self.pop_code_block()?;
        let rendered = render_code_block_html(
            ss,
            code_block.language.as_deref(),
            &code_block.content,
            &line_attrs,
        );
        self.push_html(&rendered);
        Ok(())
    }

    pub(super) fn start_image(&mut self, dest_url: &str, title: &str) {
        self.contexts.push(BlockContext::Image(ImageState {
            src: dest_url.to_string(),
            title: if title.is_empty() {
                None
            } else {
                Some(title.to_string())
            },
            alt: String::new(),
        }));
    }

    pub(super) fn finish_image(&mut self) -> Result<String, RenderStateMismatch> {
        let image = self.pop_image()?;
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
        Ok(image_html)
    }

    pub(super) fn start_table(&mut self, alignments: Vec<Alignment>) {
        self.contexts.push(BlockContext::Table(TableState {
            in_head: false,
            alignments,
            cell_index: 0,
        }));
    }

    pub(super) fn finish_table(&mut self) -> Result<(), RenderStateMismatch> {
        self.pop_table().map(|_| ())
    }

    pub(super) fn start_table_head(&mut self) -> Result<(), RenderStateMismatch> {
        let table = self.table_mut().ok_or(RenderStateMismatch::ExpectedTable)?;
        table.in_head = true;
        Ok(())
    }

    pub(super) fn finish_table_head(&mut self) -> Result<(), RenderStateMismatch> {
        let table = self.table_mut().ok_or(RenderStateMismatch::ExpectedTable)?;
        table.in_head = false;
        Ok(())
    }

    pub(super) fn reset_table_row(&mut self) -> Result<(), RenderStateMismatch> {
        let table = self.table_mut().ok_or(RenderStateMismatch::ExpectedTable)?;
        table.cell_index = 0;
        Ok(())
    }

    pub(super) fn table_cell_start_tag(&mut self) -> Result<String, RenderStateMismatch> {
        let table = self.table_mut().ok_or(RenderStateMismatch::ExpectedTable)?;
        let align_class = table
            .alignments
            .get(table.cell_index)
            .and_then(table_align_class_attr)
            .unwrap_or("");
        table.cell_index = table.cell_index.saturating_add(1);
        if table.in_head {
            Ok(format!("<th{}>", align_class))
        } else {
            Ok(format!("<td{}>", align_class))
        }
    }

    pub(super) fn table_cell_end_tag(&self) -> Result<&'static str, RenderStateMismatch> {
        let table = self.table().ok_or(RenderStateMismatch::ExpectedTable)?;
        if table.in_head {
            Ok("</th>\n")
        } else {
            Ok("</td>\n")
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

        assert!(matches!(
            state.finish_table(),
            Err(RenderStateMismatch::ExpectedTable)
        ));
        assert!(matches!(
            state.start_table_head(),
            Err(RenderStateMismatch::ExpectedTable)
        ));
        assert!(matches!(
            state.finish_table_head(),
            Err(RenderStateMismatch::ExpectedTable)
        ));
        assert!(matches!(
            state.reset_table_row(),
            Err(RenderStateMismatch::ExpectedTable)
        ));
        assert!(matches!(
            state.table_cell_start_tag(),
            Err(RenderStateMismatch::ExpectedTable)
        ));
        assert!(matches!(
            state.table_cell_end_tag(),
            Err(RenderStateMismatch::ExpectedTable)
        ));
    }

    #[test]
    fn test_finish_headingは上位imageがあるならmismatchを返しstackを保持する() {
        let mut state = RenderState::new();

        state.start_heading(1, 0..1);
        state.start_image("image.png", "");

        assert!(matches!(
            state.finish_heading("heading".to_string(), String::new()),
            Err(RenderStateMismatch::ExpectedHeading)
        ));
        assert!(state.finish_image().is_ok());
        assert!(state
            .finish_heading("heading".to_string(), String::new())
            .is_ok());
    }

    #[test]
    fn test_heading書き込みは上位imageがあるなら下位headingへ副作用を出さない() {
        let mut state = RenderState::new();

        state.start_heading(1, 0..1);
        state.push_heading_escaped_text_html("before", "before");
        state.start_image("image.png", "");

        assert!(!state.in_heading());
        state.push_heading_escaped_text_html("hidden", "hidden");
        state.push_heading_rendered_html_fragment("<em>hidden</em>");

        assert!(state.finish_image().is_ok());
        let heading = state
            .finish_heading("heading".to_string(), String::new())
            .expect("imageを閉じた後はheadingを閉じられる");

        assert!(heading.contains("before"));
        assert!(!heading.contains("hidden"));
    }

    #[test]
    fn test_finish_tableは上位imageがあるならmismatchを返しstackを保持する() {
        let mut state = RenderState::new();

        state.start_table(vec![Alignment::Left]);
        state.start_image("image.png", "");

        assert!(matches!(
            state.finish_table(),
            Err(RenderStateMismatch::ExpectedTable)
        ));
        assert!(state.finish_image().is_ok());
        assert!(state.finish_table().is_ok());
    }

    #[test]
    fn test_finish_code_blockは上位imageがあるならmismatchを返しstackを保持する() {
        let mut state = RenderState::new();
        let syntax_set = SyntaxSet::load_defaults_newlines();

        state.start_code_block(CodeBlockKind::Indented, 0..10);
        state.push_code_text("code");
        state.start_image("image.png", "");

        assert!(matches!(
            state.finish_code_block(&syntax_set, String::new()),
            Err(RenderStateMismatch::ExpectedCodeBlock)
        ));
        assert!(state.finish_image().is_ok());
        assert!(state.finish_code_block(&syntax_set, String::new()).is_ok());
    }
}
