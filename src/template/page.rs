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

/// HTMLテンプレートを生成する
///
/// CSS/JSをすべて埋め込み、外部ファイル不要で動作する
pub fn render_page(params: RenderPageParams<'_>) -> String {
    let escaped_title = html_escape(params.title);
    let (dir_mode_attr, sidebar_inner, meta) =
        render_sidebar(&params.sidebar, params.toc, params.memo);
    let memo_file_attr = params.memo.file().unwrap_or_default();

    format!(
        r##"<!DOCTYPE html>
<html lang="ja" data-theme="{theme}"{dir_mode_attr} data-memo-file="{memo_file}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - markdown-view</title>
<style>{css}</style>
</head>
<body>
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
<main id="content" class="content" data-title="{title}">
{content}
</main>
</div>
</div>
<button id="back-to-top" class="back-to-top" aria-label="ページ上部へ戻る">↑</button>
<button id="quote-selection-action" class="quote-selection-action" type="button" hidden>引用を追加</button>
<script>{js}</script>
</body>
</html>"##,
        theme = if params.dark_mode { "dark" } else { "light" },
        dir_mode_attr = dir_mode_attr,
        memo_file = html_escape(memo_file_attr),
        title = escaped_title,
        css = combined_css(params.syntax_css),
        sidebar_inner = sidebar_inner,
        content = params.content.as_str(),
        js = inline_js(),
        mode_label = meta.mode_label,
        file_count_label = meta.file_count_label,
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
                " data-dir-mode=\"true\" data-current-file=\"{}\"",
                html_escape(current_file.unwrap_or(""))
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
