use super::{html_escape, render_document, HeadingInfo, SanitizedHtml};

/// MarkdownテキストからTOC（目次）HTMLを生成する
///
/// 見出しがない場合は空文字列を返す
pub fn generate_toc(input: &str) -> SanitizedHtml {
    render_document(input).toc
}

pub(in crate::renderer) fn generate_toc_from_headings(headings: &[HeadingInfo]) -> SanitizedHtml {
    if headings.is_empty() {
        return SanitizedHtml::from_sanitized_html(String::new());
    }

    SanitizedHtml::from_sanitized_html(build_toc_html(headings))
}

/// 見出し情報からネストされたTOC HTMLを構築する
fn build_toc_html(headings: &[HeadingInfo]) -> String {
    let mut html = String::new();
    let mut current_level: u8 = 0;
    let mut open_li_at_level: Vec<bool> = Vec::new();

    for heading in headings {
        // 1 <= level <= current_level + 1 に正規化し、急な深化（h1→h4）はh1→h2として扱う
        // HeadingInfo.levelはu8なので、内部境界として0が渡ってもh1相当に正規化する
        // これにより<ul>の直接ネストと、後段のcurrent_level - 1によるunderflow/OOBを防ぐ
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

#[cfg(test)]
mod tests {
    use super::{build_toc_html, HeadingInfo};

    #[test]
    fn test_build_toc_htmlはlevel0をh1相当に正規化する() {
        let headings = vec![HeadingInfo {
            level: 0,
            text: "Zero <Level>".to_string(),
            id: r#"zero"level"#.to_string(),
        }];

        let html = build_toc_html(&headings);

        assert_eq!(
            html,
            "<ul>\n<li><a href=\"#zero&quot;level\">Zero &lt;Level&gt;</a></li>\n</ul>\n"
        );
    }

    #[test]
    fn test_build_toc_htmlは連続level0を同階層として閉じる() {
        let headings = vec![
            HeadingInfo {
                level: 0,
                text: "Zero One".to_string(),
                id: "zero-one".to_string(),
            },
            HeadingInfo {
                level: 0,
                text: "Zero Two".to_string(),
                id: "zero-two".to_string(),
            },
        ];

        let html = build_toc_html(&headings);

        assert_eq!(
            html,
            "<ul>\n<li><a href=\"#zero-one\">Zero One</a></li>\n<li><a href=\"#zero-two\">Zero Two</a></li>\n</ul>\n"
        );
    }

    #[test]
    fn test_build_toc_htmlはネスト後のlevel0をh1相当へ戻す() {
        let headings = vec![
            HeadingInfo {
                level: 1,
                text: "Parent".to_string(),
                id: "parent".to_string(),
            },
            HeadingInfo {
                level: 2,
                text: "Child".to_string(),
                id: "child".to_string(),
            },
            HeadingInfo {
                level: 0,
                text: "Zero".to_string(),
                id: "zero".to_string(),
            },
        ];

        let html = build_toc_html(&headings);

        assert_eq!(
            html,
            "<ul>\n<li><a href=\"#parent\">Parent</a><ul>\n<li><a href=\"#child\">Child</a></li>\n</ul>\n</li>\n<li><a href=\"#zero\">Zero</a></li>\n</ul>\n"
        );
    }
}
