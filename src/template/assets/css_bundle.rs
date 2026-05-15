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
        let disallowed_sources = [
            ("css/sidebar.css", include_str!("css/sidebar.css")),
            ("css/content.css", include_str!("css/content.css")),
            ("css/memo.css", include_str!("css/memo.css")),
            ("css/file_tree.css", include_str!("css/file_tree.css")),
            ("css/overlay.css", include_str!("css/overlay.css")),
        ];

        assert_eq!(
            count_occurrences(allowed_base_css, DARK_THEME_SENTINEL),
            2,
            "base.css の dark theme sentinel 出現回数が変わった"
        );

        for (path, source) in disallowed_sources {
            assert!(
                !source.contains(DARK_THEME_SENTINEL),
                "{path} に dark theme sentinel が混入している"
            );
        }

        assert_eq!(
            count_occurrences(TEMPLATE, DARK_THEME_SENTINEL),
            2,
            "結合済み CSS template の dark theme sentinel 出現回数が変わった"
        );
    }

    #[test]
    fn test_css生成後にdark_theme_sentinelが残らない() {
        let generated = css(":root { --test-color: #fff; }");

        assert!(
            !generated.contains(DARK_THEME_SENTINEL),
            "生成済み CSS に dark theme sentinel が残っている"
        );
    }
}
