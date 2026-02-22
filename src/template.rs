use crate::renderer::html_escape;

/// HTMLテンプレートを生成する
///
/// CSS/JSをすべて埋め込み、外部ファイル不要で動作する
pub fn render_page(title: &str, content: &str, toc: &str, dark_mode: bool) -> String {
    let escaped_title = html_escape(title);
    format!(
        r##"<!DOCTYPE html>
<html lang="ja" data-theme="{theme}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - markdown-view</title>
<style>
{css}
</style>
</head>
<body>
<aside id="sidebar" class="sidebar">
  <div class="sidebar-header">
    <h2>目次</h2>
    <button id="sidebar-toggle" class="sidebar-toggle" aria-label="目次を閉じる">×</button>
  </div>
  <nav id="toc">{toc}</nav>
</aside>
<button id="sidebar-open" class="sidebar-open" aria-label="目次を開く">☰</button>
<main id="content" class="content">
{content}
</main>
<script>
{js}
</script>
</body>
</html>"##,
        theme = if dark_mode { "dark" } else { "light" },
        title = escaped_title,
        css = CSS,
        toc = toc,
        content = content,
        js = JS,
    )
}

/// コンテンツ更新用JSONメッセージ構造体（HTTP API・WebSocket共用）
#[derive(serde::Serialize)]
pub struct UpdateMessage {
    pub content: String,
    pub toc: String,
}

const CSS: &str = r##"
/* リセットと基本設定 */
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

:root {
  --bg: #ffffff;
  --fg: #24292f;
  --sidebar-bg: #f6f8fa;
  --sidebar-border: #d0d7de;
  --link: #0969da;
  --code-bg: #f6f8fa;
  --blockquote-border: #d0d7de;
  --blockquote-fg: #57606a;
  --table-border: #d0d7de;
  --table-alt-bg: #f6f8fa;
  --hr-color: #d8dee4;
  --toc-active: #0969da;
  --toc-hover-bg: #eaeef2;
}

[data-theme="dark"] {
  --bg: #0d1117;
  --fg: #e6edf3;
  --sidebar-bg: #161b22;
  --sidebar-border: #30363d;
  --link: #58a6ff;
  --code-bg: #161b22;
  --blockquote-border: #30363d;
  --blockquote-fg: #8b949e;
  --table-border: #30363d;
  --table-alt-bg: #161b22;
  --hr-color: #21262d;
  --toc-active: #58a6ff;
  --toc-hover-bg: #1c2128;
}

@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #0d1117;
    --fg: #e6edf3;
    --sidebar-bg: #161b22;
    --sidebar-border: #30363d;
    --link: #58a6ff;
    --code-bg: #161b22;
    --blockquote-border: #30363d;
    --blockquote-fg: #8b949e;
    --table-border: #30363d;
    --table-alt-bg: #161b22;
    --hr-color: #21262d;
    --toc-active: #58a6ff;
    --toc-hover-bg: #1c2128;
  }
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif;
  background: var(--bg);
  color: var(--fg);
  line-height: 1.6;
  display: flex;
  min-height: 100vh;
}

/* サイドバー（TOC） */
.sidebar {
  width: 280px;
  min-width: 280px;
  background: var(--sidebar-bg);
  border-right: 1px solid var(--sidebar-border);
  padding: 1rem;
  overflow-y: auto;
  position: sticky;
  top: 0;
  height: 100vh;
  transition: transform 0.3s ease;
}

.sidebar-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1rem;
  padding-bottom: 0.5rem;
  border-bottom: 1px solid var(--sidebar-border);
}

.sidebar-header h2 {
  font-size: 0.875rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--blockquote-fg);
}

.sidebar-toggle {
  display: none;
  background: none;
  border: none;
  font-size: 1.25rem;
  cursor: pointer;
  color: var(--fg);
}

.sidebar-open {
  display: none;
  position: fixed;
  top: 0.75rem;
  left: 0.75rem;
  z-index: 1000;
  background: var(--sidebar-bg);
  border: 1px solid var(--sidebar-border);
  border-radius: 4px;
  padding: 0.25rem 0.5rem;
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
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  color: var(--fg);
  text-decoration: none;
  font-size: 0.875rem;
  transition: background 0.15s;
}

#toc a:hover { background: var(--toc-hover-bg); }
#toc a.active { color: var(--toc-active); font-weight: 600; }

/* メインコンテンツ */
.content {
  flex: 1;
  max-width: 900px;
  margin: 0 auto;
  padding: 2rem 3rem;
}

.content h1, .content h2, .content h3, .content h4, .content h5, .content h6 {
  margin-top: 1.5em;
  margin-bottom: 0.5em;
  font-weight: 600;
  line-height: 1.25;
}

.content h1 { font-size: 2em; padding-bottom: 0.3em; border-bottom: 1px solid var(--hr-color); }
.content h2 { font-size: 1.5em; padding-bottom: 0.3em; border-bottom: 1px solid var(--hr-color); }
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
  background: var(--code-bg);
  padding: 1rem;
  border-radius: 6px;
  overflow-x: auto;
  margin-bottom: 1em;
  line-height: 1.45;
}

.content pre.code-block code {
  background: none;
  padding: 0;
  font-size: 85%;
}

.content blockquote {
  border-left: 4px solid var(--blockquote-border);
  padding: 0.5rem 1rem;
  margin-bottom: 1em;
  color: var(--blockquote-fg);
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

/* モバイル対応 */
@media (max-width: 768px) {
  .sidebar {
    position: fixed;
    left: 0;
    top: 0;
    z-index: 999;
    transform: translateX(-100%);
    width: 280px;
    box-shadow: 2px 0 8px rgba(0,0,0,0.15);
  }
  .sidebar.open { transform: translateX(0); }
  .sidebar-toggle { display: block; }
  .sidebar-open { display: block; }
  .content { padding: 1.5rem 1rem; padding-top: 3rem; }
}
"##;

// セキュリティ注記:
// コンテンツ更新はサーバーサイドでpulldown-cmarkによりパースされたHTMLのみを反映する。
// renderer.rsで明示的にEvent::Html / Event::InlineHtmlを無視しているため、
// raw HTMLの注入によるXSSリスクは軽減されている。
// サーバーは127.0.0.1にバインドし、Host/Originヘッダー検証（server.rs）で
// DNS Rebinding攻撃を防止している。
const JS: &str = r##"
(function() {
  'use strict';

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
    };

    ws.onmessage = function(event) {
      try {
        var data = JSON.parse(event.data);
        if (data.error) {
          console.error('[markdown-view] サーバーエラー:', data.error);
          return;
        }
        updateContent(data);
      } catch (e) {
        console.error('[markdown-view] parse error:', e);
      }
    };

    ws.onclose = function() {
      scheduleReconnect();
    };

    ws.onerror = function(event) {
      console.error('[markdown-view] WebSocketエラー:', event);
      ws.close();
    };
  }

  function scheduleReconnect() {
    if (reconnectAttempts >= WS_RECONNECT_MAX_ATTEMPTS) {
      console.error('[markdown-view] 再接続上限に達しました。ページをリロードしてください');
      return;
    }
    var delay = Math.min(WS_RECONNECT_BASE * Math.pow(2, reconnectAttempts), WS_RECONNECT_MAX_DELAY);
    reconnectAttempts++;
    setTimeout(connectWS, delay);
  }

  function updateContent(data) {
    var scrollY = window.scrollY;
    var contentEl = document.getElementById('content');
    var tocEl = document.getElementById('toc');

    // サーバーサイドでサニタイズ済みのHTMLを反映
    // （pulldown-cmarkでraw HTML無効化 + Host/Origin検証）
    if (data.content !== undefined) {
      contentEl.innerHTML = data.content;
    }
    if (data.toc !== undefined) {
      tocEl.innerHTML = data.toc;
    }

    requestAnimationFrame(function() {
      window.scrollTo(0, scrollY);
    });

    setupTocTracking();
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

  connectWS();
  setupTocTracking();
})();
"##;
