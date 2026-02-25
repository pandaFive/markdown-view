use crate::renderer::html_escape;

/// HTMLテンプレートを生成する
///
/// CSS/JSをすべて埋め込み、外部ファイル不要で動作する
///
/// - `file_list`: ディレクトリモード時のファイル一覧（`None`なら単一ファイルモード）
/// - `current_file`: ディレクトリモード時の現在表示ファイル相対パス
pub fn render_page(
    title: &str,
    content: &str,
    toc: &str,
    dark_mode: bool,
    file_list: Option<&[String]>,
    current_file: Option<&str>,
) -> String {
    let escaped_title = html_escape(title);

    // ファイル一覧HTML（ディレクトリモードのみ）
    let file_list_html = match file_list {
        Some(files) => {
            let mut html = String::from(
                "<div class=\"file-list\">\n<div class=\"file-list-header\"><h2>ファイル</h2></div>\n<ul>\n",
            );
            for file in files {
                let active = current_file.is_some_and(|c| c == file);
                let class = if active { " class=\"active\"" } else { "" };
                html.push_str(&format!(
                    "<li{class}><a href=\"#\" data-file=\"{file}\">{file}</a></li>\n",
                    class = class,
                    file = html_escape(file),
                ));
            }
            html.push_str("</ul>\n</div>\n");
            html
        }
        None => String::new(),
    };

    // ディレクトリモードフラグをdata属性で渡す
    let dir_mode_attr = if file_list.is_some() {
        format!(
            " data-dir-mode=\"true\" data-current-file=\"{}\"",
            html_escape(current_file.unwrap_or(""))
        )
    } else {
        String::new()
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="ja" data-theme="{theme}"{dir_mode_attr}>
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
  {file_list_html}
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
        dir_mode_attr = dir_mode_attr,
        title = escaped_title,
        css = CSS,
        file_list_html = file_list_html,
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
    /// ディレクトリモード時の変更ファイル相対パス（単一ファイルモードはNone）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
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

/* ファイル一覧 */
.file-list {
  margin-bottom: 1rem;
  padding-bottom: 0.5rem;
  border-bottom: 1px solid var(--sidebar-border);
}

.file-list-header h2 {
  font-size: 0.875rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--blockquote-fg);
  margin-bottom: 0.5rem;
}

.file-list ul {
  list-style: none;
  padding-left: 0;
  max-height: 200px;
  overflow-y: auto;
}

.file-list li { margin: 0.125rem 0; }

.file-list a {
  display: block;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  color: var(--fg);
  text-decoration: none;
  font-size: 0.8125rem;
  transition: background 0.15s;
  word-break: break-all;
}

.file-list a:hover { background: var(--toc-hover-bg); }
.file-list li.active a { color: var(--toc-active); font-weight: 600; }

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
// innerHTML使用箇所: updateContent()内でサーバーサイドでサニタイズ済みHTMLを反映。
// XSS防止: pulldown-cmarkのEvent::Html/Event::InlineHtmlを除去し、
// raw HTMLが出力に含まれないようにしている（renderer.rs）。
// DNS Rebinding防止: 127.0.0.1バインド + Host/Originヘッダー検証（server.rs）。
const JS: &str = r##"
(function() {
  'use strict';

  // ディレクトリモード判定
  var htmlEl = document.documentElement;
  var isDirMode = htmlEl.getAttribute('data-dir-mode') === 'true';
  var currentFile = htmlEl.getAttribute('data-current-file') || '';

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
    };

    ws.onmessage = function(event) {
      var data;
      try {
        data = JSON.parse(event.data);
      } catch (e) {
        console.error('[markdown-view] JSONパースエラー:', e);
        return;
      }
      if (data.error) {
        console.error('[markdown-view] サーバーエラー:', data.error);
        return;
      }
      // ディレクトリモード: サーバーからリフレッシュ要求時は現在ファイルを再取得
      if (data.refresh && isDirMode && currentFile) {
        selectFile(currentFile, false);
        return;
      }
      // ディレクトリモード: 自分の表示ファイルと一致する更新のみ適用
      if (isDirMode && data.file) {
        if (data.file !== currentFile) return;
      }
      updateContent(data);
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
      showDisconnectBanner();
      return;
    }
    var delay = Math.min(WS_RECONNECT_BASE * Math.pow(2, reconnectAttempts), WS_RECONNECT_MAX_DELAY);
    reconnectAttempts++;
    setTimeout(connectWS, delay);
  }

  function showDisconnectBanner() {
    if (document.getElementById('ws-disconnect-banner')) return;
    var banner = document.createElement('div');
    banner.id = 'ws-disconnect-banner';
    banner.style.cssText = 'position:fixed;top:0;left:0;right:0;padding:8px 16px;background:#d32f2f;color:#fff;text-align:center;z-index:9999;font-size:14px;';
    banner.textContent = 'ライブリロード接続が切断されました。ページをリロードしてください。';
    document.body.appendChild(banner);
  }

  // サーバーサイドでサニタイズ済みのHTMLを反映する
  // XSS防止: pulldown-cmarkでraw HTML無効化済み（renderer.rs参照）
  function updateContent(data) {
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
    });

    setupTocTracking();
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
      if (!resp.ok) throw new Error('HTTP ' + resp.status);
      return resp.json();
    })
    .then(function(data) {
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
      // タイトル更新
      var fileName = currentFile.split('/').pop() || currentFile;
      document.title = fileName + ' - markdown-view';
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
    });
  }

  // ファイル一覧のアクティブ状態を更新
  function updateFileListActive(file) {
    var items = document.querySelectorAll('.file-list li');
    items.forEach(function(li) {
      var link = li.querySelector('a');
      if (link && link.getAttribute('data-file') === file) {
        li.classList.add('active');
      } else {
        li.classList.remove('active');
      }
    });
  }

  // ファイル一覧のクリックハンドラ設定
  function setupFileList() {
    var fileLinks = document.querySelectorAll('.file-list a[data-file]');
    fileLinks.forEach(function(link) {
      link.addEventListener('click', function(e) {
        e.preventDefault();
        selectFile(link.getAttribute('data-file'));
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

  connectWS();
  setupTocTracking();
  if (isDirMode) {
    setupFileList();
  }
})();
"##;
