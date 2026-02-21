use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use syntect::parsing::SyntaxSet;

/// Markdownテキストを HTML に変換する
///
/// - GFM拡張（テーブル、タスクリスト、取消線）対応
/// - コードブロックはsyntectでテーマ付きハイライト
/// - 見出しにはスラッグIDを付与
/// - raw HTMLは無効化（XSS防止）
pub fn render_markdown(input: &str, theme_name: Option<&str>) -> String {
    if input.is_empty() {
        return String::new();
    }

    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = resolve_theme(&ts, theme_name);
    if theme.is_none() {
        eprintln!("[markdown-view] テーマが見つかりません。ハイライトなしで出力します");
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(input, options);

    let mut html_output = String::new();
    let mut in_code_block = false;
    let mut code_block_lang: Option<String> = None;
    let mut code_block_content = String::new();
    let mut heading_level: Option<u8> = None;
    let mut heading_plain_text = String::new();
    let mut heading_html = String::new();
    let mut image_src: Option<String> = None;
    let mut image_title: Option<String> = None;
    let mut image_alt = String::new();
    let mut in_table_head = false;
    let mut id_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                // コードブロック開始: 言語名を取得
                in_code_block = true;
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
                // コードブロック終了: syntectでハイライト（テーマが利用可能な場合のみ）
                if let Some(ref lang) = code_block_lang {
                    let highlighted = theme.and_then(|t| {
                        ss.find_syntax_by_token(lang)
                            .or_else(|| ss.find_syntax_by_extension(lang))
                            .and_then(|syntax| {
                                match highlighted_html_for_string(
                                    &code_block_content,
                                    &ss,
                                    syntax,
                                    t,
                                ) {
                                    Ok(html) => Some(html),
                                    Err(e) => {
                                        eprintln!(
                                            "[markdown-view] コードハイライトエラー (lang={}): {}",
                                            lang, e
                                        );
                                        None
                                    }
                                }
                            })
                    });

                    if let Some(highlighted) = highlighted {
                        html_output.push_str(&add_code_block_class(highlighted));
                    } else {
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
                in_code_block = false;
                code_block_lang = None;
                code_block_content.clear();
            }
            Event::Start(Tag::Heading { level, .. }) => {
                heading_level = Some(level as u8);
                heading_plain_text.clear();
                heading_html.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = heading_level {
                    let slug = slugify(&heading_plain_text);
                    let id = generate_unique_id(&slug, &mut id_counts);

                    html_output.push_str(&format!(
                        "<h{} id=\"{}\">{}</h{}>\n",
                        level, id, heading_html, level
                    ));
                }
                heading_level = None;
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                image_src = Some(dest_url.to_string());
                image_title = if title.is_empty() {
                    None
                } else {
                    Some(title.to_string())
                };
                image_alt.clear();
            }
            Event::End(TagEnd::Image) => {
                if let Some(src) = image_src.take() {
                    let safe_src = sanitize_href(&src);
                    let mut image_html = format!(
                        "<img src=\"{}\" alt=\"{}\"",
                        html_escape(&safe_src),
                        image_alt
                    );
                    if let Some(title) = image_title.take() {
                        image_html.push_str(&format!(" title=\"{}\"", html_escape(&title)));
                    }
                    image_html.push_str(" />");

                    if heading_level.is_some() {
                        heading_html.push_str(&image_html);
                    } else {
                        html_output.push_str(&image_html);
                    }
                }
                image_title = None;
                image_alt.clear();
            }
            Event::Text(text) => {
                if in_code_block {
                    code_block_content.push_str(&text);
                    continue;
                }

                if image_src.is_some() {
                    image_alt.push_str(&html_escape(&text));
                    continue;
                }

                if heading_level.is_some() {
                    heading_plain_text.push_str(&text);
                    heading_html.push_str(&html_escape(&text));
                } else {
                    html_output.push_str(&html_escape(&text));
                }
            }
            Event::Code(text) => {
                if image_src.is_some() {
                    image_alt.push_str(&html_escape(&text));
                    continue;
                }

                if heading_level.is_some() {
                    heading_plain_text.push_str(&text);
                    heading_html.push_str(&format!("<code>{}</code>", html_escape(&text)));
                } else {
                    html_output.push_str(&format!("<code>{}</code>", html_escape(&text)));
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                // raw HTMLは無効化（XSS防止）
            }
            Event::SoftBreak => {
                if in_code_block {
                    code_block_content.push('\n');
                } else if image_src.is_some() {
                    image_alt.push(' ');
                } else if heading_level.is_some() {
                    heading_plain_text.push(' ');
                    heading_html.push(' ');
                } else {
                    html_output.push('\n');
                }
            }
            Event::HardBreak => {
                if in_code_block {
                    code_block_content.push('\n');
                } else if image_src.is_some() {
                    image_alt.push(' ');
                } else if heading_level.is_some() {
                    heading_plain_text.push(' ');
                    heading_html.push_str("<br />");
                } else {
                    html_output.push_str("<br />\n");
                }
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
                if image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("<em>");
                } else {
                    html_output.push_str("<em>");
                }
            }
            Event::End(TagEnd::Emphasis) => {
                if image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("</em>");
                } else {
                    html_output.push_str("</em>");
                }
            }
            Event::Start(Tag::Strong) => {
                if image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("<strong>");
                } else {
                    html_output.push_str("<strong>");
                }
            }
            Event::End(TagEnd::Strong) => {
                if image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("</strong>");
                } else {
                    html_output.push_str("</strong>");
                }
            }
            Event::Start(Tag::Strikethrough) => {
                if image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("<del>");
                } else {
                    html_output.push_str("<del>");
                }
            }
            Event::End(TagEnd::Strikethrough) => {
                if image_src.is_some() {
                    continue;
                }
                if heading_level.is_some() {
                    heading_html.push_str("</del>");
                } else {
                    html_output.push_str("</del>");
                }
            }
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                if image_src.is_some() {
                    continue;
                }

                let safe_dest = sanitize_href(&dest_url);
                let mut link_html = format!("<a href=\"{}\"", html_escape(&safe_dest));
                if !title.is_empty() {
                    link_html.push_str(&format!(" title=\"{}\"", html_escape(&title)));
                }
                link_html.push('>');

                if heading_level.is_some() {
                    heading_html.push_str(&link_html);
                } else {
                    html_output.push_str(&link_html);
                }
            }
            Event::End(TagEnd::Link) => {
                if image_src.is_some() {
                    continue;
                }

                if heading_level.is_some() {
                    heading_html.push_str("</a>");
                } else {
                    html_output.push_str("</a>");
                }
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
                in_table_head = false;
                // alignmentsは現在未使用（将来のセル揃え対応で使用予定）
                let _ = alignments;
            }
            Event::End(TagEnd::Table) => {
                html_output.push_str("</table>\n");
                in_table_head = false;
            }
            Event::Start(Tag::TableHead) => {
                in_table_head = true;
                html_output.push_str("<thead>\n");
            }
            Event::End(TagEnd::TableHead) => {
                html_output.push_str("</thead>\n");
                in_table_head = false;
            }
            Event::Start(Tag::TableRow) => {
                html_output.push_str("<tr>\n");
            }
            Event::End(TagEnd::TableRow) => {
                html_output.push_str("</tr>\n");
            }
            Event::Start(Tag::TableCell) => {
                if in_table_head {
                    html_output.push_str("<th>");
                } else {
                    html_output.push_str("<td>");
                }
            }
            Event::End(TagEnd::TableCell) => {
                if in_table_head {
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

    theme_name
        .and_then(|name| theme_set.themes.get(name))
        .or_else(|| theme_set.themes.get(DEFAULT_THEME))
        .or_else(|| theme_set.themes.values().next())
}

fn add_code_block_class(highlighted_html: String) -> String {
    if highlighted_html.starts_with("<pre ") {
        highlighted_html.replacen("<pre ", "<pre class=\"code-block\" ", 1)
    } else if highlighted_html.starts_with("<pre>") {
        highlighted_html.replacen("<pre>", "<pre class=\"code-block\">", 1)
    } else {
        highlighted_html
    }
}

fn sanitize_href(dest_url: &str) -> String {
    let trimmed = dest_url.trim();
    if is_safe_href(trimmed) {
        trimmed.to_string()
    } else {
        "#".to_string()
    }
}

fn is_safe_href(dest_url: &str) -> bool {
    if dest_url.is_empty() {
        return false;
    }

    // プロトコル相対URL（//example.com/...）はリダイレクト先を制御可能なため拒否
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

    let scheme = dest_url[..colon_pos].to_ascii_lowercase();
    matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel")
}

/// HTML特殊文字のエスケープ（属性値にも安全）
pub fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
