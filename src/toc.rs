use crate::renderer::{extract_headings, html_escape, HeadingInfo, SanitizedHtml};

/// MarkdownテキストからTOC（目次）HTMLを生成する
///
/// 見出しがない場合は空文字列を返す
pub fn generate_toc(input: &str) -> SanitizedHtml {
    let headings = extract_headings(input);
    if headings.is_empty() {
        return SanitizedHtml::from_sanitized_html(String::new());
    }

    SanitizedHtml::from_sanitized_html(build_toc_html(&headings))
}

/// 見出し情報からネストされたTOC HTMLを構築する
fn build_toc_html(headings: &[HeadingInfo]) -> String {
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
            html_escape(&heading.id),
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
