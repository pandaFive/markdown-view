//! Markdown→HTML変換とTOC生成を担当するモジュール。
//!
//! このモジュールとそのサブモジュールは [`SanitizedHtml`] の構築権を持つ
//! 信頼境界を構成する。サブモジュールの追加はセキュリティ影響を伴う。

pub mod toc;

use std::ops::Range;
use std::sync::OnceLock;

use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};
use syntect::highlighting::ThemeSet;
use syntect::html::{css_for_theme_with_class_style, ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// サニタイズ済みHTMLを表すnewtype
///
/// `render_markdown` / `generate_toc` が主たる生成経路。
/// コンストラクタは `pub(in crate::renderer)` とし、
/// `renderer`モジュールツリー内でのみ構築可能にする。
/// 生文字列の混入を型で防止する。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct SanitizedHtml(String);

impl SanitizedHtml {
    /// サニタイズ済みHTML文字列として参照する
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// サニタイズ済みHTMLから構築する（rendererモジュール内部専用）
    ///
    /// 呼び出し側がHTMLのサニタイズを保証する必要がある。
    /// 外部からの生文字列に対して使用してはならない。
    pub(in crate::renderer) fn from_sanitized_html(html: String) -> Self {
        Self(html)
    }
}

struct RenderState {
    html_output: String,
    in_code_block: bool,
    code_block_lang: Option<String>,
    code_block_content: String,
    code_block_range: Option<Range<usize>>,
    image_src: Option<String>,
    image_title: Option<String>,
    image_alt: String,
}

impl RenderState {
    fn new() -> Self {
        Self {
            html_output: String::new(),
            in_code_block: false,
            code_block_lang: None,
            code_block_content: String::new(),
            code_block_range: None,
            image_src: None,
            image_title: None,
            image_alt: String::new(),
        }
    }

    fn push_html(&mut self, html: &str) {
        self.html_output.push_str(html);
    }

    fn start_code_block(&mut self, kind: pulldown_cmark::CodeBlockKind<'_>, range: Range<usize>) {
        self.in_code_block = true;
        self.code_block_range = Some(range);
        self.code_block_lang = match kind {
            pulldown_cmark::CodeBlockKind::Fenced(lang) => {
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

    fn finish_code_block(&mut self, ss: &SyntaxSet, range: Range<usize>, line_lookup: &LineLookup) {
        let line_attrs = self
            .code_block_range
            .as_ref()
            .map(|start_range| Range {
                start: start_range.start,
                end: range.end,
            })
            .map(|full_range| line_block_marker_with(source_line_attrs(line_lookup, &full_range)))
            .unwrap_or_default();
        if let Some(ref lang) = self.code_block_lang {
            let highlighted = ss
                .find_syntax_by_token(lang)
                .or_else(|| ss.find_syntax_by_extension(lang))
                .and_then(|syntax| {
                    let mut generator = ClassedHTMLGenerator::new_with_class_style(
                        syntax,
                        ss,
                        ClassStyle::SpacedPrefixed { prefix: "syn-" },
                    );
                    for line in LinesWithEndings::from(&self.code_block_content) {
                        if let Err(e) = generator.parse_html_for_line_which_includes_newline(line) {
                            tracing::warn!(
                                "[markdown-view] コードハイライトエラー (lang={}): {}",
                                lang,
                                e
                            );
                            return None;
                        }
                    }
                    Some(generator.finalize())
                });

            if let Some(highlighted) = highlighted {
                self.push_html(&format!(
                    "<pre class=\"code-block\"{}><code class=\"syn-code language-{}\">{}</code></pre>\n",
                    line_attrs,
                    html_escape(lang),
                    highlighted
                ));
            } else {
                self.push_html(&format!(
                    "<pre class=\"code-block\"{}><code class=\"syn-code language-{}\">{}</code></pre>\n",
                    line_attrs,
                    html_escape(lang),
                    html_escape(&self.code_block_content)
                ));
            }
        } else {
            self.push_html(&format!(
                "<pre class=\"code-block\"{}><code class=\"syn-code\">{}</code></pre>\n",
                line_attrs,
                html_escape(&self.code_block_content)
            ));
        }

        self.in_code_block = false;
        self.code_block_lang = None;
        self.code_block_content.clear();
        self.code_block_range = None;
    }

    fn start_image(&mut self, dest_url: &str, title: &str) {
        self.image_src = Some(dest_url.to_string());
        self.image_title = if title.is_empty() {
            None
        } else {
            Some(title.to_string())
        };
        self.image_alt.clear();
    }

    fn finish_image(&mut self) -> Option<String> {
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
}

/// Markdownテキストを HTML に変換する
///
/// - GFM拡張（テーブル、タスクリスト、取消線）対応
/// - コードブロックはsyntectでクラスベースハイライト
/// - 見出しにはスラッグIDを付与
/// - raw HTMLは完全に除去される（XSS防止のため出力に含めない）
pub fn render_markdown(input: &str) -> SanitizedHtml {
    if input.is_empty() {
        return SanitizedHtml::from_sanitized_html(String::new());
    }

    let ss = syntax_set();
    let parser = Parser::new_ext(input, markdown_options()).into_offset_iter();
    let line_lookup = LineLookup::new(input);

    let mut state = RenderState::new();
    let mut heading_level: Option<u8> = None;
    let mut heading_range: Option<Range<usize>> = None;
    let mut heading_plain_text = String::new();
    let mut heading_html = String::new();
    let mut in_table_head = false;
    let mut table_alignments: Vec<Alignment> = Vec::new();
    let mut table_cell_index = 0usize;
    let mut id_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for (event, range) in parser {
        let line_attrs = source_line_attrs(&line_lookup, &range);
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                state.start_code_block(kind, range);
            }
            Event::End(TagEnd::CodeBlock) => {
                state.finish_code_block(ss, range, &line_lookup);
            }
            Event::Start(Tag::Heading { level, .. }) => {
                heading_level = Some(level as u8);
                heading_range = Some(range);
                heading_plain_text.clear();
                heading_html.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = heading_level {
                    let slug = slugify(&heading_plain_text);
                    let id = generate_unique_id(&slug, &mut id_counts);
                    let heading_attrs = heading_range
                        .as_ref()
                        .map(|heading_range| {
                            line_block_marker_with(source_line_attrs(&line_lookup, heading_range))
                        })
                        .unwrap_or_default();

                    state.push_html(&format!(
                        "<h{} id=\"{}\"{}>{}</h{}>\n",
                        level,
                        html_escape(&id),
                        heading_attrs,
                        heading_html,
                        level
                    ));
                }
                heading_level = None;
                heading_range = None;
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                state.start_image(&dest_url, &title);
            }
            Event::End(TagEnd::Image) => {
                if let Some(image_html) = state.finish_image() {
                    if heading_level.is_some() {
                        heading_html.push_str(&image_html);
                    } else {
                        state.push_html(&image_html);
                    }
                }
            }
            Event::Text(text) => {
                if state.in_code_block {
                    state.code_block_content.push_str(&text);
                    continue;
                }

                if state.image_src.is_some() {
                    state.image_alt.push_str(&text);
                    continue;
                }

                if heading_level.is_some() {
                    heading_plain_text.push_str(&text);
                    heading_html.push_str(&format!(
                        "<span{}>{}</span>",
                        line_attrs,
                        html_escape(&text)
                    ));
                } else {
                    state.push_html(&format!(
                        "<span{}>{}</span>",
                        line_attrs,
                        html_escape(&text)
                    ));
                }
            }
            Event::Code(text) => {
                if state.image_src.is_some() {
                    state.image_alt.push_str(&text);
                    continue;
                }

                if heading_level.is_some() {
                    heading_plain_text.push_str(&text);
                    heading_html.push_str(&format!(
                        "<code{}>{}</code>",
                        line_attrs,
                        html_escape(&text)
                    ));
                } else {
                    state.push_html(&format!(
                        "<code{}>{}</code>",
                        line_attrs,
                        html_escape(&text)
                    ));
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                // raw HTMLイベントは出力せず破棄する（XSS防止）
            }
            Event::SoftBreak => {
                if state.in_code_block {
                    state.code_block_content.push('\n');
                } else if state.image_src.is_some() {
                    state.image_alt.push(' ');
                } else if heading_level.is_some() {
                    heading_plain_text.push(' ');
                    heading_html.push(' ');
                } else {
                    state.html_output.push('\n');
                }
            }
            Event::HardBreak => {
                if state.in_code_block {
                    state.code_block_content.push('\n');
                } else if state.image_src.is_some() {
                    state.image_alt.push(' ');
                } else if heading_level.is_some() {
                    heading_plain_text.push(' ');
                    heading_html.push_str("<br />");
                } else {
                    state.push_html("<br />\n");
                }
            }
            Event::Rule => {
                state.push_html("<hr />\n");
            }
            Event::Start(Tag::Paragraph) => {
                let attrs = block_line_attrs(&line_lookup, &range);
                state.push_html(&format!("<p{}>", attrs));
            }
            Event::End(TagEnd::Paragraph) => {
                state.push_html("</p>\n");
            }
            Event::Start(Tag::Emphasis) => {
                if state.image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("<em>");
                } else {
                    state.push_html("<em>");
                }
            }
            Event::End(TagEnd::Emphasis) => {
                if state.image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("</em>");
                } else {
                    state.push_html("</em>");
                }
            }
            Event::Start(Tag::Strong) => {
                if state.image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("<strong>");
                } else {
                    state.push_html("<strong>");
                }
            }
            Event::End(TagEnd::Strong) => {
                if state.image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("</strong>");
                } else {
                    state.push_html("</strong>");
                }
            }
            Event::Start(Tag::Strikethrough) => {
                if state.image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("<del>");
                } else {
                    state.push_html("<del>");
                }
            }
            Event::End(TagEnd::Strikethrough) => {
                if state.image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("</del>");
                } else {
                    state.push_html("</del>");
                }
            }
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                if state.image_src.is_some() {
                    continue;
                }

                let safe_dest = sanitize_link_href(&dest_url);
                let mut link_html = format!("<a href=\"{}\"", html_escape(&safe_dest));
                if !title.is_empty() {
                    link_html.push_str(&format!(" title=\"{}\"", html_escape(&title)));
                }
                link_html.push('>');

                if heading_level.is_some() {
                    heading_html.push_str(&link_html);
                } else {
                    state.push_html(&link_html);
                }
            }
            Event::End(TagEnd::Link) => {
                if state.image_src.is_some() {
                    continue;
                }

                if heading_level.is_some() {
                    heading_html.push_str("</a>");
                } else {
                    state.push_html("</a>");
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                let attrs = block_line_attrs(&line_lookup, &range);
                state.push_html(&format!("<blockquote{}>\n", attrs));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                state.push_html("</blockquote>\n");
            }
            Event::Start(Tag::List(Some(start))) => {
                let attrs = block_line_attrs(&line_lookup, &range);
                state.push_html(&format!("<ol start=\"{}\"{}>\n", start, attrs));
            }
            Event::Start(Tag::List(None)) => {
                let attrs = block_line_attrs(&line_lookup, &range);
                state.push_html(&format!("<ul{}>\n", attrs));
            }
            Event::End(TagEnd::List(true)) => {
                state.push_html("</ol>\n");
            }
            Event::End(TagEnd::List(false)) => {
                state.push_html("</ul>\n");
            }
            Event::Start(Tag::Item) => {
                let attrs = block_line_attrs(&line_lookup, &range);
                state.push_html(&format!("<li{}>", attrs));
            }
            Event::End(TagEnd::Item) => {
                state.push_html("</li>\n");
            }
            Event::TaskListMarker(checked) => {
                if checked {
                    state.push_html("<input type=\"checkbox\" checked=\"\" disabled=\"\" /> ");
                } else {
                    state.push_html("<input type=\"checkbox\" disabled=\"\" /> ");
                }
            }
            Event::Start(Tag::Table(alignments)) => {
                let attrs = block_line_attrs(&line_lookup, &range);
                state.push_html(&format!("<table{}>\n", attrs));
                in_table_head = false;
                table_alignments = alignments;
                table_cell_index = 0;
            }
            Event::End(TagEnd::Table) => {
                state.push_html("</table>\n");
                in_table_head = false;
                table_alignments.clear();
                table_cell_index = 0;
            }
            Event::Start(Tag::TableHead) => {
                in_table_head = true;
                state.push_html("<thead>\n");
            }
            Event::End(TagEnd::TableHead) => {
                state.push_html("</thead>\n");
                in_table_head = false;
            }
            Event::Start(Tag::TableRow) => {
                state.push_html("<tr>\n");
                table_cell_index = 0;
            }
            Event::End(TagEnd::TableRow) => {
                state.push_html("</tr>\n");
            }
            Event::Start(Tag::TableCell) => {
                let align_class = table_alignments
                    .get(table_cell_index)
                    .and_then(table_align_class_attr)
                    .unwrap_or("");
                if in_table_head {
                    state.push_html(&format!("<th{}>", align_class));
                } else {
                    state.push_html(&format!("<td{}>", align_class));
                }
                table_cell_index = table_cell_index.saturating_add(1);
            }
            Event::End(TagEnd::TableCell) => {
                if in_table_head {
                    state.push_html("</th>\n");
                } else {
                    state.push_html("</td>\n");
                }
            }
            other => {
                tracing::debug!(
                    "[markdown-view] 未処理のMarkdownイベントを無視: {:?}",
                    other
                );
            }
        }
    }

    SanitizedHtml::from_sanitized_html(state.html_output)
}

struct LineLookup {
    line_starts: Vec<usize>,
}

impl LineLookup {
    fn new(input: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, byte) in input.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { line_starts }
    }

    fn line_for_offset(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(index) => index + 1,
            Err(index) => index,
        }
    }

    fn line_range(&self, range: &Range<usize>) -> (usize, usize) {
        if range.is_empty() {
            let line = self.line_for_offset(range.start);
            return (line, line);
        }
        let start_line = self.line_for_offset(range.start);
        let end_line = self.line_for_offset(range.end.saturating_sub(1));
        (start_line, end_line)
    }
}

fn source_line_attrs(line_lookup: &LineLookup, range: &Range<usize>) -> String {
    let (start_line, end_line) = line_lookup.line_range(range);
    format!(
        " data-source-start-line=\"{}\" data-source-end-line=\"{}\"",
        start_line, end_line
    )
}

/// block-level コンテナ（<p>, <ul>, <ol>, <li>, <table>, <blockquote>）向けの行範囲属性。
///
/// 設計意図: 新規attribute `data-line-block-start/end` のみを付与し、既存 `data-source-*` は
/// 付与しない。理由は `getSelectionLineRange()` (memo.js) が `[data-source-start-line]` で
/// 集計しており、コンテナにも `data-source-*` を付けると、中の `<li>` 単体を選択しても
/// 祖先 `<ul>` の範囲まで拾って引用 `Lx-Ly` が広がる回帰を起こすため。
/// heading / code-block は元から `data-source-*` を持つ（その要素の範囲を示すのが正しい）のでそちらは維持。
fn block_line_attrs(line_lookup: &LineLookup, range: &Range<usize>) -> String {
    let (start_line, end_line) = line_lookup.line_range(range);
    format!(
        " data-line-block data-line-block-start=\"{}\" data-line-block-end=\"{}\"",
        start_line, end_line
    )
}

/// heading / code-block 用: 既存の `source_line_attrs` に `data-line-block` マーカーを前置。
/// これらの要素は元から `data-source-*` を持ち、quote 機能上もその範囲が「その要素の範囲」として正しい。
fn line_block_marker_with(source_attrs: String) -> String {
    format!(" data-line-block{}", source_attrs)
}

fn table_align_class_attr(alignment: &Alignment) -> Option<&'static str> {
    match alignment {
        Alignment::Left => Some(" class=\"align-left\""),
        Alignment::Center => Some(" class=\"align-center\""),
        Alignment::Right => Some(" class=\"align-right\""),
        Alignment::None => None,
    }
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

/// テーマ名が有効か検証する（起動時のfail-fast用）
///
/// 無効な場合は利用可能なテーマ名の一覧を返す。
pub fn validate_theme(name: &str) -> Result<(), Vec<String>> {
    let ts = theme_set();
    if ts.themes.contains_key(name) {
        Ok(())
    } else {
        Err(ts.themes.keys().cloned().collect())
    }
}

/// syntectテーマからクラスベースのCSSを生成する
///
/// テーマが見つからない場合やCSS生成に失敗した場合は空文字列を返す。
/// 空文字列の場合、構文ハイライトは無効化される。
pub fn syntax_theme_css(theme_name: Option<&str>) -> String {
    let ts = theme_set();
    let Some(theme) = resolve_theme(ts, theme_name) else {
        tracing::warn!("[markdown-view] テーマが見つからないため構文ハイライトCSSを生成できません");
        return highlight_disabled_notice_css();
    };

    match css_for_theme_with_class_style(theme, ClassStyle::SpacedPrefixed { prefix: "syn-" }) {
        Ok(css) => css,
        Err(e) => {
            tracing::warn!(
                "[markdown-view] 構文ハイライトCSS生成に失敗したため無効化します: {}",
                e
            );
            highlight_disabled_notice_css()
        }
    }
}

fn highlight_disabled_notice_css() -> String {
    r#"
/* 構文ハイライト無効化時のユーザー通知 */
body::before {
  content: '構文ハイライトを無効化しました（テーマ読み込み失敗）';
  position: fixed;
  top: 0;
  right: 0;
  z-index: 1100;
  background: #fff4ce;
  color: #5c4500;
  border: 1px solid #d9b84f;
  border-radius: 0 0 0 6px;
  padding: 0.35rem 0.55rem;
  font-size: 0.75rem;
  font-family: sans-serif;
}
"#
    .to_string()
}

fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options
}

/// Markdownから抽出した見出し情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingInfo {
    pub level: u8,
    pub text: String,
    pub id: String,
}

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

/// Markdownから見出し情報を抽出する
///
/// 画像altは見出しテキストから除外し、`render_markdown`と同じID生成ルールを適用する。
pub fn extract_headings(input: &str) -> Vec<HeadingInfo> {
    let parser = Parser::new_ext(input, markdown_options());
    let mut headings = Vec::new();
    let mut current_level: Option<u8> = None;
    let mut current_text = String::new();
    let mut in_heading_image = false;
    let mut id_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current_level = Some(level as u8);
                current_text.clear();
                in_heading_image = false;
            }
            Event::Start(Tag::Image { .. }) if current_level.is_some() => {
                in_heading_image = true;
            }
            Event::End(TagEnd::Image) if current_level.is_some() => {
                in_heading_image = false;
            }
            Event::Text(text) if current_level.is_some() => {
                if !in_heading_image {
                    current_text.push_str(&text);
                }
            }
            Event::Code(text) if current_level.is_some() => {
                if !in_heading_image {
                    current_text.push_str(&text);
                }
            }
            Event::SoftBreak if current_level.is_some() => {
                if !in_heading_image {
                    current_text.push(' ');
                }
            }
            Event::HardBreak if current_level.is_some() => {
                if !in_heading_image {
                    current_text.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = current_level {
                    let slug = slugify(&current_text);
                    let id = generate_unique_id(&slug, &mut id_counts);
                    headings.push(HeadingInfo {
                        level,
                        text: current_text.clone(),
                        id,
                    });
                }
                current_level = None;
                in_heading_image = false;
            }
            other => {
                tracing::debug!(
                    "[markdown-view] 見出し抽出で未処理イベントを無視: {:?}",
                    other
                );
            }
        }
    }

    headings
}

/// 見出しテキストをスラッグ（URL-safe ID）に変換する
pub fn slugify(text: &str) -> String {
    let slug = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        "section".to_string()
    } else {
        slug
    }
}

/// スラッグから一意なIDを生成する（重複時は連番を付与）
pub fn generate_unique_id(
    slug: &str,
    id_counts: &mut std::collections::HashMap<String, usize>,
) -> String {
    let count = id_counts.entry(slug.to_string()).or_insert(0);
    let id = if *count == 0 {
        slug.to_string()
    } else {
        format!("{}-{}", slug, count)
    };
    *count += 1;
    id
}

fn resolve_theme<'a>(
    theme_set: &'a ThemeSet,
    theme_name: Option<&str>,
) -> Option<&'a syntect::highlighting::Theme> {
    const DEFAULT_THEME: &str = "base16-ocean.dark";

    if let Some(name) = theme_name {
        if let Some(theme) = theme_set.themes.get(name) {
            return Some(theme);
        }
        let available: Vec<&str> = theme_set.themes.keys().map(|s| s.as_str()).collect();
        tracing::warn!(
            "[markdown-view] 警告: テーマ '{}' が見つかりません。デフォルトテーマを使用します。利用可能: {:?}",
            name,
            available
        );
    }

    if let Some(theme) = theme_set.themes.get(DEFAULT_THEME) {
        return Some(theme);
    }

    let fallback = theme_set.themes.iter().next();
    if let Some((name, _)) = &fallback {
        tracing::warn!(
            "[markdown-view] 警告: デフォルトテーマ '{}' が見つかりません。'{}' を使用します",
            DEFAULT_THEME,
            name
        );
    }
    fallback.map(|(_, theme)| theme)
}

/// リンクURLを安全な形式に正規化する
///
/// 前後の空白を除去し、ローカル参照・相対パス・許可スキーム(http/https/mailto/tel)
/// 以外は `"#"` に置き換える。
fn sanitize_link_href(dest_url: &str) -> String {
    sanitize_url(dest_url, UrlPolicy::Link)
}

/// 画像URLを安全な形式に正規化する
///
/// ローカル参照以外は `"#"` に置き換える。
fn sanitize_image_src(dest_url: &str) -> String {
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

/// URLが許可ポリシーに一致するか判定する
///
/// `UrlPolicy::Link` は `http/https/mailto/tel` とローカル参照を許可する。
/// `UrlPolicy::Image` はローカル参照のみ許可する。
fn is_safe_href(dest_url: &str, policy: UrlPolicy) -> bool {
    if dest_url.is_empty() {
        return false;
    }

    // `//example.com` のようなプロトコル相対URLは拒否する。
    // 現在ページのスキームを継承して外部サイトへ遷移できるため、
    // ローカル参照のみ許可するポリシーを迂回する余地を作らない。
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::UpdateMessage;

    #[test]
    fn test_sanitized_html_serde_transparentで文字列として直列化される() {
        let html = SanitizedHtml::from_sanitized_html("<p>x</p>".to_string());
        let value = serde_json::to_value(&html).unwrap();
        assert_eq!(value, serde_json::Value::String("<p>x</p>".to_string()));
    }

    #[test]
    fn test_sanitized_htmlがupdate_message内でも文字列として直列化される() {
        let update = UpdateMessage::new(
            SanitizedHtml::from_sanitized_html("<p>content</p>".to_string()),
            SanitizedHtml::from_sanitized_html("<ul><li>toc</li></ul>".to_string()),
            None,
        );
        let value = serde_json::to_value(update).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "content": "<p>content</p>",
                "toc": "<ul><li>toc</li></ul>"
            })
        );
    }

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
