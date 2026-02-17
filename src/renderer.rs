use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use syntect::highlighting::ThemeSet;
use syntect::html::ClassedHTMLGenerator;
use syntect::parsing::SyntaxSet;

/// Markdownテキストを HTML に変換する
///
/// - GFM拡張（テーブル、タスクリスト、取消線）対応
/// - コードブロックはsyntectでclass-basedハイライト
/// - 見出しにはスラッグIDを付与
/// - raw HTMLは無効化（XSS防止）
pub fn render_markdown(input: &str, theme_name: Option<&str>) -> String {
    if input.is_empty() {
        return String::new();
    }

    let ss = SyntaxSet::load_defaults_newlines();
    let _ts = ThemeSet::load_defaults();
    let _theme_name = theme_name.unwrap_or("base16-ocean.dark");

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(input, options);

    let mut html_output = String::new();
    let mut code_block_lang: Option<String> = None;
    let mut code_block_content = String::new();
    let mut heading_level: Option<u8> = None;
    let mut heading_text = String::new();
    let mut id_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                // コードブロック開始: 言語名を取得
                code_block_lang = match kind {
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
                code_block_content.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                // コードブロック終了: syntectでハイライト
                if let Some(ref lang) = code_block_lang {
                    if let Some(syntax) = ss
                        .find_syntax_by_token(lang)
                        .or_else(|| ss.find_syntax_by_extension(lang))
                    {
                        let mut generator = ClassedHTMLGenerator::new_with_class_style(
                            syntax,
                            &ss,
                            syntect::html::ClassStyle::Spaced,
                        );
                        for line in syntect::util::LinesWithEndings::from(&code_block_content) {
                            let _ = generator.parse_html_for_line_which_includes_newline(line);
                        }
                        let highlighted = generator.finalize();
                        html_output.push_str(&format!(
                            "<pre class=\"code-block\"><code class=\"language-{}\">{}</code></pre>\n",
                            html_escape(lang),
                            highlighted
                        ));
                    } else {
                        // 言語が見つからない場合はプレーンテキスト
                        html_output.push_str(&format!(
                            "<pre class=\"code-block\"><code class=\"language-{}\">{}</code></pre>\n",
                            html_escape(lang),
                            html_escape(&code_block_content)
                        ));
                    }
                } else {
                    // 言語指定なし
                    html_output.push_str(&format!(
                        "<pre class=\"code-block\"><code>{}</code></pre>\n",
                        html_escape(&code_block_content)
                    ));
                }
                code_block_lang = None;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                heading_level = Some(level as u8);
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = heading_level {
                    let slug = slugify(&heading_text);
                    let count = id_counts.entry(slug.clone()).or_insert(0);
                    let id = if *count == 0 {
                        slug.clone()
                    } else {
                        format!("{}-{}", slug, count)
                    };
                    *count += 1;

                    html_output.push_str(&format!(
                        "<h{} id=\"{}\">{}</h{}>\n",
                        level, id, heading_text, level
                    ));
                }
                heading_level = None;
            }
            Event::Text(text) => {
                if code_block_lang.is_some()
                    || !code_block_content.is_empty() && heading_level.is_none()
                {
                    // コードブロック内
                    if heading_level.is_none() {
                        code_block_content.push_str(&text);
                        continue;
                    }
                }
                if heading_level.is_some() {
                    heading_text.push_str(&text);
                } else {
                    html_output.push_str(&text);
                }
            }
            Event::Code(text) => {
                if heading_level.is_some() {
                    heading_text.push_str(&format!("<code>{}</code>", html_escape(&text)));
                } else {
                    html_output.push_str(&format!("<code>{}</code>", html_escape(&text)));
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                // raw HTMLは無効化（XSS防止）
            }
            Event::SoftBreak => {
                html_output.push('\n');
            }
            Event::HardBreak => {
                html_output.push_str("<br />\n");
            }
            Event::Rule => {
                html_output.push_str("<hr />\n");
            }
            Event::Start(Tag::Paragraph) => {
                html_output.push_str("<p>");
            }
            Event::End(TagEnd::Paragraph) => {
                html_output.push_str("</p>\n");
            }
            Event::Start(Tag::Emphasis) => {
                html_output.push_str("<em>");
            }
            Event::End(TagEnd::Emphasis) => {
                html_output.push_str("</em>");
            }
            Event::Start(Tag::Strong) => {
                html_output.push_str("<strong>");
            }
            Event::End(TagEnd::Strong) => {
                html_output.push_str("</strong>");
            }
            Event::Start(Tag::Strikethrough) => {
                html_output.push_str("<del>");
            }
            Event::End(TagEnd::Strikethrough) => {
                html_output.push_str("</del>");
            }
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                html_output.push_str(&format!("<a href=\"{}\"", html_escape(&dest_url)));
                if !title.is_empty() {
                    html_output.push_str(&format!(" title=\"{}\"", html_escape(&title)));
                }
                html_output.push('>');
            }
            Event::End(TagEnd::Link) => {
                html_output.push_str("</a>");
            }
            Event::Start(Tag::Image {
                dest_url, title: _, ..
            }) => {
                html_output.push_str(&format!("<img src=\"{}\" alt=\"", html_escape(&dest_url)));
                // altテキストは子テキストイベントで収集される
                // ここでは開始タグだけ出力し、テキストイベントでaltに追加
            }
            Event::End(TagEnd::Image) => {
                html_output.push_str("\" />");
            }
            Event::Start(Tag::BlockQuote(_)) => {
                html_output.push_str("<blockquote>\n");
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                html_output.push_str("</blockquote>\n");
            }
            Event::Start(Tag::List(Some(start))) => {
                html_output.push_str(&format!("<ol start=\"{}\">\n", start));
            }
            Event::Start(Tag::List(None)) => {
                html_output.push_str("<ul>\n");
            }
            Event::End(TagEnd::List(true)) => {
                html_output.push_str("</ol>\n");
            }
            Event::End(TagEnd::List(false)) => {
                html_output.push_str("</ul>\n");
            }
            Event::Start(Tag::Item) => {
                html_output.push_str("<li>");
            }
            Event::End(TagEnd::Item) => {
                html_output.push_str("</li>\n");
            }
            Event::TaskListMarker(checked) => {
                if checked {
                    html_output.push_str("<input type=\"checkbox\" checked=\"\" disabled=\"\" /> ");
                } else {
                    html_output.push_str("<input type=\"checkbox\" disabled=\"\" /> ");
                }
            }
            Event::Start(Tag::Table(alignments)) => {
                html_output.push_str("<table>\n");
                // alignmentsは後で使用
                let _ = alignments;
            }
            Event::End(TagEnd::Table) => {
                html_output.push_str("</table>\n");
            }
            Event::Start(Tag::TableHead) => {
                html_output.push_str("<thead><tr>\n");
            }
            Event::End(TagEnd::TableHead) => {
                html_output.push_str("</tr></thead>\n");
            }
            Event::Start(Tag::TableRow) => {
                html_output.push_str("<tr>\n");
            }
            Event::End(TagEnd::TableRow) => {
                html_output.push_str("</tr>\n");
            }
            Event::Start(Tag::TableCell) => {
                // thead内ならth、tbody内ならtd
                // 簡易判定: 直近のタグで判断
                if html_output.ends_with("<thead><tr>\n")
                    || html_output.contains("<thead><tr>\n") && !html_output.contains("</thead>")
                {
                    html_output.push_str("<th>");
                } else {
                    html_output.push_str("<td>");
                }
            }
            Event::End(TagEnd::TableCell) => {
                // 対応する閉じタグ
                if !html_output.contains("</thead>") {
                    html_output.push_str("</th>\n");
                } else {
                    html_output.push_str("</td>\n");
                }
            }
            _ => {}
        }
    }

    html_output
}

/// 見出しテキストをスラッグ（URL-safe ID）に変換する
pub fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// HTML特殊文字のエスケープ
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
