use super::assets::{combined_css, inline_js};
use super::tree::{build_file_tree, render_file_tree_html};
use crate::renderer::{html_escape, SanitizedHtml};

/// サイドバー描画パラメータ
pub enum SidebarParams<'a> {
    /// 単一ファイルモード（ブランド・目次検索・目次を表示）
    SingleFile,
    /// ディレクトリモード（ファイル一覧 + 目次）
    Directory {
        file_list: &'a [String],
        current_file: Option<&'a str>,
    },
}

/// HTMLテンプレートのパラメータ
pub struct RenderPageParams<'a> {
    pub title: &'a str,
    pub content: &'a SanitizedHtml,
    pub toc: &'a SanitizedHtml,
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
    let (dir_mode_attr, sidebar_inner, meta) = render_sidebar(&params.sidebar, params.toc);

    format!(
        r##"<!DOCTYPE html>
<html lang="ja" data-theme="{theme}"{dir_mode_attr}>
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
<script>{js}</script>
</body>
</html>"##,
        theme = if params.dark_mode { "dark" } else { "light" },
        dir_mode_attr = dir_mode_attr,
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
) -> (String, String, DocumentMeta) {
    match sidebar {
        SidebarParams::Directory {
            file_list,
            current_file,
        } => {
            let tree = build_file_tree(file_list);
            let tree_html = render_file_tree_html(&tree, *current_file);
            let dir_mode_attr = format!(
                " data-dir-mode=\"true\" data-current-file=\"{}\"",
                html_escape(current_file.unwrap_or(""))
            );
            let sidebar_inner = format!(
                r##"  <div class="sidebar-brand">
    <p class="sidebar-kicker">Workspace</p>
    <h2>Documents</h2>
    <p class="sidebar-caption">ディレクトリ内のMarkdownを切り替えて閲覧できます。</p>
  </div>
  <div class="sidebar-utility">
    <label class="sidebar-search">
      <span>絞り込み</span>
      <input id="file-filter" type="search" placeholder="ファイル名で検索" autocomplete="off">
    </label>
    <p id="file-filter-summary" class="sidebar-summary">{file_count} files</p>
  </div>
  <div class="sidebar-tabs">
    <button class="sidebar-tab active" data-tab="files">ファイル</button>
    <button class="sidebar-tab" data-tab="toc">目次</button>
    <button id="sidebar-toggle" class="sidebar-toggle" aria-label="閉じる">×</button>
  </div>
  <div class="sidebar-panel active" id="panel-files">
    <div class="file-list">
{tree_html}    </div>
  </div>
  <div class="sidebar-panel" id="panel-toc">
    <label class="sidebar-search sidebar-search-compact">
      <span>目次検索</span>
      <input id="toc-filter" type="search" placeholder="見出しを検索" autocomplete="off">
    </label>
    <nav id="toc">{toc}</nav>
  </div>"##,
                tree_html = tree_html,
                toc = toc.as_str(),
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
        SidebarParams::SingleFile => (
            String::new(),
            format!(
                r##"  <div class="sidebar-brand">
    <p class="sidebar-kicker">Workspace</p>
    <h2>Outline</h2>
    <p class="sidebar-caption">このドキュメントの見出しを追跡します。</p>
  </div>
  <div class="sidebar-header">
    <h2>目次</h2>
    <button id="sidebar-toggle" class="sidebar-toggle" aria-label="目次を閉じる">×</button>
  </div>
  <label class="sidebar-search sidebar-search-compact">
    <span>目次検索</span>
    <input id="toc-filter" type="search" placeholder="見出しを検索" autocomplete="off">
  </label>
  <nav id="toc">{toc}</nav>"##,
                toc = toc.as_str(),
            ),
            DocumentMeta {
                mode_label: "Single file".to_string(),
                file_count_label: "1 file".to_string(),
            },
        ),
    }
}
