use std::sync::OnceLock;

const TEMPLATE: &str = concat!(
    include_str!("css/base.css"),
    "\n",
    include_str!("css/sidebar.css"),
    "\n",
    include_str!("css/content.css"),
    "\n",
    include_str!("css/file_tree.css"),
    "\n",
    include_str!("css/overlay.css"),
);

pub(super) fn css(dark_theme_vars: &str) -> &'static str {
    static CSS: OnceLock<String> = OnceLock::new();
    CSS.get_or_init(|| TEMPLATE.replace("__DARK_THEME_VARS__", dark_theme_vars))
}
