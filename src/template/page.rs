use super::assets::{combined_css, inline_js};
use super::message::{MemoResponse, MemoState};
use super::tree::{build_file_tree, render_file_tree_html};
use crate::renderer::{html_escape, SanitizedHtml};

/// サイドバー描画パラメータ
pub enum SidebarParams<'a> {
    /// 単一ファイルモード（ブランド・目次検索・目次を表示）
    SingleFile,
    /// ディレクトリモード（ファイル一覧 + 目次）
    Directory {
        directory_name: &'a str,
        file_list: &'a [String],
        current_file: Option<&'a str>,
    },
}

/// HTMLテンプレートのパラメータ
pub struct RenderPageParams<'a> {
    pub title: &'a str,
    pub content: &'a SanitizedHtml,
    pub toc: &'a SanitizedHtml,
    pub memo: &'a MemoResponse,
    pub dark_mode: bool,
    /// syntectクラスベースハイライト用CSS
    pub syntax_css: &'a str,
    /// サイドバー表示モード
    pub sidebar: SidebarParams<'a>,
}

struct DocumentMeta {
    mode_label: String,
    file_count_label: String,
}

fn html_attr(name: &'static str, value: &str) -> String {
    format!(" {}=\"{}\"", name, html_escape(value))
}

/// HTMLテンプレートを生成する
///
/// CSS/JSをすべて埋め込み、外部ファイル不要で動作する
pub fn render_page(params: RenderPageParams<'_>) -> String {
    let escaped_title = html_escape(params.title);
    let (dir_mode_attr, sidebar_inner, meta) =
        render_sidebar(&params.sidebar, params.toc, params.memo);
    let memo_file_attr = params
        .memo
        .file()
        .map(|file| html_attr("data-memo-file", file))
        .unwrap_or_default();

    render_html_document(HtmlDocumentParts {
        theme: if params.dark_mode { "dark" } else { "light" },
        dir_mode_attr,
        memo_file_attr,
        title: escaped_title,
        title_attr: html_attr("data-title", params.title),
        css: combined_css(params.syntax_css),
        sidebar_inner,
        content: params.content.as_str().to_string(),
        js: inline_js(),
        mode_label: meta.mode_label,
        file_count_label: meta.file_count_label,
    })
}

struct HtmlDocumentParts {
    theme: &'static str,
    dir_mode_attr: String,
    memo_file_attr: String,
    title: String,
    title_attr: String,
    css: String,
    sidebar_inner: String,
    content: String,
    js: String,
    mode_label: String,
    file_count_label: String,
}

fn render_html_document(parts: HtmlDocumentParts) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="ja" data-theme="{theme}"{dir_mode_attr}{memo_file_attr}>
{head}
{body}
</html>"##,
        theme = parts.theme,
        dir_mode_attr = parts.dir_mode_attr,
        memo_file_attr = parts.memo_file_attr,
        head = render_head(&parts.title, &parts.css),
        body = render_workspace_body(&parts),
    )
}

fn render_head(title: &str, css: &str) -> String {
    format!(
        r##"<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - markdown-view</title>
<style>{css}</style>
</head>"##
    )
}

fn render_workspace_body(parts: &HtmlDocumentParts) -> String {
    format!(
        r##"<body>
<div class="app-shell">
<aside id="sidebar" class="sidebar">
{sidebar_inner}
</aside>
<div class="workspace">
<header class="topbar">
  <div class="topbar-copy">
    <p class="topbar-kicker">Markdown Workspace</p>
    <h1 id="document-title" class="document-title">{title}</h1>
    <div class="document-meta">
      <span class="meta-pill meta-pill-strong" id="doc-mode">{mode_label}</span>
      <span class="meta-pill" id="doc-file-count">{file_count_label}</span>
      <span class="meta-pill" id="doc-heading-count">見出し 0</span>
      <span class="meta-pill" id="doc-char-count">文字 0</span>
      <span class="meta-pill live-pill" id="live-status">Live</span>
    </div>
  </div>
  <div class="topbar-actions">
    <button id="theme-toggle" class="topbar-btn" aria-label="テーマ切替">
      <span class="theme-icon theme-icon-light">☀</span>
      <span class="theme-icon theme-icon-dark">☾</span>
    </button>
    <button id="sidebar-open" class="topbar-btn sidebar-open" aria-label="サイドバーを開く">☰</button>
  </div>
</header>
<div class="reading-progress" aria-hidden="true">
  <div id="reading-progress-bar" class="reading-progress-bar"></div>
</div>
<main id="content" class="content"{title_attr}>
{content}
</main>
</div>
</div>
<button id="back-to-top" class="back-to-top" aria-label="ページ上部へ戻る">↑</button>
<button id="quote-selection-action" class="quote-selection-action" type="button" hidden>引用を追加</button>
<script>{js}</script>
</body>"##,
        sidebar_inner = parts.sidebar_inner,
        title = parts.title,
        title_attr = parts.title_attr,
        mode_label = parts.mode_label,
        file_count_label = parts.file_count_label,
        content = parts.content,
        js = parts.js,
    )
}

fn render_sidebar(
    sidebar: &SidebarParams<'_>,
    toc: &SanitizedHtml,
    memo: &MemoResponse,
) -> (String, String, DocumentMeta) {
    let memo_editor = render_memo_panel(memo);
    match sidebar {
        SidebarParams::Directory {
            directory_name,
            file_list,
            current_file,
        } => {
            let document_search = render_document_search(true);
            let tree = build_file_tree(file_list);
            let tree_html = render_file_tree_html(&tree, *current_file);
            let dir_mode_attr = format!(
                "{}{}",
                html_attr("data-dir-mode", "true"),
                html_attr("data-current-file", current_file.unwrap_or(""))
            );
            let sidebar_inner = format!(
                r##"  <div class="sidebar-brand">
    <p class="sidebar-kicker">Workspace</p>
    <h2>{directory_name}</h2>
  </div>
  <div class="sidebar-tabs">
    <button class="sidebar-tab active" data-tab="files">ファイル</button>
    <button class="sidebar-tab" data-tab="toc">目次</button>
    <button class="sidebar-tab" data-tab="memo">メモ</button>
    <button id="sidebar-toggle" class="sidebar-toggle" aria-label="閉じる">×</button>
  </div>
  <div class="sidebar-panel active" id="panel-files">
    <div class="sidebar-utility">
      <label class="sidebar-search">
        <span>絞り込み</span>
        <input id="file-filter" type="search" placeholder="ファイル名で検索" autocomplete="off">
      </label>
      <p id="file-filter-summary" class="sidebar-summary">{file_count} files</p>
    </div>
    <div class="file-list">
{tree_html}    </div>
  </div>
  <div class="sidebar-panel" id="panel-toc">
    {document_search}
    <label class="sidebar-search sidebar-search-compact">
      <span>目次検索</span>
      <input id="toc-filter" type="search" placeholder="見出しを検索" autocomplete="off">
    </label>
    <nav id="toc">{toc}</nav>
  </div>
  <div class="sidebar-panel" id="panel-memo">
{memo_editor}
  </div>"##,
                tree_html = tree_html,
                document_search = document_search,
                toc = toc.as_str(),
                memo_editor = memo_editor,
                directory_name = html_escape(directory_name),
                file_count = file_list.len(),
            );
            (
                dir_mode_attr,
                sidebar_inner,
                DocumentMeta {
                    mode_label: "Directory".to_string(),
                    file_count_label: format!("{} files", file_list.len()),
                },
            )
        }
        SidebarParams::SingleFile => {
            let document_search = render_document_search(false);
            (
                String::new(),
                format!(
                    r##"  <div class="sidebar-brand">
    <p class="sidebar-kicker">Workspace</p>
    <h2>Annotations</h2>
  </div>
  <div class="sidebar-tabs">
    <button class="sidebar-tab active" data-tab="toc">目次</button>
    <button class="sidebar-tab" data-tab="memo">メモ</button>
    <button id="sidebar-toggle" class="sidebar-toggle" aria-label="目次を閉じる">×</button>
  </div>
  <div class="sidebar-panel active" id="panel-toc">
    {document_search}
    <label class="sidebar-search sidebar-search-compact">
      <span>目次検索</span>
      <input id="toc-filter" type="search" placeholder="見出しを検索" autocomplete="off">
    </label>
    <nav id="toc">{toc}</nav>
  </div>
  <div class="sidebar-panel" id="panel-memo">
{memo_editor}
  </div>"##,
                    document_search = document_search,
                    toc = toc.as_str(),
                    memo_editor = memo_editor,
                ),
                DocumentMeta {
                    mode_label: "Single file".to_string(),
                    file_count_label: "1 file".to_string(),
                },
            )
        }
    }
}

fn render_document_search(is_directory_mode: bool) -> String {
    let (aria_label, label, placeholder) = if is_directory_mode {
        (
            "ディレクトリ検索",
            "ディレクトリ検索",
            "ディレクトリ全体を検索",
        )
    } else {
        ("文書内検索", "本文検索", "本文を検索")
    };

    format!(
        r##"    <section class="document-search-shell" aria-label="{aria_label}">
      <label class="sidebar-search sidebar-search-compact">
        <span>{label}</span>
        <input id="document-search-input" type="search" placeholder="{placeholder}" autocomplete="off">
      </label>
      <div class="document-search-toolbar">
        <span id="document-search-summary" class="sidebar-summary">0 件</span>
        <div class="document-search-actions">
          <button id="document-search-prev" class="document-search-btn" type="button" aria-label="前の一致へ">↑</button>
          <button id="document-search-next" class="document-search-btn" type="button" aria-label="次の一致へ">↓</button>
          <button id="document-search-clear" class="document-search-btn" type="button" aria-label="検索をクリア">×</button>
        </div>
      </div>
      <div id="document-search-results" class="document-search-results" aria-live="polite"></div>
    </section>"##,
        aria_label = aria_label,
        label = label,
        placeholder = placeholder,
    )
}

fn render_memo_panel(memo: &MemoResponse) -> String {
    let is_degraded = memo.memo_state() == MemoState::Degraded;
    let status_state = if is_degraded { "error" } else { "saved" };
    let status_text = if is_degraded {
        "読込失敗"
    } else {
        "保存済み"
    };
    let textarea_attrs = if is_degraded {
        " disabled aria-disabled=\"true\""
    } else {
        ""
    };
    let degraded_message =
        "メモを読み込めませんでした。内容を保護するため編集を無効化しています。本文の閲覧は継続できます。";
    let degraded_banner = if is_degraded {
        format!(
            r##"      <div id="memo-degraded-banner" class="memo-degraded-banner" role="status">{message}</div>
"##,
            message = html_escape(degraded_message),
        )
    } else {
        String::new()
    };
    format!(
        r##"    <div class="memo-layout">
      <div class="memo-toolbar">
        <div>
          <h3>Research Notes</h3>
        </div>
        <span id="memo-save-status" class="memo-save-status" data-state="{status_state}">{status_text}</span>
      </div>
{degraded_banner}      <label class="memo-field">
        <span>メモ本文</span>
        <textarea id="memo-editor" placeholder="気づきや引用メモを残す"{textarea_attrs}>{memo_raw}</textarea>
      </label>
      <div class="memo-preview-shell">
        <div class="memo-preview-header">
          <span>Preview</span>
        </div>
        <div id="memo-preview" class="memo-preview">{memo_html}</div>
      </div>
    </div>"##,
        status_state = status_state,
        status_text = status_text,
        degraded_banner = degraded_banner,
        textarea_attrs = textarea_attrs,
        memo_raw = html_escape(memo.raw()),
        memo_html = memo.html().as_str(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{render_markdown, SanitizedHtml};
    use crate::toc::generate_toc;

    fn test_content() -> SanitizedHtml {
        render_markdown("content")
    }

    fn test_toc() -> SanitizedHtml {
        generate_toc("# toc")
    }

    fn test_memo() -> MemoResponse {
        MemoResponse::empty(None)
    }

    fn render_single_file_page(memo: &MemoResponse) -> String {
        let content = test_content();
        let toc = test_toc();
        render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo,
            dark_mode: false,
            syntax_css: "",
            sidebar: SidebarParams::SingleFile,
        })
    }

    fn render_directory_page(files: &[String], memo: &MemoResponse) -> String {
        let content = test_content();
        let toc = test_toc();
        render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            memo,
            dark_mode: false,
            syntax_css: "",
            sidebar: SidebarParams::Directory {
                directory_name: "workspace",
                file_list: files,
                current_file: Some("README.md"),
            },
        })
    }

    #[test]
    fn test_html_attrは属性値をescapeする() {
        assert_eq!(
            html_attr("data-file", "a\" onclick=\"x & <y>"),
            " data-file=\"a&quot; onclick=&quot;x &amp; &lt;y&gt;\""
        );
    }

    #[test]
    fn test_contentとtocは二重escapeしない() {
        let content = render_markdown("**ok**");
        let toc = generate_toc("# toc");
        let memo = MemoResponse::empty(None);

        let html = render_page(RenderPageParams {
            title: "Escape",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: "",
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains("<strong>"));
        assert!(html.contains("</strong>"));
        assert!(html.contains("href=\"#toc\""));
        assert!(!html.contains("&lt;strong&gt;ok&lt;/strong&gt;"));
    }

    #[test]
    fn test_title属性はhtml_attrでescapeする() {
        let content = test_content();
        let toc = test_toc();
        let memo = test_memo();

        let html = render_page(RenderPageParams {
            title: "a\" onclick=\"x",
            content: &content,
            toc: &toc,
            memo: &memo,
            dark_mode: false,
            syntax_css: "",
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains(" data-title=\"a&quot; onclick=&quot;x\""));
        assert!(!html.contains(" data-title=\"a\" onclick=\"x\""));
    }

    #[test]
    fn test_ディレクトリモードでタブ構造が生成される() {
        let files = vec!["README.md".to_string(), "docs/guide.md".to_string()];
        let memo = test_memo();
        let html = render_directory_page(&files, &memo);

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
        let memo = test_memo();
        let html = render_single_file_page(&memo);

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
        let memo = test_memo();
        let html = render_single_file_page(&memo);

        assert!(html.contains("id=\"doc-mode\">Single file</span>"));
        assert!(html.contains("id=\"doc-file-count\">1 file</span>"));
    }

    #[test]
    fn test_ディレクトリモードのメタラベルが描画される() {
        let files = vec!["README.md".to_string(), "docs/guide.md".to_string()];
        let memo = test_memo();
        let html = render_directory_page(&files, &memo);

        assert!(html.contains("id=\"doc-mode\">Directory</span>"));
        assert!(html.contains("id=\"doc-file-count\">2 files</span>"));
    }

    #[test]
    fn test_読書ワークスペース用uiが描画される() {
        let memo = test_memo();
        let html = render_single_file_page(&memo);

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
        let memo = test_memo();
        let html = render_single_file_page(&memo);

        assert!(html.contains("id=\"sidebar-open\" class=\"topbar-btn sidebar-open\""));
    }

    #[test]
    fn test_selectfile_fetch失敗時の視覚フィードバックjsが埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let memo = test_memo();
        let html = render_directory_page(&files, &memo);

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
        let memo = test_memo();
        let html = render_directory_page(&files, &memo);

        assert!(html.contains("hideWsServerErrorBanner();"));
        assert!(html.contains("hideFileFetchErrorBanner();"));
    }

    #[test]
    fn test_websocket_data_error時の視覚フィードバックjsが埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let memo = test_memo();
        let html = render_directory_page(&files, &memo);

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
        let memo = test_memo();
        let html = render_directory_page(&files, &memo);

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
        let memo = test_memo();
        let html = render_directory_page(&files, &memo);

        assert!(html.contains("function ensurePendingUpdateTimer()"));
        assert!(html.contains("if (data.refresh && ctx.config.isDirMode) {"));
        assert!(html.contains("現在ファイルが未設定のため refresh 通知を無視しました。"));
        assert!(html.contains("file を含まない refresh 通知を現在ファイルへ適用します。"));
        assert!(html.contains("if (isTextSelected()) {"));
        assert!(html
            .contains("ctx.state.pendingUpdate = { refresh: true, file: ctx.state.currentFile };"));
        assert!(html.contains("ensurePendingUpdateTimer();"));
        assert!(html.contains("discardBufferedLiveUpdate('ファイル切替を優先');"));
        assert!(html.contains("if (ctx.state.pendingUpdate.refresh) {"));
        assert!(html.contains("deps.selectFile(refreshFile, false);"));
    }

    #[test]
    fn test_websocket_memo_update受信処理が埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let memo = test_memo();
        let html = render_directory_page(&files, &memo);

        assert!(html.contains("function isMemoUpdateMessage(data)"));
        assert!(html.contains("function isMemoRefreshMessage(data)"));
        assert!(html.contains("function applyRemoteMemoUpdate(data)"));
        assert!(html.contains("function queueRemoteMemoReload(data)"));
        assert!(html.contains("pendingReload: null"));
        assert!(html.contains("function flushPendingMemoReloadIfSafe()"));
        assert!(html.contains("if (appContext.memo.pendingReload === null) return false;"));
        assert!(html.contains("if (isMemoUpdateMessage(data)) {"));
        assert!(html.contains("if (deps.applyRemoteMemoUpdate(data)) {"));
        assert!(html.contains(
            "if (isMemoRefreshMessage(data) && !(data.refresh && ctx.config.isDirMode)) {"
        ));
        assert!(html.contains("if (deps.queueRemoteMemoReload(data)) {"));
        assert!(html.contains("loadMemo(file, appContext.fetch.generation);"));
    }

    #[test]
    fn test_websocket_memo_updateは編集中の上書きを回避する() {
        let files = vec!["README.md".to_string()];
        let memo = test_memo();
        let html = render_directory_page(&files, &memo);

        assert!(html.contains("function getMemoRemoteUpdateBlockReason()"));
        assert!(html.contains("appContext.memo.pendingReload = data.file;"));
        assert!(html.contains("return flushPendingMemoReloadIfSafe();"));
    }

    #[test]
    fn test_copyハンドラが共通化されている() {
        let memo = test_memo();
        let html = render_single_file_page(&memo);

        assert!(html.contains("function handleCopyClick(button, text, baseLabel)"));
        assert!(html.contains("handleCopyClick(button, url.toString(), '#');"));
        assert!(html.contains(
            "handleCopyClick(button, code.innerText || code.textContent || '', 'Copy');"
        ));
    }

    #[test]
    fn test_live_statusラベルが内部解決される() {
        let memo = test_memo();
        let html = render_single_file_page(&memo);

        assert!(html.contains("liveStatus: {"));
        assert!(html.contains("function setLiveStatus(state) {"));
        assert!(!html.contains("function setLiveStatus(state, label) {"));
    }

    #[test]
    fn test_メモuiが描画される() {
        let memo = MemoResponse::from_raw("> 引用メモ".to_string(), Some("README.md".to_string()));
        let html = render_single_file_page(&memo);

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
        assert!(
            html.contains("<blockquote") && html.contains("</blockquote>"),
            "raw=\"> 引用メモ\" から render_markdown 経由で <blockquote> 要素が描画されること"
        );
    }

    #[test]
    fn test_メモfileがnoneの場合data_memo_file属性を出力しない() {
        let memo = MemoResponse::empty(None);
        let html = render_single_file_page(&memo);

        assert!(
            !html.contains("data-memo-file"),
            "file=None は空属性ではなく属性なしとして表現すること"
        );
        assert!(html.contains("id=\"memo-editor\""));
        assert!(html.contains("id=\"memo-preview\""));
    }

    #[test]
    fn test_メモ読み込み失敗時はエラー表示して編集を無効化する() {
        let memo = MemoResponse::empty_with_load_error(
            Some("README.md".to_string()),
            "/tmp/private/path/README.md: permission denied",
        );
        let html = render_single_file_page(&memo);

        assert!(html.contains("id=\"memo-degraded-banner\""));
        assert!(html.contains("role=\"status\""));
        assert!(html.contains("内容を保護するため編集を無効化"));
        assert!(!html.contains("/tmp/private/path"));
        assert!(html.contains("data-state=\"error\""));
        assert!(html.contains(">読込失敗</span>"));
        assert!(html.contains("id=\"memo-editor\""));
        assert!(html.contains("disabled aria-disabled=\"true\""));
        assert!(html.contains("MEMO_DEGRADED_MESSAGE"));
        assert!(html.contains("setMemoEditorDisabled(true)"));
    }

    #[test]
    fn test_通常メモではdegradedバナーを表示しない() {
        let memo = MemoResponse::from_raw("通常メモ".to_string(), Some("README.md".to_string()));
        let html = render_single_file_page(&memo);

        assert!(!html.contains("id=\"memo-degraded-banner\""));
        assert!(html.contains("data-state=\"saved\""));
        assert!(html.contains(">保存済み</span>"));
        assert!(!html.contains("disabled aria-disabled=\"true\""));
    }
}
