use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

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

const MAX_TRACKED_UNKNOWN_LANGUAGES: usize = 256;
const MAX_LOGGED_LANGUAGE_BYTES: usize = 128;

static UNKNOWN_LANGUAGE_LOG_TRACKER: OnceLock<UnknownLanguageLogTracker> = OnceLock::new();

struct UnknownLanguageLogTracker {
    seen: Mutex<HashSet<u64>>,
}

impl UnknownLanguageLogTracker {
    fn new() -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
        }
    }

    fn mark_seen(&self, language: &str) -> bool {
        let fingerprint = language_fingerprint(language);
        let mut seen = self
            .seen
            .lock()
            .expect("未知言語ログの重複抑制状態をロックできること");

        if seen.contains(&fingerprint) {
            return false;
        }

        if seen.len() >= MAX_TRACKED_UNKNOWN_LANGUAGES {
            return false;
        }

        seen.insert(fingerprint)
    }
}

fn unknown_language_log_tracker() -> &'static UnknownLanguageLogTracker {
    UNKNOWN_LANGUAGE_LOG_TRACKER.get_or_init(UnknownLanguageLogTracker::new)
}

fn language_fingerprint(language: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    language.hash(&mut hasher);
    hasher.finish()
}

fn log_safe_language(language: &str) -> String {
    let Some(truncated) = truncate_to_utf8_boundary(language, MAX_LOGGED_LANGUAGE_BYTES) else {
        return language.escape_debug().to_string();
    };

    format!("{}...(truncated)", truncated.escape_debug())
}

fn truncate_to_utf8_boundary(value: &str, max_bytes: usize) -> Option<&str> {
    if value.len() <= max_bytes {
        return None;
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }

    Some(&value[..end])
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

    #[test]
    fn test_unknown_language_log_trackerは同じ言語を初回だけ記録対象にする() {
        let tracker = UnknownLanguageLogTracker::new();

        assert!(tracker.mark_seen("unknown-lang"));
        assert!(!tracker.mark_seen("unknown-lang"));
    }

    #[test]
    fn test_unknown_language_log_trackerは異なる言語をそれぞれ初回記録対象にする() {
        let tracker = UnknownLanguageLogTracker::new();

        assert!(tracker.mark_seen("unknown-lang"));
        assert!(tracker.mark_seen("another-lang"));
        assert!(!tracker.mark_seen("unknown-lang"));
        assert!(!tracker.mark_seen("another-lang"));
    }

    #[test]
    fn test_unknown_language_log_trackerは上限到達後に新規言語を記録対象にしない() {
        let tracker = UnknownLanguageLogTracker::new();

        for index in 0..MAX_TRACKED_UNKNOWN_LANGUAGES {
            assert!(tracker.mark_seen(&format!("lang-{index}")));
        }

        assert!(!tracker.mark_seen("overflow-lang"));
        assert!(!tracker.mark_seen("lang-0"));
    }

    #[test]
    fn test_log_safe_languageは制御文字をescapeする() {
        assert_eq!(
            log_safe_language("bad\nlang\t\u{1b}"),
            r#"bad\nlang\t\u{1b}"#
        );
    }

    #[test]
    fn test_log_safe_languageは長い言語名をutf8境界で切り詰める() {
        let language = format!("{}あ", "a".repeat(MAX_LOGGED_LANGUAGE_BYTES - 1));

        assert_eq!(
            log_safe_language(&language),
            format!("{}...(truncated)", "a".repeat(MAX_LOGGED_LANGUAGE_BYTES - 1))
        );
    }
}
