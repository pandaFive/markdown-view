mod assets;
mod message;
mod page;
mod tree;

pub use self::assets::{combined_css, csp_hash_sources};
pub use self::message::{
    error_message_json, MemoResponse, MemoState, MemoUpdateMessage, UpdateMessage,
};
pub use self::page::{render_page, RenderPageParams, SidebarParams};
pub use self::tree::{build_file_tree, render_file_tree_html, FileTreeNode};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{render_markdown, syntax_theme_css};
    use crate::toc::generate_toc;

    #[test]
    fn test_public_apiでページとcsp_hashを組み合わせられる() {
        let content = render_markdown("content");
        let toc = generate_toc("# toc");
        let memo = MemoResponse::empty(None);
        let syntax_css = syntax_theme_css(None);

        let html = render_page(RenderPageParams {
            title: "Public API",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });
        let (script_hash, style_hash) = csp_hash_sources(&syntax_css);

        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Public API"));
        assert!(script_hash.starts_with("'sha256-"));
        assert!(style_hash.starts_with("'sha256-"));
    }
}
