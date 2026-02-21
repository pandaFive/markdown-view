use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::renderer::{generate_unique_id, html_escape, slugify};

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

                    headings.push(Heading {
                        level,
                        text: current_text.clone(),
                        id,
                    });
                }
                current_level = None;
                in_heading_image = false;
            }
            _ => {}
        }
    }

    headings
}

/// 見出し情報からネストされたTOC HTMLを構築する
fn build_toc_html(headings: &[Heading]) -> String {
    let mut html = String::new();
    let mut current_level: u8 = 0;
    let mut open_li_at_level: Vec<bool> = Vec::new();

    for heading in headings {
        // 見出しレベルの急な深化を防止（h1→h4のような場合、h1→h2として扱う）
        // これにより<ul>の直接ネスト（<ul><ul>）を回避する
        let level = heading.level.min(current_level.saturating_add(1)).max(1);

        // 深い階層から戻る場合は、開いているli/ulを閉じる
        while current_level > level {
            if open_li_at_level.pop().unwrap_or(false) {
                html.push_str("</li>\n");
            }
            html.push_str("</ul>\n");
            current_level -= 1;
        }

        // 深い階層へ進む場合はulを開く（親liの内側にネストされる）
        while current_level < level {
            html.push_str("<ul>\n");
            current_level += 1;
            open_li_at_level.push(false);
        }

        // 同階層で次の項目へ進む前に前のliを閉じる
        if current_level > 0 && open_li_at_level[(current_level - 1) as usize] {
            html.push_str("</li>\n");
            open_li_at_level[(current_level - 1) as usize] = false;
        }

        html.push_str(&format!(
            "<li><a href=\"#{}\">{}</a>",
            heading.id,
            html_escape(&heading.text)
        ));
        open_li_at_level[(current_level - 1) as usize] = true;
    }

    // 残りのli/ulを閉じる
    while current_level > 0 {
        if open_li_at_level.pop().unwrap_or(false) {
            html.push_str("</li>\n");
        }
        html.push_str("</ul>\n");
        current_level -= 1;
    }

    html
}
