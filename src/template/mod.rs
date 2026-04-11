mod assets;
mod message;
mod page;
mod tree;

pub use self::assets::{combined_css, csp_hash_sources};
pub use self::message::{error_message_json, MemoResponse, MemoUpdateMessage, UpdateMessage};
pub use self::page::{render_page, RenderPageParams, SidebarParams};
pub use self::tree::{build_file_tree, render_file_tree_html, FileTreeNode};

#[cfg(test)]
use self::assets::{css, inline_js};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{render_markdown, syntax_theme_css, SanitizedHtml};
    use crate::toc::generate_toc;

    fn test_content() -> SanitizedHtml {
        render_markdown("content")
    }

    fn test_toc() -> SanitizedHtml {
        generate_toc("# toc")
    }

    fn test_memo() -> MemoResponse {
        MemoResponse::new(String::new(), render_markdown(""), None)
    }

    #[test]
    fn test_フラットファイルリストからツリーを構築() {
        let files = vec![
            "README.md".to_string(),
            "docs/api.md".to_string(),
            "docs/guide/intro.md".to_string(),
        ];
        let tree = build_file_tree(&files);

        // ルート直下: ディレクトリ(docs)が先、ファイル(README.md)が後
        assert_eq!(tree.len(), 2);

        // docs ディレクトリ
        match &tree[0] {
            FileTreeNode::Directory { name, children } => {
                assert_eq!(name, "docs");
                assert_eq!(children.len(), 2);
                match &children[0] {
                    FileTreeNode::Directory {
                        name,
                        children: nested,
                    } => {
                        assert_eq!(name, "guide");
                        assert_eq!(nested.len(), 1);
                        match &nested[0] {
                            FileTreeNode::File { name, full_path } => {
                                assert_eq!(name, "intro.md");
                                assert_eq!(full_path, "docs/guide/intro.md");
                            }
                            _ => panic!("docs/guide/intro.md はファイルノードを期待"),
                        }
                    }
                    _ => panic!("docs/guide はディレクトリノードを期待"),
                }
                match &children[1] {
                    FileTreeNode::File { name, full_path } => {
                        assert_eq!(name, "api.md");
                        assert_eq!(full_path, "docs/api.md");
                    }
                    _ => panic!("docs/api.md はファイルノードを期待"),
                }
            }
            _ => panic!("ルート先頭はdocsディレクトリを期待"),
        }

        match &tree[1] {
            FileTreeNode::File { name, full_path } => {
                assert_eq!(name, "README.md");
                assert_eq!(full_path, "README.md");
            }
            _ => panic!("README.md はファイルノードを期待"),
        }
    }

    #[test]
    fn test_空のファイルリストからツリーを構築() {
        let files: Vec<String> = vec![];
        let tree = build_file_tree(&files);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_ルート直下のファイルのみ() {
        let files = vec!["README.md".to_string(), "CHANGELOG.md".to_string()];
        let tree = build_file_tree(&files);

        // すべてファイルノード、ディレクトリノードなし
        assert_eq!(tree.len(), 2);
        assert!(matches!(
            &tree[0],
            FileTreeNode::File { name, full_path } if name == "CHANGELOG.md" && full_path == "CHANGELOG.md"
        ));
        assert!(matches!(
            &tree[1],
            FileTreeNode::File { name, full_path } if name == "README.md" && full_path == "README.md"
        ));
    }

    #[test]
    fn test_深いネストのファイルツリー() {
        let files = vec!["a/b/c/d.md".to_string()];
        let tree = build_file_tree(&files);

        // a/
        assert_eq!(tree.len(), 1);
        match &tree[0] {
            FileTreeNode::Directory { name, children } => {
                assert_eq!(name, "a");
                assert_eq!(children.len(), 1);
                match &children[0] {
                    FileTreeNode::Directory {
                        name,
                        children: b_children,
                    } => {
                        assert_eq!(name, "b");
                        assert_eq!(b_children.len(), 1);
                        match &b_children[0] {
                            FileTreeNode::Directory {
                                name,
                                children: c_children,
                            } => {
                                assert_eq!(name, "c");
                                assert_eq!(c_children.len(), 1);
                                assert!(matches!(
                                    &c_children[0],
                                    FileTreeNode::File { name, full_path }
                                        if name == "d.md" && full_path == "a/b/c/d.md"
                                ));
                            }
                            _ => panic!("cディレクトリを期待"),
                        }
                    }
                    _ => panic!("bディレクトリを期待"),
                }
            }
            _ => panic!("aディレクトリを期待"),
        }
    }

    #[test]
    fn test_ツリーhtmlにアクティブファイルのパスが展開される() {
        let files = vec!["README.md".to_string(), "docs/guide/intro.md".to_string()];
        let tree = build_file_tree(&files);
        let html = render_file_tree_html(&tree, Some("docs/guide/intro.md"));

        // ルートはulでラップされる
        assert!(html.starts_with("<ul class=\"file-tree-root\">"));
        // アクティブファイルの祖先ディレクトリがopen状態
        assert!(html.contains("<details class=\"file-tree-dir\" open>"));
        // アクティブファイルにactiveクラスが付与される
        assert!(html.contains("class=\"file-tree-file active\""));
        // data-file属性が正しい
        assert!(html.contains("data-file=\"docs/guide/intro.md\""));
    }

    #[test]
    fn test_アクティブファイル判定でcurrent_fileの空セグメントを正規化する() {
        let files = vec!["README.md".to_string(), "docs/guide/intro.md".to_string()];
        let tree = build_file_tree(&files);
        let html = render_file_tree_html(&tree, Some("docs//guide//intro.md"));

        // 正規化により同一ファイルとしてactive判定される
        assert!(html.contains("class=\"file-tree-file active\""));
        // 祖先ディレクトリもopen状態になる
        assert!(html.contains("<details class=\"file-tree-dir\" open>"));
    }

    #[test]
    fn test_ファイル名のエスケープがツリーhtmlで維持される() {
        let files = vec!["A&B \"<notes>\".md".to_string()];
        let tree = build_file_tree(&files);
        let html = render_file_tree_html(&tree, None);

        // &, <, >, " がエスケープされている
        assert!(html.contains("A&amp;B &quot;&lt;notes&gt;&quot;.md"));
        // 生の特殊文字がdata-file属性に含まれない
        assert!(!html.contains("data-file=\"A&B \"<notes>\".md\""));
        // data-file属性値のダブルクォートがエスケープされる
        assert!(html.contains("data-file=\"A&amp;B &quot;&lt;notes&gt;&quot;.md\""));
    }

    #[test]
    fn test_空セグメントと重複パスを除去してツリー構築() {
        let files = vec![
            "docs//guide.md".to_string(),
            "docs/guide.md".to_string(),
            "///".to_string(),
        ];
        let tree = build_file_tree(&files);

        assert_eq!(tree.len(), 1);
        match &tree[0] {
            FileTreeNode::Directory { name, children } => {
                assert_eq!(name, "docs");
                assert_eq!(children.len(), 1);
                assert!(matches!(
                    &children[0],
                    FileTreeNode::File { name, full_path }
                        if name == "guide.md" && full_path == "docs/guide.md"
                ));
            }
            _ => panic!("docs ディレクトリを期待"),
        }
    }

    #[test]
    fn test_ディレクトリモードでタブ構造が生成される() {
        let files = vec!["README.md".to_string(), "docs/guide.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                directory_name: "workspace",
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        assert!(html.contains("data-dir-mode=\"true\""));
        assert!(html.contains("<h2>workspace</h2>"));
        assert!(html.contains("class=\"sidebar-tabs\""));
        assert!(html.contains("data-tab=\"files\""));
        assert!(html.contains("data-tab=\"toc\""));
        assert!(html.contains("id=\"panel-files\""));
        assert!(html.contains("id=\"panel-toc\""));
        assert!(html.contains("id=\"file-filter\""));
        assert!(html.contains("id=\"file-filter-summary\""));
        assert!(html.contains("<div class=\"sidebar-panel active\" id=\"panel-files\">"));
        assert!(html.contains("<div class=\"sidebar-utility\">"));
        assert!(html.contains("id=\"document-search-input\""));
        assert!(html.contains("id=\"document-search-summary\""));
        assert!(html.contains("id=\"document-search-results\""));
        assert!(html.contains("ディレクトリ検索"));
        assert!(html.contains("placeholder=\"ディレクトリ全体を検索\""));
        assert!(!html.contains("sidebar-caption"));
        assert!(!html.contains("ディレクトリ内のMarkdownを切り替えて閲覧できます。"));
    }

    #[test]
    fn test_単一ファイルモードでタブが生成されない() {
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains("class=\"sidebar-tabs\""));
        assert!(!html.contains("id=\"file-filter\""));
        assert!(html.contains("id=\"toc-filter\""));
        assert!(html.contains("id=\"document-search-input\""));
        assert!(html.contains("id=\"document-search-results\""));
        assert!(html.contains("本文検索"));
        assert!(html.contains("placeholder=\"本文を検索\""));
        assert!(html.contains("id=\"panel-memo\""));
        assert!(!html.contains("sidebar-caption"));
        assert!(!html.contains("目次とメモを横断して読書メモを残せます。"));
    }

    #[test]
    fn test_単一ファイルモードのメタラベルが描画される() {
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains("id=\"doc-mode\">Single file</span>"));
        assert!(html.contains("id=\"doc-file-count\">1 file</span>"));
    }

    #[test]
    fn test_ディレクトリモードのメタラベルが描画される() {
        let files = vec!["README.md".to_string(), "docs/guide.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                directory_name: "workspace",
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        assert!(html.contains("id=\"doc-mode\">Directory</span>"));
        assert!(html.contains("id=\"doc-file-count\">2 files</span>"));
    }

    #[test]
    fn test_読書ワークスペース用uiが描画される() {
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains("Markdown Workspace"));
        assert!(html.contains("id=\"document-title\""));
        assert!(html.contains("id=\"doc-heading-count\""));
        assert!(html.contains("id=\"doc-char-count\""));
        assert!(html.contains("id=\"live-status\""));
        assert!(html.contains("id=\"reading-progress-bar\""));
        assert!(html.contains("id=\"back-to-top\""));
    }

    #[test]
    fn test_sidebar_openがtopbar_btnクラスを共有する() {
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains("id=\"sidebar-open\" class=\"topbar-btn sidebar-open\""));
    }

    #[test]
    fn test_selectfile_fetch失敗時の視覚フィードバックjsが埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                directory_name: "workspace",
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        assert!(html.contains("function showFileFetchErrorBanner(message)"));
        assert!(html.contains("function hideFileFetchErrorBanner()"));
        assert!(html.contains("file-fetch-error-banner"));
        assert!(html.contains("className = 'error-banner fetch'"));
        assert!(html.contains("closeBtn.onclick = hideFileFetchErrorBanner"));
        assert!(html.contains("getElementById('ws-disconnect-banner')"));
        assert!(html.contains("function createHttpError(status)"));
        assert!(html.contains("function getFileFetchErrorMessage(err)"));
        assert!(html.contains("err.type = 'parse';"));
        assert!(html.contains("hideFileFetchErrorBanner();"));
        assert!(html.contains("showFileFetchErrorBanner(getFileFetchErrorMessage(err));"));
    }

    #[test]
    fn test_websocket更新時にfetchエラーバナーがクリアされる() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                directory_name: "workspace",
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        assert!(html.contains("hideWsServerErrorBanner();"));
        assert!(html.contains("hideFileFetchErrorBanner();"));
    }

    #[test]
    fn test_websocket_data_error時の視覚フィードバックjsが埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                directory_name: "workspace",
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        assert!(html.contains("function showWsServerErrorBanner(message)"));
        assert!(html.contains("function hideWsServerErrorBanner()"));
        assert!(html.contains("ws-server-error-banner"));
        assert!(html.contains("className = 'error-banner server'"));
        assert!(html.contains("className = 'error-banner disconnect'"));
        assert!(html.contains("closeBtn.onclick = hideWsServerErrorBanner"));
        assert!(html.contains("getElementById('ws-disconnect-banner')"));
        assert!(html.contains("showWsServerErrorBanner(data.error);"));
    }

    #[test]
    fn test_websocket_json_parse_error時の視覚フィードバックjsが埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                directory_name: "workspace",
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        assert!(html.contains("function showWsParseErrorBanner(message)"));
        assert!(html.contains("function hideWsParseErrorBanner()"));
        assert!(html.contains("ws-parse-error-banner"));
        assert!(html.contains(
            "showWsParseErrorBanner('サーバーから不正なJSONを受信しました。ページを再読み込みしてください。');"
        ));
        assert!(html.contains("hideWsParseErrorBanner();"));
    }

    #[test]
    fn test_websocket_refresh時もテキスト選択延期機構を通る() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                directory_name: "workspace",
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        assert!(html.contains("function ensurePendingUpdateTimer()"));
        assert!(html.contains("if (data.refresh && isDirMode && currentFile) {"));
        assert!(html.contains("if (isTextSelected()) {"));
        assert!(html.contains("pendingUpdate = { refresh: true, file: currentFile };"));
        assert!(html.contains("ensurePendingUpdateTimer();"));
        assert!(html.contains("if (pendingUpdate.refresh) {"));
        assert!(html.contains("selectFile(refreshFile, false);"));
    }

    #[test]
    fn test_websocket_memo_update受信処理が埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                directory_name: "workspace",
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        assert!(html.contains("function isMemoUpdateMessage(data)"));
        assert!(html.contains("function isMemoRefreshMessage(data)"));
        assert!(html.contains("function applyRemoteMemoUpdate(data)"));
        assert!(html.contains("function queueRemoteMemoReload(data)"));
        assert!(html.contains("var pendingMemoReload = null;"));
        assert!(html.contains("function flushPendingMemoReloadIfSafe()"));
        assert!(html.contains("if (pendingMemoReload === null) return false;"));
        assert!(html.contains("if (isMemoUpdateMessage(data)) {"));
        assert!(html.contains("if (applyRemoteMemoUpdate(data)) {"));
        assert!(html.contains("if (isMemoRefreshMessage(data)) {"));
        assert!(html.contains("if (queueRemoteMemoReload(data)) {"));
        assert!(html.contains("loadMemo(file, fetchGeneration);"));
    }

    #[test]
    fn test_websocket_memo_updateは編集中の上書きを回避する() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                directory_name: "workspace",
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        assert!(html.contains("function getMemoRemoteUpdateBlockReason()"));
        assert!(html.contains("pendingMemoReload = data.file;"));
        assert!(html.contains("return flushPendingMemoReloadIfSafe();"));
    }

    #[test]
    fn test_copyハンドラが共通化されている() {
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains("function handleCopyClick(button, text, baseLabel)"));
        assert!(html.contains("handleCopyClick(button, url.toString(), '#');"));
        assert!(html.contains(
            "handleCopyClick(button, code.innerText || code.textContent || '', 'Copy');"
        ));
    }

    #[test]
    fn test_live_statusラベルが内部解決される() {
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains("var LIVE_STATUS_LABELS = {"));
        assert!(html.contains("function setLiveStatus(state) {"));
        assert!(!html.contains("function setLiveStatus(state, label) {"));
    }

    #[test]
    fn test_メモuiが描画される() {
        let content = test_content();
        let toc = test_toc();
        let memo = MemoResponse::new(
            "引用メモ".to_string(),
            render_markdown("> 引用メモ"),
            Some("README.md".to_string()),
        );
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains("id=\"memo-editor\""));
        assert!(html.contains("id=\"memo-preview\""));
        assert!(html.contains("id=\"memo-save-status\""));
        assert!(html.contains("Research Notes"));
        assert!(
            !html.contains("本文選択から引用を追加できます。出典リンクと行番号を自動付与します。")
        );
        assert!(!html.contains("memo-caption"));
        assert!(html.contains("id=\"quote-selection-action\""));
        assert!(html.contains("data-memo-file=\"README.md\""));
    }

    #[test]
    fn test_memo_response_fileフィールドが直列化される() {
        let memo = MemoResponse::new(
            "memo".to_string(),
            render_markdown("memo"),
            Some("docs/guide.md".to_string()),
        );
        let value = serde_json::to_value(memo).unwrap();
        assert_eq!(value["file"], "docs/guide.md");
        assert_eq!(value["raw"], "memo");
        assert!(value.get("html").is_some());
    }

    #[test]
    fn test_cspハッシュがrender_pageのstyle内容と一致する() {
        use base64::Engine as _;
        use sha2::Digest as _;

        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let (script_src, style_src) = csp_hash_sources(&syntax_css);

        let expected_style_hash = {
            let css_content = combined_css(&syntax_css);
            let digest = sha2::Sha256::digest(css_content.as_bytes());
            format!(
                "'sha256-{}'",
                base64::engine::general_purpose::STANDARD.encode(digest)
            )
        };
        let expected_script_hash = {
            let digest = sha2::Sha256::digest(inline_js().as_bytes());
            format!(
                "'sha256-{}'",
                base64::engine::general_purpose::STANDARD.encode(digest)
            )
        };

        assert_eq!(style_src, expected_style_hash, "style-srcハッシュが不一致");
        assert_eq!(
            script_src, expected_script_hash,
            "script-srcハッシュが不一致"
        );
    }

    #[test]
    fn test_csp_hash_sources_複数テーマでstyleハッシュが変化しscriptは固定() {
        let dark_css = syntax_theme_css(Some("base16-ocean.dark"));
        let light_css = syntax_theme_css(Some("InspiredGitHub"));

        let (dark_script, dark_style) = csp_hash_sources(&dark_css);
        let (light_script, light_style) = csp_hash_sources(&light_css);

        assert_eq!(
            dark_script, light_script,
            "script-srcハッシュはテーマによらず固定であるべき"
        );
        assert_ne!(
            dark_style, light_style,
            "style-srcハッシュはテーマごとに変化するべき"
        );
    }

    #[test]
    fn test_update_message_fileフィールドが直列化される() {
        let message = UpdateMessage::new(
            test_content(),
            test_toc(),
            Some("docs/guide.md".to_string()),
        );
        let value = serde_json::to_value(message).unwrap();
        assert_eq!(value["file"], "docs/guide.md");
        assert!(value.get("content").is_some());
        assert!(value.get("toc").is_some());
    }

    #[test]
    fn test_combined_css_空のsyntax_cssはベースcssのみを返す() {
        let combined = combined_css("");
        assert_eq!(combined, css());
        assert!(!combined.trim().is_empty());
    }
}
