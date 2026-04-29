use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use super::security::html_escape;

/// `line_attrs` は `line` helper が生成した属性文字列だけを渡す内部API。
pub(super) fn render_code_block_html(
    syntax_set: &SyntaxSet,
    language: Option<&str>,
    code: &str,
    line_attrs: &str,
) -> String {
    if let Some(lang) = language {
        if let Some(highlighted) = highlighted_code_html(syntax_set, lang, code) {
            return format!(
                "<pre class=\"code-block\"{}><code class=\"syn-code language-{}\">{}</code></pre>\n",
                line_attrs,
                html_escape(lang),
                highlighted
            );
        }

        return plain_code_block_html(Some(lang), code, line_attrs);
    }

    plain_code_block_html(None, code, line_attrs)
}

fn plain_code_block_html(language: Option<&str>, code: &str, line_attrs: &str) -> String {
    if let Some(lang) = language {
        return format!(
            "<pre class=\"code-block\"{}><code class=\"syn-code language-{}\">{}</code></pre>\n",
            line_attrs,
            html_escape(lang),
            html_escape(code)
        );
    }

    format!(
        "<pre class=\"code-block\"{}><code class=\"syn-code\">{}</code></pre>\n",
        line_attrs,
        html_escape(code)
    )
}

fn highlighted_code_html(syntax_set: &SyntaxSet, language: &str, code: &str) -> Option<String> {
    let syntax = syntax_set
        .find_syntax_by_token(language)
        .or_else(|| syntax_set.find_syntax_by_extension(language))?;
    let mut generator = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        syntax_set,
        ClassStyle::SpacedPrefixed { prefix: "syn-" },
    );

    for line in LinesWithEndings::from(code) {
        if let Err(e) = generator.parse_html_for_line_which_includes_newline(line) {
            tracing::warn!(
                "[markdown-view] コードハイライトエラー (lang={}): {}",
                language,
                e
            );
            return None;
        }
    }

    Some(generator.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_code_block_htmlは言語と本文をescapeする() {
        let html = plain_code_block_html(Some("bad\"lang"), "<x>&", " data-line-block");

        assert_eq!(
            html,
            "<pre class=\"code-block\" data-line-block><code class=\"syn-code language-bad&quot;lang\">&lt;x&gt;&amp;</code></pre>\n"
        );
    }

    #[test]
    fn test_plain_code_block_htmlは言語なしでも本文をescapeする() {
        let html = plain_code_block_html(None, "<x>&", "");

        assert_eq!(
            html,
            "<pre class=\"code-block\"><code class=\"syn-code\">&lt;x&gt;&amp;</code></pre>\n"
        );
    }
}
