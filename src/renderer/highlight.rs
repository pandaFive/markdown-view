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
    state: Mutex<UnknownLanguageLogState>,
}

struct UnknownLanguageLogState {
    seen: HashSet<u64>,
    limit_reached_logged: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum UnknownLanguageLogDecision {
    LogLanguage,
    LogLimitReached,
    Suppress,
}

impl UnknownLanguageLogTracker {
    fn new() -> Self {
        Self {
            state: Mutex::new(UnknownLanguageLogState {
                seen: HashSet::new(),
                limit_reached_logged: false,
            }),
        }
    }

    fn mark_seen(&self, language: &str) -> UnknownLanguageLogDecision {
        let fingerprint = language_fingerprint(language);
        let mut state = self
            .state
            .lock()
            .expect("未知言語ログの重複抑制状態をロックできること");

        if state.seen.contains(&fingerprint) {
            return UnknownLanguageLogDecision::Suppress;
        }

        if state.seen.len() >= MAX_TRACKED_UNKNOWN_LANGUAGES {
            if state.limit_reached_logged {
                return UnknownLanguageLogDecision::Suppress;
            }

            state.limit_reached_logged = true;
            return UnknownLanguageLogDecision::LogLimitReached;
        }

        state.seen.insert(fingerprint);
        UnknownLanguageLogDecision::LogLanguage
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

fn should_log_unknown_language_fallback(
    tracker: &UnknownLanguageLogTracker,
    language: &str,
) -> UnknownLanguageLogDecision {
    tracker.mark_seen(language)
}

fn log_unknown_language_fallback(language: &str) {
    log_unknown_language_fallback_with_tracker(unknown_language_log_tracker(), language);
}

fn log_unknown_language_fallback_with_tracker(tracker: &UnknownLanguageLogTracker, language: &str) {
    match should_log_unknown_language_fallback(tracker, language) {
        UnknownLanguageLogDecision::LogLanguage => {
            tracing::debug!(
                "[markdown-view] 未知のコードブロック言語のためプレーン表示にフォールバックしました (lang={})",
                log_safe_language(language)
            );
        }
        UnknownLanguageLogDecision::LogLimitReached => {
            tracing::debug!(
                "[markdown-view] 未知のコードブロック言語 fallback ログが上限に達したため以降の新規言語ログを抑制します (limit={})",
                MAX_TRACKED_UNKNOWN_LANGUAGES
            );
        }
        UnknownLanguageLogDecision::Suppress => {}
    }
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
    let Some(syntax) = syntax_set
        .find_syntax_by_token(language)
        .or_else(|| syntax_set.find_syntax_by_extension(language))
    else {
        log_unknown_language_fallback(language);
        return None;
    };

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
    use std::collections::BTreeMap;
    use std::fmt;
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing::{Event, Level, Subscriber};
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::{Layer, Registry};

    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct CapturedEvent {
        level: Level,
        fields: BTreeMap<String, String>,
    }

    #[derive(Default)]
    struct CapturedFields {
        values: BTreeMap<String, String>,
    }

    impl Visit for CapturedFields {
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.values
                .insert(field.name().to_string(), format!("{value:?}"));
        }
    }

    #[derive(Clone, Default)]
    struct EventCapture(Arc<Mutex<Vec<CapturedEvent>>>);

    impl<S> Layer<S> for EventCapture
    where
        S: Subscriber,
    {
        fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
            let mut fields = CapturedFields::default();
            event.record(&mut fields);
            self.0
                .lock()
                .expect("event capture lock")
                .push(CapturedEvent {
                    level: *event.metadata().level(),
                    fields: fields.values,
                });
        }
    }

    fn capture_unknown_language_events(action: impl FnOnce()) -> Vec<CapturedEvent> {
        let capture = EventCapture::default();
        let events = Arc::clone(&capture.0);
        let subscriber = Registry::default().with(capture);

        tracing::subscriber::with_default(subscriber, action);

        let captured = events.lock().expect("event capture lock").clone();
        captured
    }

    fn fallback_log_events(events: &[CapturedEvent], language: &str) -> usize {
        events
            .iter()
            .filter(|event| {
                event.level == Level::DEBUG
                    && event.fields.get("message").is_some_and(|message| {
                        message.contains(
                            "未知のコードブロック言語のためプレーン表示にフォールバックしました",
                        ) && message.contains(language)
                    })
            })
            .count()
    }

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

        assert_eq!(
            tracker.mark_seen("unknown-lang"),
            UnknownLanguageLogDecision::LogLanguage
        );
        assert_eq!(
            tracker.mark_seen("unknown-lang"),
            UnknownLanguageLogDecision::Suppress
        );
    }

    #[test]
    fn test_unknown_language_log_trackerは異なる言語をそれぞれ初回記録対象にする() {
        let tracker = UnknownLanguageLogTracker::new();

        assert_eq!(
            tracker.mark_seen("unknown-lang"),
            UnknownLanguageLogDecision::LogLanguage
        );
        assert_eq!(
            tracker.mark_seen("another-lang"),
            UnknownLanguageLogDecision::LogLanguage
        );
        assert_eq!(
            tracker.mark_seen("unknown-lang"),
            UnknownLanguageLogDecision::Suppress
        );
        assert_eq!(
            tracker.mark_seen("another-lang"),
            UnknownLanguageLogDecision::Suppress
        );
    }

    #[test]
    fn test_unknown_language_log_trackerは上限到達時だけ抑制ログ対象にする() {
        let tracker = UnknownLanguageLogTracker::new();

        for index in 0..MAX_TRACKED_UNKNOWN_LANGUAGES {
            assert_eq!(
                tracker.mark_seen(&format!("lang-{index}")),
                UnknownLanguageLogDecision::LogLanguage
            );
        }

        assert_eq!(
            tracker.mark_seen("overflow-lang"),
            UnknownLanguageLogDecision::LogLimitReached
        );
        assert_eq!(
            tracker.mark_seen("another-overflow-lang"),
            UnknownLanguageLogDecision::Suppress
        );
        assert_eq!(
            tracker.mark_seen("lang-0"),
            UnknownLanguageLogDecision::Suppress
        );
    }

    #[test]
    fn test_log_unknown_language_fallbackは同じ言語を一度だけログ対象にする() {
        let tracker = UnknownLanguageLogTracker::new();

        assert_eq!(
            should_log_unknown_language_fallback(&tracker, "unknown-lang"),
            UnknownLanguageLogDecision::LogLanguage
        );
        assert_eq!(
            should_log_unknown_language_fallback(&tracker, "unknown-lang"),
            UnknownLanguageLogDecision::Suppress
        );
        assert_eq!(
            should_log_unknown_language_fallback(&tracker, "another-lang"),
            UnknownLanguageLogDecision::LogLanguage
        );
    }

    #[test]
    fn test_unknown_language_fallbackはrender経路でdebugログを出す() {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let language = "unknown-callsite-log";

        let events = capture_unknown_language_events(|| {
            let html = render_code_block_html(&syntax_set, Some(language), "let x = 1;", "");
            assert_eq!(
                html,
                "<pre class=\"code-block\"><code class=\"syn-code language-unknown-callsite-log\">let x = 1;</code></pre>\n"
            );
        });

        assert_eq!(fallback_log_events(&events, language), 1);
    }

    #[test]
    fn test_unknown_language_fallbackログは同一言語を一度だけ出す() {
        let tracker = UnknownLanguageLogTracker::new();
        let language = "unknown-dedupe-log";

        let events = capture_unknown_language_events(|| {
            log_unknown_language_fallback_with_tracker(&tracker, language);
            log_unknown_language_fallback_with_tracker(&tracker, language);
        });

        assert_eq!(fallback_log_events(&events, language), 1);
    }

    #[test]
    fn test_unknown_language_fallbackログは安全な言語名を出す() {
        let tracker = UnknownLanguageLogTracker::new();
        let language = format!("bad\nlang\t\u{1b}{}", "a".repeat(MAX_LOGGED_LANGUAGE_BYTES));

        let events = capture_unknown_language_events(|| {
            log_unknown_language_fallback_with_tracker(&tracker, &language);
        });
        let message = events
            .iter()
            .find_map(|event| event.fields.get("message"))
            .expect("fallback debug log message");

        assert!(message.contains(r#"\nlang\t\u{1b}"#));
        assert!(message.contains("...(truncated)"));
        assert!(!message.contains('\n'));
        assert!(!message.contains('\t'));
    }

    #[test]
    fn test_unknown_language_fallbackログは上限到達を一度だけ出す() {
        let tracker = UnknownLanguageLogTracker::new();

        let events = capture_unknown_language_events(|| {
            for index in 0..MAX_TRACKED_UNKNOWN_LANGUAGES {
                log_unknown_language_fallback_with_tracker(&tracker, &format!("lang-{index}"));
            }
            log_unknown_language_fallback_with_tracker(&tracker, "overflow-lang");
            log_unknown_language_fallback_with_tracker(&tracker, "another-overflow-lang");
        });

        let limit_reached_logs = events
            .iter()
            .filter(|event| {
                event.level == Level::DEBUG
                    && event.fields.get("message").is_some_and(|message| {
                        message.contains("fallback ログが上限に達した")
                            && message.contains("limit=256")
                    })
            })
            .count();

        assert_eq!(limit_reached_logs, 1);
        assert_eq!(fallback_log_events(&events, "overflow-lang"), 0);
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
            format!(
                "{}...(truncated)",
                "a".repeat(MAX_LOGGED_LANGUAGE_BYTES - 1)
            )
        );
    }
}
