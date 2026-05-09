const TEMPLATE: &str = concat!(
    "(function() {\n",
    include_str!("js/bootstrap.js"),
    "\n",
    include_str!("js/selection.js"),
    "\n",
    include_str!("js/content-renderer.js"),
    "\n",
    include_str!("js/content-enhancements.js"),
    "\n",
    include_str!("js/content-navigation.js"),
    "\n",
    include_str!("js/document-search.js"),
    "\n",
    include_str!("js/content.js"),
    "\n",
    include_str!("js/memo.js"),
    "\n",
    include_str!("js/fetch.js"),
    "\n",
    include_str!("js/websocket.js"),
    "\n",
    include_str!("js/sidebar.js"),
    "\n",
    "startMarkdownViewApp();\n",
    "}());\n",
);

pub(super) fn inline_js(max_file_size: u64) -> String {
    TEMPLATE.replace(
        "__MAX_FILE_SIZE_MB__",
        &(max_file_size / 1024 / 1024).to_string(),
    )
}
