use std::sync::OnceLock;

use crate::server::MAX_FILE_SIZE;

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
  padding-inline: 0.65rem;
  font-size: 1.25rem;
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

pub(crate) fn css() -> &'static str {
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

pub(crate) fn inline_js() -> String {
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

  // グローバルDOM参照の初期化
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
  var LIVE_STATUS_LABELS = {
    live: 'Live',
    retry: 'Reconnecting',
    error: 'Error',
    offline: 'Offline'
  };

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

  function setLiveStatus(state) {
    if (!liveStatusEl) return;
    liveStatusEl.textContent = LIVE_STATUS_LABELS[state] || state;
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

  function handleCopyClick(button, text, baseLabel) {
    copyText(text).then(function() {
      flashCopiedState(button, 'Copied', baseLabel);
    }).catch(function(err) {
      console.warn('[markdown-view] コピーに失敗:', err);
      flashCopiedState(button, 'Failed', baseLabel);
    });
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
        handleCopyClick(button, url.toString(), '#');
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
        handleCopyClick(button, code.innerText || code.textContent || '', 'Copy');
      });
      block.appendChild(button);
    });
  }

  function setupFilterableList(options) {
    var input = document.getElementById(options.inputId);
    var root = document.getElementById(options.rootId);
    if (!input || !root) return;

    var items = options.getItems(root);
    var applyFilter = function() {
      var query = input.value.trim().toLowerCase();
      options.apply(items, query, input);
    };

    input.addEventListener('input', applyFilter);
    applyFilter();
  }

  function setupTocFilter() {
    setupFilterableList({
      inputId: 'toc-filter',
      rootId: 'toc',
      getItems: function(root) {
        return root.querySelectorAll('li');
      },
      apply: function(items, query) {
        items.forEach(function(item) {
          var link = item.querySelector(':scope > a');
          if (!link) return;
          var matched = !query || link.textContent.toLowerCase().indexOf(query) !== -1;
          item.hidden = !matched;
        });
      }
    });
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
    setLiveStatus('live');
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
      setLiveStatus('live');
    };

    ws.onmessage = function(event) {
      var data;
      try {
        data = JSON.parse(event.data);
      } catch (e) {
        console.error('[markdown-view] JSONパースエラー:', e);
        showWsParseErrorBanner('サーバーから不正なJSONを受信しました。ページを再読み込みしてください。');
        setLiveStatus('error');
        return;
      }
      hideWsParseErrorBanner();
      if (data.error) {
        console.error('[markdown-view] サーバーエラー:', data.error);
        showWsServerErrorBanner(data.error);
        setLiveStatus('error');
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
      setLiveStatus('live');
    };

    ws.onclose = function() {
      setLiveStatus('retry');
      scheduleReconnect();
    };

    ws.onerror = function(event) {
      console.error('[markdown-view] WebSocketエラー:', event);
      setLiveStatus('error');
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
    setLiveStatus('offline');
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
      setLiveStatus('live');
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
      setLiveStatus('error');
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
    var summary = document.getElementById('file-filter-summary');
    setupFilterableList({
      inputId: 'file-filter',
      rootId: 'panel-files',
      getItems: function(root) {
        return root.querySelectorAll('.file-tree-file');
      },
      apply: function(items, query, input) {
        var total = items.length;
        var visible = 0;

        items.forEach(function(item) {
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

        if (summary) {
          if (input.value.trim()) {
            summary.textContent = visible + ' / ' + total + ' files';
          } else {
            summary.textContent = total + ' files';
          }
        }
      }
    });
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
