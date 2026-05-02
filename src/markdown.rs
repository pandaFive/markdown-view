use pulldown_cmark::Options;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownProfile {
    Render,
    Search,
}

pub(crate) fn markdown_options(profile: MarkdownProfile) -> Options {
    match profile {
        MarkdownProfile::Render => render_options(),
        MarkdownProfile::Search => {
            let mut options = render_options();
            options.insert(Options::ENABLE_FOOTNOTES);
            options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
            options.insert(Options::ENABLE_GFM);
            options
        }
    }
}

fn render_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_contains_all(options: Options, expected: &[Options]) {
        for option in expected {
            assert!(
                options.contains(*option),
                "expected options to contain {:?}",
                option
            );
        }
    }

    #[test]
    fn test_render_profileは表示用gfm_subsetだけを有効化する() {
        let options = markdown_options(MarkdownProfile::Render);

        assert_contains_all(
            options,
            &[
                Options::ENABLE_TABLES,
                Options::ENABLE_TASKLISTS,
                Options::ENABLE_STRIKETHROUGH,
            ],
        );
        assert!(!options.contains(Options::ENABLE_FOOTNOTES));
        assert!(!options.contains(Options::ENABLE_HEADING_ATTRIBUTES));
        assert!(!options.contains(Options::ENABLE_GFM));
    }

    #[test]
    fn test_search_profileはrender_profileの上位互換として検索用拡張を有効化する() {
        let render_options = markdown_options(MarkdownProfile::Render);
        let search_options = markdown_options(MarkdownProfile::Search);

        assert_eq!(search_options & render_options, render_options);
        assert_contains_all(
            search_options,
            &[
                Options::ENABLE_TABLES,
                Options::ENABLE_TASKLISTS,
                Options::ENABLE_STRIKETHROUGH,
                Options::ENABLE_FOOTNOTES,
                Options::ENABLE_HEADING_ATTRIBUTES,
                Options::ENABLE_GFM,
            ],
        );
    }
}
