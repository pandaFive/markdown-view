use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::renderer::slugify;

/// 見出し情報
struct Heading {
    level: u8,
    text: String,
    id: String,
}

/// MarkdownテキストからTOC（目次）HTMLを生成する
///
/// 見出しがない場合は空文字列を返す
pub fn generate_toc(input: &str) -> String {
    let headings = extract_headings(input);
    if headings.is_empty() {
        return String::new();
    }

    build_toc_html(&headings)
}

/// Markdownから見出し情報を抽出する
fn extract_headings(input: &str) -> Vec<Heading> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(input, options);

    let mut headings = Vec::new();
    let mut current_level: Option<u8> = None;
    let mut current_text = String::new();
    let mut id_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current_level = Some(level as u8);
                current_text.clear();
            }
            Event::Text(text) if current_level.is_some() => {
                current_text.push_str(&text);
            }
            Event::Code(text) if current_level.is_some() => {
                current_text.push_str(&text);
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = current_level {
                    let slug = slugify(&current_text);
                    let count = id_counts.entry(slug.clone()).or_insert(0);
                    let id = if *count == 0 {
                        slug.clone()
                    } else {
                        format!("{}-{}", slug, count)
                    };
                    *count += 1;

                    headings.push(Heading {
                        level,
                        text: current_text.clone(),
                        id,
                    });
                }
                current_level = None;
            }
            _ => {}
        }
    }

    headings
}

/// 見出し情報からネストされたTOC HTMLを構築する
fn build_toc_html(headings: &[Heading]) -> String {
    let mut html = String::new();
    let mut stack: Vec<u8> = Vec::new(); // 現在開いているulのレベル

    for heading in headings {
        let level = heading.level;

        // 現在のネストレベルを調整
        while let Some(&top) = stack.last() {
            if top >= level {
                html.push_str("</ul>\n");
                stack.pop();
            } else {
                break;
            }
        }

        // 必要なulを開く
        while stack.last().copied().unwrap_or(0) < level {
            html.push_str("<ul>\n");
            stack.push(stack.last().copied().unwrap_or(0) + 1);
        }

        html.push_str(&format!(
            "<li><a href=\"#{}\">{}</a></li>\n",
            heading.id, heading.text
        ));
    }

    // 残りのulを閉じる
    while stack.pop().is_some() {
        html.push_str("</ul>\n");
    }

    html
}
