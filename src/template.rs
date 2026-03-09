use std::sync::OnceLock;

use crate::renderer::{html_escape, SanitizedHtml};
use crate::server::MAX_FILE_SIZE;

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

/// HTMLテンプレートを生成する
///
/// CSS/JSをすべて埋め込み、外部ファイル不要で動作する
pub fn render_page(params: RenderPageParams<'_>) -> String {
    let escaped_title = html_escape(params.title);

    let (dir_mode_attr, sidebar_inner, mode_label, file_count_label) = match params.sidebar {
        SidebarParams::Directory {
            file_list,
            current_file,
        } => {
            let tree = build_file_tree(file_list);
            let tree_html = render_file_tree_html(&tree, current_file);
            let attr = format!(
                " data-dir-mode=\"true\" data-current-file=\"{}\"",
                html_escape(current_file.unwrap_or(""))
            );
            let html = format!(
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
                toc = params.toc.as_str(),
                file_count = file_list.len(),
            );
            (
                attr,
                html,
                "Directory".to_string(),
                format!("{} files", file_list.len()),
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
                toc = params.toc.as_str(),
            ),
            "Single file".to_string(),
            "1 file".to_string(),
        ),
    };

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
    <button id="sidebar-open" class="sidebar-open" aria-label="サイドバーを開く">☰</button>
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
        mode_label = mode_label,
        file_count_label = file_count_label,
    )
}

/// コンテンツ更新用JSONメッセージ構造体（HTTP API・WebSocket共用）
#[derive(serde::Serialize, Debug, Clone)]
pub struct UpdateMessage {
    content: SanitizedHtml,
    toc: SanitizedHtml,
    /// ディレクトリモード時の変更ファイル相対パス（単一ファイルモードはNone）
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
}

impl UpdateMessage {
    /// 更新メッセージを生成する
    pub fn new(content: SanitizedHtml, toc: SanitizedHtml, file: Option<String>) -> Self {
        Self { content, toc, file }
    }

    /// コンテンツHTMLを返す
    pub fn content(&self) -> &SanitizedHtml {
        &self.content
    }

    /// TOC HTMLを返す
    pub fn toc(&self) -> &SanitizedHtml {
        &self.toc
    }

    /// ディレクトリモード時の変更ファイル相対パスを返す
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// fileフィールドを置き換えた新しいメッセージを返す
    pub fn with_file(mut self, file: Option<String>) -> Self {
        self.file = file;
        self
    }
}

/// エラーJSONを生成する
pub fn error_message_json(message: impl AsRef<str>) -> serde_json::Value {
    serde_json::json!({ "error": message.as_ref() })
}

const DARK_THEME_VARS: &str = r##"
  --bg: #1a1b26;
  --bg-accent: radial-gradient(circle at top, rgba(122, 162, 247, 0.10), transparent 32%), radial-gradient(circle at 80% 20%, rgba(187, 154, 247, 0.08), transparent 24%), linear-gradient(180deg, #1e2030 0%, #16161e 100%);
  --fg: #c0caf5;
  --muted: #565f89;
  --sidebar-bg: rgba(22, 22, 30, 0.88);
  --sidebar-border: rgba(61, 66, 104, 0.35);
  --panel-bg: rgba(26, 27, 38, 0.78);
  --panel-border: rgba(61, 66, 104, 0.30);
  --panel-shadow: 0 24px 80px rgba(0, 0, 0, 0.40);
  --link: #7aa2f7;
  --code-bg: #16161e;
  --blockquote-border: rgba(122, 162, 247, 0.40);
  --blockquote-fg: #9aa5ce;
  --table-border: rgba(61, 66, 104, 0.40);
  --table-alt-bg: rgba(255, 255, 255, 0.02);
  --hr-color: rgba(61, 66, 104, 0.40);
  --toc-active: #bb9af7;
  --toc-hover-bg: rgba(122, 162, 247, 0.08);
  --pill-bg: rgba(61, 66, 104, 0.20);
  --pill-strong-bg: rgba(187, 154, 247, 0.16);
  --accent: #bb9af7;
  --accent-soft: rgba(187, 154, 247, 0.14);
  --blockquote-bg: rgba(255, 255, 255, 0.04);
  --sidebar-utility-bg: rgba(61, 66, 104, 0.15);
  --search-input-bg: rgba(22, 22, 30, 0.50);
  --topbar-btn-bg: rgba(61, 66, 104, 0.20);
  --content-bg: rgba(22, 22, 30, 0.80);
  --code-copy-bg: rgba(22, 22, 30, 0.50);
"##;

const CSS_TEMPLATE: &str = r##"
/* リセットと基本設定 */
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

:root {
  --bg: #f2f2f2;
  --bg-accent: linear-gradient(180deg, #f6f6f6 0%, #eaeaea 100%);
  --fg: #1a1a1a;
  --muted: #737373;
  --sidebar-bg: rgba(245, 245, 245, 0.88);
  --sidebar-border: rgba(0, 0, 0, 0.10);
  --panel-bg: rgba(250, 250, 250, 0.80);
  --panel-border: rgba(0, 0, 0, 0.08);
  --panel-shadow: 0 28px 80px rgba(0, 0, 0, 0.06);
  --link: #3d3d3d;
  --code-bg: #e8e8e8;
  --blockquote-border: rgba(0, 0, 0, 0.22);
  --blockquote-fg: #525252;
  --table-border: rgba(0, 0, 0, 0.10);
  --table-alt-bg: rgba(0, 0, 0, 0.03);
  --hr-color: rgba(0, 0, 0, 0.10);
  --toc-active: #1a1a1a;
  --toc-hover-bg: rgba(0, 0, 0, 0.05);
  --pill-bg: rgba(255, 255, 255, 0.60);
  --pill-strong-bg: rgba(0, 0, 0, 0.07);
  --accent: #1a1a1a;
  --accent-soft: rgba(0, 0, 0, 0.06);
  --blockquote-bg: rgba(0, 0, 0, 0.03);
  --sidebar-utility-bg: rgba(255, 255, 255, 0.18);
  --search-input-bg: rgba(255, 255, 255, 0.65);
  --topbar-btn-bg: rgba(255, 255, 255, 0.4);
  --content-bg: rgba(255, 255, 255, 0.8);
  --code-copy-bg: rgba(255, 255, 255, 0.72);
}

[data-theme="dark"] {
__DARK_THEME_VARS__
}

@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
__DARK_THEME_VARS__
  }
}

body {
  font-family: "IBM Plex Sans JP", "Hiragino Sans", "Yu Gothic", sans-serif;
  background: var(--bg-accent);
  color: var(--fg);
  line-height: 1.7;
  min-height: 100vh;
  position: relative;
}

body::before {
  content: "";
  position: fixed;
  inset: 0;
  pointer-events: none;
  background-image: linear-gradient(rgba(255,255,255,0.08) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.05) 1px, transparent 1px);
  background-size: 24px 24px;
  opacity: 0.2;
  mask-image: linear-gradient(180deg, rgba(0, 0, 0, 0.5), transparent 92%);
}

.app-shell {
  display: flex;
  width: 100%;
}

.workspace {
  flex: 1;
  min-width: 0;
  padding: 1.5rem 1.5rem 4rem;
}

.topbar {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;
  margin: 0 auto 1rem;
  max-width: 1040px;
  padding: 1.35rem 1.5rem 1.1rem;
  border: 1px solid var(--panel-border);
  background: var(--panel-bg);
  backdrop-filter: blur(18px);
  border-radius: 22px;
  box-shadow: var(--panel-shadow);
}

.topbar-kicker,
.sidebar-kicker {
  font-size: 0.72rem;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: var(--muted);
  margin-bottom: 0.35rem;
}

.document-title,
.sidebar-brand h2 {
  font-family: "Iowan Old Style", "Palatino Linotype", "Yu Mincho", serif;
  font-weight: 700;
  line-height: 1.08;
}

.document-title {
  font-size: clamp(1.9rem, 3vw, 3.2rem);
  margin-bottom: 0.8rem;
  overflow-wrap: anywhere;
}

.document-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 0.55rem;
}

.meta-pill {
  display: inline-flex;
  align-items: center;
  min-height: 2rem;
  padding: 0.3rem 0.75rem;
  border: 1px solid var(--panel-border);
  border-radius: 999px;
  background: var(--pill-bg);
  font-size: 0.82rem;
  color: var(--muted);
}

.meta-pill-strong {
  color: var(--fg);
  background: var(--pill-strong-bg);
}

.live-pill {
  color: var(--accent);
}

.live-pill::before {
  content: "";
  width: 0.55rem;
  height: 0.55rem;
  margin-right: 0.45rem;
  border-radius: 999px;
  background: currentColor;
  box-shadow: 0 0 0 0.28rem var(--accent-soft);
}

.reading-progress {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 3px;
  z-index: 1000;
  overflow: hidden;
  background: transparent;
}

.reading-progress-bar {
  height: 100%;
  width: 0;
  background: linear-gradient(90deg, var(--accent), var(--link));
  transition: width 0.18s ease;
}

.live-pill[data-state="retry"],
.live-pill[data-state="offline"],
.live-pill[data-state="error"] {
  color: #f7768e;
}

/* サイドバー */
.sidebar {
  width: 320px;
  min-width: 320px;
  background: var(--sidebar-bg);
  border-right: 1px solid var(--sidebar-border);
  padding: 1.2rem 1rem;
  overflow-y: auto;
  position: sticky;
  top: 0;
  height: 100vh;
  transition: transform 0.3s ease, box-shadow 0.2s ease;
  display: flex;
  flex-direction: column;
  gap: 0.85rem;
  backdrop-filter: blur(14px);
}

.sidebar-brand {
  padding: 0.35rem 0.25rem 0;
}

.sidebar-brand h2 {
  font-size: 1.9rem;
  margin-bottom: 0.35rem;
}

.sidebar-caption {
  color: var(--muted);
  font-size: 0.88rem;
}

.sidebar-utility {
  display: grid;
  gap: 0.6rem;
  padding: 0.8rem;
  border: 1px solid var(--panel-border);
  border-radius: 18px;
  background: var(--sidebar-utility-bg);
}

.sidebar-search {
  display: grid;
  gap: 0.35rem;
  font-size: 0.78rem;
  color: var(--muted);
}

.sidebar-search-compact {
  margin: 0.25rem 0 0.75rem;
}

.sidebar-search input {
  width: 100%;
  border: 1px solid var(--sidebar-border);
  border-radius: 12px;
  background: var(--search-input-bg);
  padding: 0.7rem 0.8rem;
  color: var(--fg);
}

.sidebar-summary {
  font-size: 0.78rem;
  color: var(--muted);
}

.sidebar-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.25rem 0.1rem 0;
}

.sidebar-header h2 {
  font-size: 0.875rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--muted);
}

.sidebar-toggle {
  display: none;
  background: none;
  border: none;
  font-size: 1.25rem;
  cursor: pointer;
  color: var(--fg);
}

.topbar-actions {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-shrink: 0;
}

.topbar-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 2.8rem;
  min-height: 2.8rem;
  background: var(--topbar-btn-bg);
  border: 1px solid var(--sidebar-border);
  border-radius: 14px;
  padding: 0.25rem;
  cursor: pointer;
  font-size: 1.1rem;
  color: var(--fg);
  transition: background 0.15s ease;
}

.topbar-btn:hover {
  background: var(--toc-hover-bg);
}

/* テーマアイコン切替 */
.theme-icon { display: none; }
[data-theme="light"] .theme-icon-light,
:root:not([data-theme]) .theme-icon-light { display: inline; }
[data-theme="dark"] .theme-icon-dark { display: inline; }

@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) .theme-icon-light { display: none; }
  :root:not([data-theme="light"]) .theme-icon-dark { display: inline; }
}

.sidebar-open {
  display: none;
  align-items: center;
  justify-content: center;
  min-width: 2.8rem;
  min-height: 2.8rem;
  background: var(--topbar-btn-bg);
  border: 1px solid var(--sidebar-border);
  border-radius: 14px;
  padding: 0.25rem 0.65rem;
  cursor: pointer;
  font-size: 1.25rem;
  color: var(--fg);
}

#toc ul {
  list-style: none;
  padding-left: 0;
}

#toc ul ul {
  padding-left: 1rem;
}

#toc li { margin: 0.125rem 0; }

#toc a {
  display: block;
  padding: 0.42rem 0.55rem;
  border-radius: 10px;
  color: var(--fg);
  text-decoration: none;
  font-size: 0.875rem;
  transition: background 0.15s, transform 0.15s;
}

#toc a:hover {
  background: var(--toc-hover-bg);
  transform: translateX(2px);
}

#toc a.active {
  color: var(--toc-active);
  font-weight: 600;
  background: var(--accent-soft);
}

/* メインコンテンツ */
.content {
  max-width: 1040px;
  margin: 0 auto;
  padding: 2.25rem clamp(1.2rem, 4vw, 3.5rem) 3rem;
  border: 1px solid var(--panel-border);
  border-radius: 28px;
  background: var(--content-bg);
  box-shadow: var(--panel-shadow);
  backdrop-filter: blur(18px);
}

.content h1, .content h2, .content h3, .content h4, .content h5, .content h6 {
  margin-top: 1.5em;
  margin-bottom: 0.5em;
  font-weight: 600;
  line-height: 1.25;
  scroll-margin-top: 7rem;
}

.content h1, .content h2 {
  font-family: "Iowan Old Style", "Palatino Linotype", "Yu Mincho", serif;
}

.content .heading-anchor {
  margin-left: 0.5rem;
  padding: 0.12rem 0.45rem;
  border: 1px solid var(--table-border);
  border-radius: 999px;
  background: transparent;
  color: var(--muted);
  font-size: 0.72rem;
  vertical-align: middle;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s ease, color 0.15s ease, background 0.15s ease;
}

.content h1:hover .heading-anchor,
.content h2:hover .heading-anchor,
.content h3:hover .heading-anchor,
.content h4:hover .heading-anchor,
.content h5:hover .heading-anchor,
.content h6:hover .heading-anchor,
.content .heading-anchor:focus {
  opacity: 1;
}

.content .heading-anchor:hover,
.content .heading-anchor.copied {
  color: var(--fg);
  background: var(--accent-soft);
}

.content h1 { font-size: 2.4em; padding-bottom: 0.3em; border-bottom: 1px solid var(--hr-color); }
.content h2 { font-size: 1.75em; padding-bottom: 0.3em; border-bottom: 1px solid var(--hr-color); }
.content h3 { font-size: 1.25em; }

.content p { margin-bottom: 1em; }

.content a { color: var(--link); text-decoration: none; }
.content a:hover { text-decoration: underline; }

.content code {
  background: var(--code-bg);
  padding: 0.2em 0.4em;
  border-radius: 3px;
  font-size: 85%;
  font-family: 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace;
}

.content pre.code-block {
  position: relative;
  background: var(--code-bg);
  padding: 1rem 1.1rem;
  border-radius: 16px;
  overflow-x: auto;
  margin-bottom: 1em;
  line-height: 1.45;
  border: 1px solid var(--table-border);
}

.content .code-copy {
  position: absolute;
  top: 0.7rem;
  right: 0.7rem;
  border: 1px solid var(--table-border);
  border-radius: 999px;
  background: var(--code-copy-bg);
  color: var(--fg);
  padding: 0.28rem 0.7rem;
  font-size: 0.74rem;
  cursor: pointer;
}

.content .code-copy.copied {
  background: var(--accent-soft);
  color: var(--accent);
}

.content pre.code-block code {
  background: none;
  padding: 0;
  font-size: 85%;
}

.content blockquote {
  border-left: 4px solid var(--blockquote-border);
  padding: 0.75rem 1rem;
  margin-bottom: 1em;
  color: var(--blockquote-fg);
  background: var(--blockquote-bg);
  border-radius: 0 14px 14px 0;
}

.content table {
  border-collapse: collapse;
  width: 100%;
  margin-bottom: 1em;
}

.content th, .content td {
  border: 1px solid var(--table-border);
  padding: 0.5rem 0.75rem;
  text-align: left;
}
.content th.align-left, .content td.align-left { text-align: left; }
.content th.align-center, .content td.align-center { text-align: center; }
.content th.align-right, .content td.align-right { text-align: right; }

.content th { font-weight: 600; background: var(--table-alt-bg); }
.content tr:nth-child(even) { background: var(--table-alt-bg); }

.content hr {
  border: none;
  height: 1px;
  background: var(--hr-color);
  margin: 1.5em 0;
}

.content img { max-width: 100%; height: auto; border-radius: 4px; }

.content ul, .content ol { padding-left: 2em; margin-bottom: 1em; }
.content li { margin: 0.25em 0; }
.content li input[type="checkbox"] { margin-right: 0.5em; }

/* タブバー */
.sidebar-tabs {
  display: flex;
  align-items: center;
  border-bottom: 1px solid var(--sidebar-border);
  margin-top: 0.15rem;
  flex-shrink: 0;
}

.sidebar-tab {
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  padding: 0.5rem 0.75rem;
  font-size: 0.8125rem;
  font-weight: 500;
  color: var(--blockquote-fg);
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}

.sidebar-tab:hover { color: var(--fg); }
.sidebar-tab.active {
  color: var(--toc-active);
  border-bottom-color: var(--toc-active);
  font-weight: 600;
}

.sidebar-tabs .sidebar-toggle { margin-left: auto; }

/* タブパネル */
.sidebar-panel { display: none; }
.sidebar-panel.active {
  display: block;
  flex: 1;
  overflow-y: auto;
  min-height: 0;
  padding: 0.25rem 0 1rem;
}

/* ファイル一覧 */
.file-list {
  padding: 0;
}

.file-list ul {
  list-style: none;
  padding-left: 0;
}

.file-tree-root {
  list-style: none;
  padding-left: 0;
  margin: 0;
}

.file-list a {
  display: block;
  padding: 0.42rem 0.55rem;
  border-radius: 10px;
  color: var(--fg);
  text-decoration: none;
  font-size: 0.8125rem;
  transition: background 0.15s, transform 0.15s;
  word-break: break-all;
}

.file-list a:hover {
  background: var(--toc-hover-bg);
  transform: translateX(2px);
}

.file-tree-file.active a {
  color: var(--toc-active);
  font-weight: 600;
  background: var(--accent-soft);
}

/* ファイルツリー */
.file-tree-dir > summary {
  cursor: pointer;
  font-size: 0.8125rem;
  padding: 0.35rem 0.45rem;
  border-radius: 10px;
  list-style: none;
  color: var(--fg);
  font-weight: 500;
  display: flex;
  align-items: center;
  gap: 0.25rem;
}

.file-tree-dir > summary::-webkit-details-marker { display: none; }

.file-tree-dir > summary .tree-icon-chevron {
  display: inline-flex;
  width: 0.75rem;
  flex-shrink: 0;
  font-size: 0.625rem;
  transition: transform 0.15s;
}

.file-tree-dir[open] > summary .tree-icon-chevron { transform: rotate(90deg); }
.file-tree-dir > summary:hover { background: var(--toc-hover-bg); }

.file-tree-children {
  list-style: none;
  padding-left: 0.5rem;
  margin-left: 0.45rem;
  border-left: 1px solid var(--sidebar-border);
}

.file-tree-file {
  margin: 0;
  display: flex;
  align-items: center;
}

.file-tree-file a {
  display: flex;
  align-items: center;
  gap: 0.25rem;
  padding-left: 0.4rem;
}

.tree-icon {
  display: inline-flex;
  flex-shrink: 0;
  width: 1rem;
  font-size: 0.8125rem;
  justify-content: center;
}

/* エラーバナー */
.error-banner {
  position: fixed;
  top: 1rem;
  left: 50%;
  transform: translateX(-50%);
  width: min(640px, calc(100vw - 2rem));
  padding: 12px 16px;
  border-radius: 16px;
  background: #f7768e;
  color: #1a1b26;
  font-size: 14px;
  box-shadow: 0 20px 40px rgba(127, 29, 29, 0.28);
}

.error-banner.disconnect {
  z-index: 9999;
}

.error-banner.server,
.error-banner.fetch {
  z-index: 9998;
}

.error-banner-close {
  cursor: pointer;
  float: right;
  font-size: 18px;
  line-height: 1;
}

.back-to-top {
  position: fixed;
  right: 1.4rem;
  bottom: 1.4rem;
  width: 3rem;
  height: 3rem;
  border: 1px solid var(--sidebar-border);
  border-radius: 999px;
  background: var(--panel-bg);
  color: var(--fg);
  box-shadow: var(--panel-shadow);
  cursor: pointer;
  opacity: 0;
  pointer-events: none;
  transform: translateY(12px);
  transition: opacity 0.2s ease, transform 0.2s ease;
}

.back-to-top.visible {
  opacity: 1;
  pointer-events: auto;
  transform: translateY(0);
}

/* モバイル対応 */
@media (max-width: 768px) {
  .sidebar {
    position: fixed;
    left: 0;
    top: 0;
    z-index: 999;
    transform: translateX(-100%);
    width: min(88vw, 320px);
    min-width: min(88vw, 320px);
    box-shadow: 2px 0 8px rgba(0,0,0,0.15);
  }
  .sidebar.open { transform: translateX(0); }
  .sidebar-toggle { display: block; }
  .sidebar-tabs .sidebar-toggle { display: block; }
  .sidebar-open { display: inline-flex; }
  .workspace { padding: 1rem 0.85rem 3.5rem; }
  .topbar { padding: 1.1rem 1rem 0.95rem; border-radius: 18px; }
  .document-title { font-size: 1.7rem; }
  .content { padding: 1.45rem 1rem 2rem; border-radius: 20px; }
  .back-to-top { right: 0.85rem; bottom: 0.85rem; }
}
"##;

fn css() -> &'static str {
    static CSS: OnceLock<String> = OnceLock::new();
    CSS.get_or_init(|| CSS_TEMPLATE.replace("__DARK_THEME_VARS__", DARK_THEME_VARS))
}

/// ベースCSSと構文ハイライトCSSを結合する
pub fn combined_css(syntax_css: &str) -> String {
    if syntax_css.is_empty() {
        css().to_string()
    } else {
        format!("{}\n{}", css(), syntax_css)
    }
}

/// インラインCSS/JS用のCSPハッシュソースを返す
///
/// `combined_css` と `JS` のハッシュは毎回計算する（キャッシュなし）。
/// 戻り値の順序: `(script-srcハッシュ, style-srcハッシュ)`
pub fn csp_hash_sources(syntax_css: &str) -> (String, String) {
    let style_hash = sha256_base64(combined_css(syntax_css).as_bytes());
    let script_hash = sha256_base64(inline_js().as_bytes());
    (
        format!("'sha256-{}'", script_hash),
        format!("'sha256-{}'", style_hash),
    )
}

fn sha256_base64(input: &[u8]) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;

    let digest = sha2::Sha256::digest(input);
    base64::engine::general_purpose::STANDARD.encode(digest)
}

/// ファイルツリーのノード（不正状態を表現しないenum）
#[derive(Debug, Clone, PartialEq)]
pub enum FileTreeNode {
    /// ファイルノード
    File { name: String, full_path: String },
    /// ディレクトリノード
    Directory {
        name: String,
        children: Vec<FileTreeNode>,
    },
}

impl FileTreeNode {
    /// ノード名を返す
    pub fn name(&self) -> &str {
        match self {
            FileTreeNode::File { name, .. } => name,
            FileTreeNode::Directory { name, .. } => name,
        }
    }
}

/// フラットなファイルパスのリストからツリー構造を構築する
///
/// 各階層内でディレクトリが先、ファイルが後（それぞれアルファベット順）
pub fn build_file_tree(files: &[String]) -> Vec<FileTreeNode> {
    // 中間表現: 各ディレクトリを子マップで表現
    struct DirNode {
        children_dirs: std::collections::BTreeMap<String, DirNode>,
        files: Vec<(String, String)>, // (ファイル名, フルパス)
    }

    impl DirNode {
        fn new() -> Self {
            Self {
                children_dirs: std::collections::BTreeMap::new(),
                files: Vec::new(),
            }
        }

        /// パスコンポーネントを辿ってファイルを挿入
        fn insert(&mut self, parts: &[&str], full_path: &str) {
            match parts.len() {
                0 => {}
                1 => {
                    // リーフ（ファイル）
                    self.files
                        .push((parts[0].to_string(), full_path.to_string()));
                }
                _ => {
                    // ディレクトリを辿る
                    let dir = self
                        .children_dirs
                        .entry(parts[0].to_string())
                        .or_insert_with(DirNode::new);
                    dir.insert(&parts[1..], full_path);
                }
            }
        }

        /// FileTreeNodeのリストに変換（ディレクトリ先、ファイル後、各アルファベット順）
        fn into_tree_nodes(self) -> Vec<FileTreeNode> {
            let mut result = Vec::new();

            // ディレクトリ（BTreeMapなのでアルファベット順）
            for (name, child) in self.children_dirs {
                result.push(FileTreeNode::Directory {
                    name,
                    children: child.into_tree_nodes(),
                });
            }

            // ファイル（アルファベット順にソート）
            let mut files = self.files;
            files.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, full_path) in files {
                result.push(FileTreeNode::File { name, full_path });
            }

            result
        }
    }

    let mut root = DirNode::new();
    let mut seen_paths = std::collections::HashSet::new();
    for file in files {
        let parts: Vec<&str> = file.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            continue;
        }
        let normalized_path = parts.join("/");
        if !seen_paths.insert(normalized_path.clone()) {
            continue;
        }
        root.insert(&parts, &normalized_path);
    }
    root.into_tree_nodes()
}

/// ファイルツリーのHTML表現を生成する
///
/// - `current_file`: 現在表示中のファイルパス（祖先ディレクトリをopen状態にする）
pub fn render_file_tree_html(nodes: &[FileTreeNode], current_file: Option<&str>) -> String {
    // current_fileは防御的に正規化して扱う（連続スラッシュ等を吸収）
    let normalized_current_file = current_file.and_then(|path| {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("/"))
        }
    });

    // current_fileの祖先ディレクトリ名セットを構築
    let active_dirs: std::collections::HashSet<String> = normalized_current_file
        .as_deref()
        .map(|path| {
            let mut dirs = std::collections::HashSet::new();
            let mut accumulated = String::new();
            let parts: Vec<&str> = path.split('/').collect();
            // 最後の要素（ファイル名）を除くディレクトリパスを蓄積
            for part in &parts[..parts.len().saturating_sub(1)] {
                if !accumulated.is_empty() {
                    accumulated.push('/');
                }
                accumulated.push_str(part);
                dirs.insert(accumulated.clone());
            }
            dirs
        })
        .unwrap_or_default();

    fn render_nodes(
        nodes: &[FileTreeNode],
        html: &mut String,
        current_file: Option<&str>,
        active_dirs: &std::collections::HashSet<String>,
        current_path: &str,
    ) {
        for node in nodes {
            let escaped_name = html_escape(node.name());
            match node {
                FileTreeNode::File { full_path, .. } => {
                    let is_active = current_file == Some(full_path.as_str());
                    let class = if is_active {
                        "file-tree-file active"
                    } else {
                        "file-tree-file"
                    };
                    let escaped_path = html_escape(full_path);
                    html.push_str(&format!(
                        "<li class=\"{class}\"><a href=\"#\" data-file=\"{path}\"><span class=\"tree-icon\">📄</span>{name}</a></li>\n",
                        class = class,
                        path = escaped_path,
                        name = escaped_name,
                    ));
                }
                FileTreeNode::Directory { name, children } => {
                    let dir_path = if current_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", current_path, name)
                    };
                    let is_open = active_dirs.contains(&dir_path);
                    let open_attr = if is_open { " open" } else { "" };
                    html.push_str(&format!(
                        "<li>\n<details class=\"file-tree-dir\"{open}>\n<summary><span class=\"tree-icon-chevron\">▶</span><span class=\"tree-icon\">📁</span>{name}</summary>\n<ul class=\"file-tree-children\">\n",
                        open = open_attr,
                        name = escaped_name,
                    ));
                    render_nodes(children, html, current_file, active_dirs, &dir_path);
                    html.push_str("</ul>\n</details>\n</li>\n");
                }
            }
        }
    }

    let mut html = String::new();
    html.push_str("<ul class=\"file-tree-root\">\n");
    render_nodes(
        nodes,
        &mut html,
        normalized_current_file.as_deref(),
        &active_dirs,
        "",
    );
    html.push_str("</ul>\n");
    html
}

fn inline_js() -> String {
    JS.replace(
        "__MAX_FILE_SIZE_MB__",
        &(MAX_FILE_SIZE / 1024 / 1024).to_string(),
    )
}

// セキュリティ注記:
// innerHTML使用箇所: updateContent()内でサニタイズ済みHTMLのみを反映。
// エスケープ経路: renderer.rs(render_markdown)でraw HTML除去
//   -> server.rs(read_and_render_file)でテンプレートへ受け渡し
//   -> template.rs(updateContent)で反映。
// XSS防止: pulldown-cmarkのEvent::Html/Event::InlineHtmlを除去し、
// raw HTMLが出力に含まれないようにしている（renderer.rs）。
// DNS Rebinding防止: 127.0.0.1バインド + Host/Originヘッダー検証（server.rs）。
const JS: &str = r##"
(function() {
  'use strict';
  var MAX_FILE_SIZE_MB = __MAX_FILE_SIZE_MB__;

  // ディレクトリモード判定
  var htmlEl = document.documentElement;
  var isDirMode = htmlEl.getAttribute('data-dir-mode') === 'true';
  var currentFile = htmlEl.getAttribute('data-current-file') || '';
  var documentTitleEl = document.getElementById('document-title');
  var docHeadingCountEl = document.getElementById('doc-heading-count');
  var docCharCountEl = document.getElementById('doc-char-count');
  var liveStatusEl = document.getElementById('live-status');
  var readingProgressBar = document.getElementById('reading-progress-bar');
  var backToTop = document.getElementById('back-to-top');
  var contentRoot = document.getElementById('content');

  // テキスト選択中のDOM更新延期機構
  // マウスドラッグ中にWebSocket経由のinnerHTML更新が走ると選択が破壊されるため、
  // 選択操作中は更新を保留し、選択解除（selectionchange + isCollapsed）後に適用する。
  // 選択が長時間維持される場合は30秒タイムアウトでフォールバック適用する。
  var pendingUpdate = null;
  var pendingUpdateTimer = null;
  var isMouseSelecting = false;

  document.addEventListener('mousedown', function(e) {
    // コンテンツ領域でのマウスダウンを追跡
    var contentEl = document.getElementById('content');
    if (contentEl && contentEl.contains(e.target)) {
      isMouseSelecting = true;
    }
  });

  document.addEventListener('mouseup', function() {
    if (!isMouseSelecting) return;
    isMouseSelecting = false;
    // mouseup後もテキストが選択状態（ハイライト表示）のままなのでDOMを更新しない。
    // selectionchangeで選択が解除された（isCollapsed）時点で適用する。
  });

  // テキスト選択が完全に解除された時に保留更新を適用
  // ドラッグ中もselectionchangeが頻発するため、isMouseSelectingで除外する
  document.addEventListener('selectionchange', function() {
    if (isMouseSelecting) return;
    var sel = window.getSelection();
    if (sel && sel.isCollapsed && pendingUpdate) {
      applyPendingUpdate();
    }
  });

  // テキスト選択中かを判定（ドラッグ操作中 or 選択範囲が存在）
  // mousedown直後はgetSelection()がまだ更新されない場合があるため、
  // isMouseSelectingフラグで補完する
  function isTextSelected() {
    if (isMouseSelecting) return true;
    var sel = window.getSelection();
    return sel && !sel.isCollapsed;
  }

  function ensurePendingUpdateTimer() {
    if (!pendingUpdateTimer) {
      pendingUpdateTimer = setTimeout(function() {
        pendingUpdateTimer = null;
        applyPendingUpdate();
      }, 30000);
    }
  }

  function setLiveStatus(state, label) {
    if (!liveStatusEl) return;
    liveStatusEl.textContent = label;
    liveStatusEl.dataset.state = state;
  }

  function updateDocumentStats() {
    if (!contentRoot) return;
    var headings = contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6').length;
    var text = (contentRoot.textContent || '').replace(/\s+/g, '');
    if (docHeadingCountEl) {
      docHeadingCountEl.textContent = '見出し ' + headings;
    }
    if (docCharCountEl) {
      docCharCountEl.textContent = '文字 ' + text.length;
    }
  }

  function updateReadingProgress() {
    var scrollTop = window.scrollY || window.pageYOffset;
    var maxScroll = Math.max(document.documentElement.scrollHeight - window.innerHeight, 1);
    var progress = Math.min(100, Math.max(0, (scrollTop / maxScroll) * 100));
    if (readingProgressBar) {
      readingProgressBar.style.width = progress + '%';
    }
    if (backToTop) {
      backToTop.classList.toggle('visible', scrollTop > 360);
    }
  }

  function syncDocumentChrome(file) {
    var title = file ? file.split('/').pop() : (contentRoot ? contentRoot.getAttribute('data-title') : '');
    if (!title) title = 'markdown-view';
    if (documentTitleEl) {
      documentTitleEl.textContent = title;
    }
    document.title = title + ' - markdown-view';
  }

  function copyText(text) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(text);
    }
    return new Promise(function(resolve, reject) {
      try {
        var input = document.createElement('textarea');
        input.value = text;
        input.setAttribute('readonly', 'readonly');
        input.style.position = 'fixed';
        input.style.opacity = '0';
        document.body.appendChild(input);
        input.select();
        var success = document.execCommand('copy');
        input.remove();
        if (success) {
          resolve();
        } else {
          reject(new Error('execCommand("copy") returned false'));
        }
      } catch (error) {
        reject(error);
      }
    });
  }

  function flashCopiedState(button, copiedLabel, baseLabel) {
    if (!button) return;
    button.classList.add('copied');
    button.textContent = copiedLabel;
    setTimeout(function() {
      button.classList.remove('copied');
      button.textContent = baseLabel;
    }, 1200);
  }

  function enhanceContentInteractions() {
    if (!contentRoot) return;

    var headings = contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6');
    headings.forEach(function(heading) {
      if (!heading.id || heading.querySelector('.heading-anchor')) return;
      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'heading-anchor';
      button.textContent = '#';
      button.setAttribute('aria-label', '見出しリンクをコピー');
      button.addEventListener('click', function() {
        var url = new URL(location.href);
        url.hash = heading.id;
        copyText(url.toString()).then(function() {
          flashCopiedState(button, 'Copied', '#');
        }).catch(function(err) {
          console.warn('[markdown-view] コピーに失敗:', err);
          flashCopiedState(button, 'Failed', '#');
        });
      });
      heading.appendChild(button);
    });

    var blocks = contentRoot.querySelectorAll('pre.code-block');
    blocks.forEach(function(block) {
      if (block.querySelector('.code-copy')) return;
      var code = block.querySelector('code');
      if (!code) return;
      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'code-copy';
      button.textContent = 'Copy';
      button.setAttribute('aria-label', 'コードをコピー');
      button.addEventListener('click', function() {
        copyText(code.innerText || code.textContent || '').then(function() {
          flashCopiedState(button, 'Copied', 'Copy');
        }).catch(function(err) {
          console.warn('[markdown-view] コピーに失敗:', err);
          flashCopiedState(button, 'Failed', 'Copy');
        });
      });
      block.appendChild(button);
    });
  }

  function setupTocFilter() {
    var input = document.getElementById('toc-filter');
    var toc = document.getElementById('toc');
    if (!input || !toc) return;

    var applyFilter = function() {
      var query = input.value.trim().toLowerCase();
      var items = toc.querySelectorAll('li');
      items.forEach(function(item) {
        var link = item.querySelector(':scope > a');
        if (!link) return;
        var matched = !query || link.textContent.toLowerCase().indexOf(query) !== -1;
        item.hidden = !matched;
      });
    };

    input.addEventListener('input', applyFilter);
    applyFilter();
  }

  function applyPendingUpdate() {
    if (!pendingUpdate) return;
    if (pendingUpdateTimer) {
      clearTimeout(pendingUpdateTimer);
      pendingUpdateTimer = null;
    }
    if (pendingUpdate.refresh) {
      var refreshFile = pendingUpdate.file;
      pendingUpdate = null;
      if (isDirMode && refreshFile) {
        selectFile(refreshFile, false);
      }
      return;
    }
    // ディレクトリモード: ファイル切り替え後は古い更新を破棄
    if (isDirMode && pendingUpdate.file && pendingUpdate.file !== currentFile) {
      pendingUpdate = null;
      return;
    }
    var data = pendingUpdate;
    pendingUpdate = null;
    updateContent(data);
    hideWsServerErrorBanner();
    hideFileFetchErrorBanner();
    setLiveStatus('live', 'Live');
  }

  // URLの?fileパラメータを取得
  function getFileParam() {
    var params = new URLSearchParams(location.search);
    return params.get('file') || '';
  }

  // URLの?fileパラメータを更新（ページ遷移なし）
  // replace=trueの場合はreplaceState（戻る/進む操作時やエラーロールバック時）
  function setFileParam(file, replace) {
    var url = new URL(location.href);
    if (file) {
      url.searchParams.set('file', file);
    } else {
      url.searchParams.delete('file');
    }
    if (replace) {
      history.replaceState(null, '', url.toString());
    } else {
      history.pushState(null, '', url.toString());
    }
  }

  // ディレクトリモード: 初期化時にURLパラメータをサーバーの正規化済み値に同期
  // data-current-fileはサーバーがcanonicalize済みの相対パスを設定するため、
  // URLクエリの生値（例: docs/../README.md）よりも信頼できる。
  // currentFileを常にサーバーの正規化値に保つことで、
  // WebSocket更新のdata.fileとの比較が正しく行われる。
  if (isDirMode) {
    if (currentFile) {
      // サーバーの正規化済みパスでURLを同期（初期化なのでreplaceState）
      setFileParam(currentFile, true);
    }
  }

  // WebSocket接続管理
  var WS_RECONNECT_BASE = 1000;
  var WS_RECONNECT_MAX_DELAY = 30000;
  var WS_RECONNECT_MAX_ATTEMPTS = 20;
  var ws = null;
  var reconnectAttempts = 0;

  function connectWS() {
    var protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    ws = new WebSocket(protocol + '//' + location.host + '/ws');

    ws.onopen = function() {
      reconnectAttempts = 0;
      setLiveStatus('live', 'Live');
    };

    ws.onmessage = function(event) {
      var data;
      try {
        data = JSON.parse(event.data);
      } catch (e) {
        console.error('[markdown-view] JSONパースエラー:', e);
        showWsParseErrorBanner('サーバーから不正なJSONを受信しました。ページを再読み込みしてください。');
        setLiveStatus('error', 'Invalid stream');
        return;
      }
      hideWsParseErrorBanner();
      if (data.error) {
        console.error('[markdown-view] サーバーエラー:', data.error);
        showWsServerErrorBanner(data.error);
        setLiveStatus('error', 'Server error');
        return;
      }
      // ディレクトリモード: サーバーからリフレッシュ要求時は現在ファイルを再取得
      if (data.refresh && isDirMode && currentFile) {
        if (isTextSelected()) {
          pendingUpdate = { refresh: true, file: currentFile };
          ensurePendingUpdateTimer();
          return;
        }
        selectFile(currentFile, false);
        return;
      }
      // ディレクトリモード: 自分の表示ファイルと一致する更新のみ適用
      if (isDirMode && data.file) {
        if (data.file !== currentFile) return;
      }
      // テキスト選択中はDOM更新を延期して選択破壊を防止
      // 複数回受信した場合は最新の更新のみ保持（最新状態が常に正しいため）
      if (isTextSelected()) {
        pendingUpdate = data;
        // 有効な更新を受信した時点でエラーバナーをクリア（DOM反映は延期）
        hideWsServerErrorBanner();
        hideFileFetchErrorBanner();
        // 30秒以上選択が維持される場合のフォールバックタイマー
        // mouseup後にWS受信した場合にもタイマーが確実に起動する
        ensurePendingUpdateTimer();
        return;
      }
      updateContent(data);
      // WebSocket経由の成功更新で各種エラーバナーをクリア
      hideWsServerErrorBanner();
      hideFileFetchErrorBanner();
      setLiveStatus('live', 'Live');
    };

    ws.onclose = function() {
      setLiveStatus('retry', 'Reconnecting');
      scheduleReconnect();
    };

    ws.onerror = function(event) {
      console.error('[markdown-view] WebSocketエラー:', event);
      setLiveStatus('error', 'Socket error');
      ws.close();
    };
  }

  function scheduleReconnect() {
    if (reconnectAttempts >= WS_RECONNECT_MAX_ATTEMPTS) {
      console.error('[markdown-view] 再接続上限に達しました。ページをリロードしてください');
      showDisconnectBanner();
      return;
    }
    var delay = Math.min(WS_RECONNECT_BASE * Math.pow(2, reconnectAttempts), WS_RECONNECT_MAX_DELAY);
    reconnectAttempts++;
    setTimeout(connectWS, delay);
  }

  function showDisconnectBanner() {
    if (document.getElementById('ws-disconnect-banner')) return;
    setLiveStatus('offline', 'Offline');
    var banner = document.createElement('div');
    banner.id = 'ws-disconnect-banner';
    banner.className = 'error-banner disconnect';
    banner.textContent = 'ライブリロード接続が切断されました。ページをリロードしてください。';
    document.body.appendChild(banner);
  }

  // WebSocketメッセージのJSONパース失敗をユーザーへ通知する
  function showWsParseErrorBanner(message) {
    if (document.getElementById('ws-disconnect-banner')) return;
    var banner = document.getElementById('ws-parse-error-banner');
    if (!banner) {
      banner = document.createElement('div');
      banner.id = 'ws-parse-error-banner';
      banner.className = 'error-banner server';
      var closeBtn = document.createElement('span');
      closeBtn.textContent = '\u00d7';
      closeBtn.className = 'error-banner-close';
      closeBtn.onclick = hideWsParseErrorBanner;
      banner.appendChild(closeBtn);
      var msg = document.createElement('span');
      msg.className = 'error-msg';
      banner.appendChild(msg);
      document.body.appendChild(banner);
    }
    banner.querySelector('.error-msg').textContent = message;
  }

  function hideWsParseErrorBanner() {
    var banner = document.getElementById('ws-parse-error-banner');
    if (banner) {
      banner.remove();
    }
  }

  // WebSocketからサーバーエラー通知を受信した時のバナーを表示する
  // WebSocket切断バナー表示中は表示しない（根本原因は接続断のため）
  function showWsServerErrorBanner(message) {
    if (document.getElementById('ws-disconnect-banner')) return;
    var banner = document.getElementById('ws-server-error-banner');
    if (!banner) {
      banner = document.createElement('div');
      banner.id = 'ws-server-error-banner';
      banner.className = 'error-banner server';
      var closeBtn = document.createElement('span');
      closeBtn.textContent = '\u00d7';
      closeBtn.className = 'error-banner-close';
      closeBtn.onclick = hideWsServerErrorBanner;
      banner.appendChild(closeBtn);
      var msg = document.createElement('span');
      msg.className = 'error-msg';
      banner.appendChild(msg);
      document.body.appendChild(banner);
    }
    banner.querySelector('.error-msg').textContent = message;
  }

  // 正常更新後または手動クローズ時にWebSocketサーバーエラーバナーを非表示にする
  function hideWsServerErrorBanner() {
    var banner = document.getElementById('ws-server-error-banner');
    if (banner) {
      banner.remove();
    }
  }

  // ファイルfetch失敗時のエラーバナーを表示する（既存バナーがあればメッセージを上書き）
  // WebSocket切断バナー表示中は表示しない（根本原因は接続断のため）
  function showFileFetchErrorBanner(message) {
    if (document.getElementById('ws-disconnect-banner')) return;
    var banner = document.getElementById('file-fetch-error-banner');
    if (!banner) {
      banner = document.createElement('div');
      banner.id = 'file-fetch-error-banner';
      banner.className = 'error-banner fetch';
      var closeBtn = document.createElement('span');
      closeBtn.textContent = '\u00d7';
      closeBtn.className = 'error-banner-close';
      closeBtn.onclick = hideFileFetchErrorBanner;
      banner.appendChild(closeBtn);
      var msg = document.createElement('span');
      msg.className = 'error-msg';
      banner.appendChild(msg);
      document.body.appendChild(banner);
    }
    banner.querySelector('.error-msg').textContent = message;
  }

  // ファイルfetch成功時にエラーバナーを非表示にする
  function hideFileFetchErrorBanner() {
    var banner = document.getElementById('file-fetch-error-banner');
    if (banner) {
      banner.remove();
    }
  }

  function createHttpError(status) {
    var err = new Error('HTTP ' + status);
    err.type = 'http';
    err.status = status;
    return err;
  }

  function getFileFetchErrorMessage(err) {
    if (err && err.type === 'http') {
      switch (err.status) {
        case 403:
          return 'このファイルにはアクセスできません。';
        case 404:
          return '指定したファイルが見つかりません。';
        case 413:
          return 'ファイルサイズが上限（' + MAX_FILE_SIZE_MB + 'MB）を超えています。';
        case 500:
          return 'サーバー内部エラーが発生しました。';
        default:
          return 'ファイルの読み込みに失敗しました（HTTP ' + err.status + '）。';
      }
    }
    if (err && err.type === 'parse') {
      return 'サーバー応答の解析に失敗しました。ページを再読み込みしてください。';
    }
    return 'ネットワークエラーが発生しました。接続を確認して再度お試しください。';
  }

  // サーバーサイドでサニタイズ済みのHTMLを反映する
  // XSS防止: pulldown-cmarkでraw HTML無効化済み（renderer.rs参照）
  function updateContent(data) {
    // 直接更新が実行されるため、保留中の更新とタイマーをクリア
    // ファイル遷移やrefreshで古い保留更新が適用されるのを防ぐ
    if (pendingUpdateTimer) {
      clearTimeout(pendingUpdateTimer);
      pendingUpdateTimer = null;
    }
    pendingUpdate = null;
    var scrollY = window.scrollY;
    var contentEl = document.getElementById('content');
    var tocEl = document.getElementById('toc');

    if (data.content !== undefined) {
      contentEl.innerHTML = data.content;
    }
    if (data.toc !== undefined) {
      tocEl.innerHTML = data.toc;
    }

    requestAnimationFrame(function() {
      window.scrollTo(0, scrollY);
      updateReadingProgress();
    });

    setupTocTracking();
    updateDocumentStats();
    syncDocumentChrome(currentFile);
    enhanceContentInteractions();
    setupTocFilter();
  }

  // ディレクトリモード: ファイル選択
  // pushHistory=false の場合はhistoryに追加しない（popstate/refresh経由）
  var fetchGeneration = 0;
  function selectFile(file, pushHistory) {
    if (pushHistory === undefined) pushHistory = true;
    var previousFile = currentFile;
    var gen = ++fetchGeneration;
    currentFile = file;
    if (pushHistory) setFileParam(file);
    updateFileListActive(file);

    // API経由でコンテンツを取得
    fetch('/api/content?file=' + encodeURIComponent(file), {
      headers: { 'Accept': 'application/json' }
    })
    .then(function(resp) {
      if (!resp.ok) throw createHttpError(resp.status);
      return resp.json().catch(function(err) {
        err.type = 'parse';
        throw err;
      });
    })
    .then(function(data) {
      // エラーバナーは成功レスポンスが来た時点で常にクリア（世代に関わらず安全）
      hideFileFetchErrorBanner();
      // 別のファイル選択が行われた場合はこのレスポンスを破棄
      if (gen !== fetchGeneration) return;
      updateContent(data);
      // サーバーの正規化済みパスでcurrentFileを同期
      // シンボリックリンク等で要求パスと返却パスが異なる場合に、
      // WebSocket更新のdata.fileフィルタリングが正しく動作するようにする
      if (data.file && data.file !== currentFile) {
        currentFile = data.file;
        setFileParam(currentFile, true);
        updateFileListActive(currentFile);
      }
      syncDocumentChrome(currentFile);
      setLiveStatus('live', 'Live');
    })
    .catch(function(err) {
      console.error('[markdown-view] ファイル取得エラー:', err);
      // 別のファイル選択が行われた場合はロールバック不要
      if (gen !== fetchGeneration) return;
      // 失敗時は前の状態にロールバック
      currentFile = previousFile;
      updateFileListActive(previousFile);
      // URLを元に戻す（pushHistory時はpushState、popstate時はreplaceState）
      setFileParam(previousFile, !pushHistory);
      showFileFetchErrorBanner(getFileFetchErrorMessage(err));
      setLiveStatus('error', 'Fetch failed');
    });
  }

  // ファイル一覧のアクティブ状態を更新
  function updateFileListActive(file) {
    // ファイルツリーノードのアクティブ状態を切り替え
    var items = document.querySelectorAll('.file-tree-file');
    items.forEach(function(li) {
      var link = li.querySelector('a');
      if (link && link.getAttribute('data-file') === file) {
        li.classList.add('active');
        // 祖先のdetails要素をすべて開く
        var parent = li.parentElement;
        while (parent && parent.id !== 'sidebar') {
          if (parent.tagName === 'DETAILS') {
            parent.open = true;
          }
          parent = parent.parentElement;
        }
      } else {
        li.classList.remove('active');
      }
    });
  }

  // ファイル一覧のクリックハンドラ設定
  function setupFileList() {
    var fileLinks = document.querySelectorAll('.file-tree-file a[data-file]');
    fileLinks.forEach(function(link) {
      link.addEventListener('click', function(e) {
        e.preventDefault();
        selectFile(link.getAttribute('data-file'));
      });
    });
  }

  function setupFileFilter() {
    var input = document.getElementById('file-filter');
    var summary = document.getElementById('file-filter-summary');
    if (!input) return;

    var updateSummary = function(visible, total) {
      if (!summary) return;
      if (input.value.trim()) {
        summary.textContent = visible + ' / ' + total + ' files';
      } else {
        summary.textContent = total + ' files';
      }
    };

    var applyFilter = function() {
      var query = input.value.trim().toLowerCase();
      var fileItems = document.querySelectorAll('.file-tree-file');
      var total = fileItems.length;
      var visible = 0;

      fileItems.forEach(function(item) {
        var link = item.querySelector('a[data-file]');
        var matched = !query || (link && link.getAttribute('data-file').toLowerCase().indexOf(query) !== -1);
        item.hidden = !matched;
        if (matched) visible++;
      });

      var dirs = document.querySelectorAll('.file-tree-dir');
      dirs.forEach(function(dir) {
        var descendants = dir.querySelectorAll('.file-tree-file');
        var hasVisibleChild = Array.prototype.some.call(descendants, function(item) {
          return !item.hidden;
        });
        dir.parentElement.hidden = !hasVisibleChild;
        if (query && hasVisibleChild) {
          dir.open = true;
        }
      });

      updateSummary(visible, total);
    };

    input.addEventListener('input', applyFilter);
    updateSummary(document.querySelectorAll('.file-tree-file').length, document.querySelectorAll('.file-tree-file').length);
  }

  // タブ切り替え設定
  function setupTabs() {
    var tabs = document.querySelectorAll('.sidebar-tab');
    tabs.forEach(function(tab) {
      tab.addEventListener('click', function() {
        var target = tab.getAttribute('data-tab');
        // タブのアクティブ状態を切り替え
        tabs.forEach(function(t) { t.classList.remove('active'); });
        tab.classList.add('active');
        // パネルの表示を切り替え
        var panels = document.querySelectorAll('.sidebar-panel');
        panels.forEach(function(panel) {
          if (panel.id === 'panel-' + target) {
            panel.classList.add('active');
          } else {
            panel.classList.remove('active');
          }
        });
      });
    });
  }

  // ブラウザの戻る/進むボタン対応（historyに追加せずコンテンツのみ更新）
  if (isDirMode) {
    window.addEventListener('popstate', function() {
      var file = getFileParam();
      if (file && file !== currentFile) {
        selectFile(file, false);
      }
    });
  }

  // TOCアクティブ追跡（IntersectionObserver）
  var currentObserver = null;

  function setupTocTracking() {
    // 既存のObserverをクリーンアップ（メモリリーク防止）
    if (currentObserver) {
      currentObserver.disconnect();
      currentObserver = null;
    }

    var headings = document.querySelectorAll('#content h1, #content h2, #content h3, #content h4, #content h5, #content h6');
    var tocLinks = document.querySelectorAll('#toc a');

    if (headings.length === 0 || tocLinks.length === 0) return;

    currentObserver = new IntersectionObserver(function(entries) {
      entries.forEach(function(entry) {
        if (entry.isIntersecting) {
          var id = entry.target.getAttribute('id');
          tocLinks.forEach(function(link) {
            link.classList.toggle('active', link.getAttribute('href') === '#' + id);
          });
        }
      });
    }, { rootMargin: '-10% 0% -80% 0%' });

    headings.forEach(function(heading) {
      if (heading.id) currentObserver.observe(heading);
    });
  }

  // サイドバー開閉（モバイル）
  var sidebarToggle = document.getElementById('sidebar-toggle');
  var sidebarOpen = document.getElementById('sidebar-open');
  var sidebar = document.getElementById('sidebar');

  if (sidebarToggle) {
    sidebarToggle.addEventListener('click', function() {
      sidebar.classList.remove('open');
    });
  }

  if (sidebarOpen) {
    sidebarOpen.addEventListener('click', function() {
      sidebar.classList.add('open');
    });
  }

  if (backToTop) {
    backToTop.addEventListener('click', function() {
      window.scrollTo({ top: 0, behavior: 'smooth' });
    });
  }

  // テーマ切替
  var themeToggle = document.getElementById('theme-toggle');
  if (themeToggle) {
    themeToggle.addEventListener('click', function() {
      var current = htmlEl.getAttribute('data-theme');
      var next;
      if (current === 'dark') {
        next = 'light';
      } else if (current === 'light') {
        next = 'dark';
      } else {
        // data-themeなし → システム設定の逆にする
        var prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
        next = prefersDark ? 'light' : 'dark';
      }
      htmlEl.setAttribute('data-theme', next);
      try { localStorage.setItem('mdview-theme', next); } catch(e) {
        console.warn('[markdown-view] テーマ設定の保存に失敗:', e.message);
      }
    });

    // 保存されたテーマを復元
    try {
      var saved = localStorage.getItem('mdview-theme');
      if (saved === 'light' || saved === 'dark') {
        htmlEl.setAttribute('data-theme', saved);
      }
    } catch(e) {
      console.warn('[markdown-view] テーマ設定の読込に失敗:', e.message);
    }
  }

  connectWS();
  setupTocTracking();
  updateDocumentStats();
  updateReadingProgress();
  syncDocumentChrome(currentFile);
  enhanceContentInteractions();
  setupTocFilter();
  window.addEventListener('scroll', updateReadingProgress, { passive: true });
  window.addEventListener('resize', updateReadingProgress);
  if (isDirMode) {
    setupFileList();
    setupTabs();
    setupFileFilter();
  }
})();
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{render_markdown, syntax_theme_css};
    use crate::toc::generate_toc;

    fn test_content() -> SanitizedHtml {
        render_markdown("content")
    }

    fn test_toc() -> SanitizedHtml {
        generate_toc("# toc")
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
            _ => panic!("docsディレクトリを期待"),
        }
    }

    #[test]
    fn test_ディレクトリモードでタブ構造が生成される() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        // タブボタンが存在する
        assert!(html.contains("sidebar-tab"));
        assert!(html.contains("data-tab=\"files\""));
        assert!(html.contains("data-tab=\"toc\""));
        // パネルが存在する
        assert!(html.contains("id=\"panel-files\""));
        assert!(html.contains("id=\"panel-toc\""));
        assert!(html.contains("id=\"file-filter\""));
        assert!(html.contains("id=\"file-filter-summary\""));
    }

    #[test]
    fn test_単一ファイルモードでタブが生成されない() {
        let content = test_content();
        let toc = test_toc();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });

        // タブボタンのHTML要素が存在しない（CSSクラス定義ではなくHTML構造を検証）
        assert!(!html.contains("data-tab=\"files\""));
        assert!(!html.contains("id=\"panel-files\""));
        assert!(!html.contains("id=\"file-filter\""));
    }

    #[test]
    fn test_読書ワークスペース用uiが描画される() {
        let content = test_content();
        let toc = test_toc();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "A Title",
            content: &content,
            toc: &toc,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::SingleFile,
        });

        assert!(html.contains("class=\"topbar\""));
        assert!(html.contains("id=\"document-title\""));
        assert!(html.contains("id=\"doc-heading-count\""));
        assert!(html.contains("id=\"doc-char-count\""));
        assert!(html.contains("id=\"reading-progress-bar\""));
        assert!(html.contains("id=\"back-to-top\""));
        assert!(html.contains("function updateDocumentStats()"));
        assert!(html.contains("function updateReadingProgress()"));
        assert!(html.contains("function syncDocumentChrome(file)"));
        assert!(html.contains("id=\"toc-filter\""));
        assert!(html.contains("function enhanceContentInteractions()"));
        assert!(html.contains("function setupTocFilter()"));
        assert!(html.contains("className = 'code-copy'"));
        assert!(html.contains("className = 'heading-anchor'"));
        assert!(html.contains("id=\"theme-toggle\""));
        assert!(html.contains("theme-icon-light"));
        assert!(html.contains("theme-icon-dark"));
        assert!(html.contains("localStorage.setItem('mdview-theme'"));
    }

    #[test]
    fn test_selectfile_fetch失敗時の視覚フィードバックjsが埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        // バナー表示/非表示関数が存在する
        assert!(html.contains("function showFileFetchErrorBanner(message)"));
        assert!(html.contains("function hideFileFetchErrorBanner()"));
        assert!(html.contains("file-fetch-error-banner"));
        assert!(html.contains("className = 'error-banner fetch'"));
        assert!(html.contains(".error-banner {"));
        assert!(!html.contains("style.cssText ="));
        // 閉じるボタンが存在する
        assert!(html.contains("closeBtn.onclick = hideFileFetchErrorBanner"));
        // WebSocket切断バナー表示中はfetchエラーバナーを抑制する
        assert!(html.contains("getElementById('ws-disconnect-banner')"));
        // HTTPエラーとJSONパースエラーを区別する
        assert!(html.contains("function createHttpError(status)"));
        assert!(html.contains("function getFileFetchErrorMessage(err)"));
        assert!(html.contains("err.type = 'parse';"));
        // fetch成功時にバナーをクリア（generation チェック前）
        assert!(html.contains("hideFileFetchErrorBanner();"));
        // fetch失敗時にエラー種別に応じたメッセージを表示
        assert!(html.contains("showFileFetchErrorBanner(getFileFetchErrorMessage(err));"));
    }

    #[test]
    fn test_websocket更新時にfetchエラーバナーがクリアされる() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        // WebSocket経由の成功更新後にバナーをクリアするコメントとコードが存在する
        assert!(html.contains("WebSocket経由の成功更新で各種エラーバナーをクリア"));
        assert!(html.contains("hideWsServerErrorBanner();"));
        assert!(html.contains("hideFileFetchErrorBanner();"));
    }

    #[test]
    fn test_websocket_data_error時の視覚フィードバックjsが埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        // サーバーエラーバナー表示/非表示関数が存在する
        assert!(html.contains("function showWsServerErrorBanner(message)"));
        assert!(html.contains("function hideWsServerErrorBanner()"));
        assert!(html.contains("ws-server-error-banner"));
        assert!(html.contains("className = 'error-banner server'"));
        assert!(html.contains("className = 'error-banner disconnect'"));
        // 閉じるボタンが存在する
        assert!(html.contains("closeBtn.onclick = hideWsServerErrorBanner"));
        // WebSocket切断バナー表示中はサーバーエラーバナーを抑制する
        assert!(html.contains("getElementById('ws-disconnect-banner')"));
        // data.error受信時にバナー表示を呼び出す
        assert!(html.contains("showWsServerErrorBanner(data.error);"));
    }

    #[test]
    fn test_websocket_json_parse_error時の視覚フィードバックjsが埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
                file_list: &files,
                current_file: Some("README.md"),
            },
        });

        assert!(html.contains("function showWsParseErrorBanner(message)"));
        assert!(html.contains("function hideWsParseErrorBanner()"));
        assert!(html.contains("ws-parse-error-banner"));
        assert!(html.contains("showWsParseErrorBanner('サーバーから不正なJSONを受信しました。ページを再読み込みしてください。');"));
        assert!(html.contains("hideWsParseErrorBanner();"));
    }

    #[test]
    fn test_websocket_refresh時もテキスト選択延期機構を通る() {
        let files = vec!["README.md".to_string()];
        let content = test_content();
        let toc = test_toc();
        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let html = render_page(RenderPageParams {
            title: "Test",
            content: &content,
            toc: &toc,
            dark_mode: false,
            syntax_css: &syntax_css,
            sidebar: SidebarParams::Directory {
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
    fn test_cspハッシュがrender_pageのstyle内容と一致する() {
        use base64::Engine as _;
        use sha2::Digest as _;

        let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
        let (script_src, style_src) = csp_hash_sources(&syntax_css);

        // render_pageに埋め込まれるCSS/JSと同一の内容からハッシュを計算
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
