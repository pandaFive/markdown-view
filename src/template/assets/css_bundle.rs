use std::sync::OnceLock;

const TEMPLATE: &str = concat!(
    include_str!("css/base.css"),
    "\n",
    include_str!("css/sidebar.css"),
    "\n",
    include_str!("css/content.css"),
    "\n",
    include_str!("css/memo.css"),
    "\n",
    include_str!("css/file_tree.css"),
    "\n",
    include_str!("css/overlay.css"),
);

pub(super) fn css(dark_theme_vars: &str) -> &'static str {
    static CSS: OnceLock<String> = OnceLock::new();
    CSS.get_or_init(|| TEMPLATE.replace("__DARK_THEME_VARS__", dark_theme_vars))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARK_THEME_SENTINEL: &str = "__DARK_THEME_VARS__";

    fn count_occurrences(source: &str, needle: &str) -> usize {
        source.matches(needle).count()
    }

    #[test]
    fn test_dark_theme_sentinelはbase_cssだけに存在する() {
        let allowed_base_css = include_str!("css/base.css");
        let expected_sentinel_count = count_occurrences(allowed_base_css, DARK_THEME_SENTINEL);
        let disallowed_sources = [
            ("css/sidebar.css", include_str!("css/sidebar.css")),
            ("css/content.css", include_str!("css/content.css")),
            ("css/memo.css", include_str!("css/memo.css")),
            ("css/file_tree.css", include_str!("css/file_tree.css")),
            ("css/overlay.css", include_str!("css/overlay.css")),
        ];

        assert_eq!(
            expected_sentinel_count, 2,
            "base.css の dark theme sentinel 出現回数が変わった"
        );

        let mut listed_sentinel_count = expected_sentinel_count;
        for (path, source) in disallowed_sources {
            let source_sentinel_count = count_occurrences(source, DARK_THEME_SENTINEL);
            listed_sentinel_count += source_sentinel_count;
            assert!(
                source_sentinel_count == 0,
                "{path} に dark theme sentinel が混入している"
            );
        }

        assert_eq!(
            count_occurrences(TEMPLATE, DARK_THEME_SENTINEL),
            listed_sentinel_count,
            "CSS template include一覧とsentinel契約テストの一覧が同期していない"
        );
    }

    #[test]
    fn test_css生成後にdark_theme_sentinelが残らない() {
        let generated = TEMPLATE.replace(DARK_THEME_SENTINEL, ":root { --test-color: #fff; }");

        assert!(
            !generated.contains(DARK_THEME_SENTINEL),
            "生成済み CSS に dark theme sentinel が残っている"
        );
    }
}
