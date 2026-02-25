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

    // ディレクトリモードフラグをdata属性で渡す
    let dir_mode_attr = if file_list.is_some() {
        format!(
            " data-dir-mode=\"true\" data-current-file=\"{}\"",
            html_escape(current_file.unwrap_or(""))
        )
    } else {
        String::new()
    };

    // サイドバー内部HTML: ディレクトリモード時はタブ切り替え式、単一ファイルモードは従来通り
    let sidebar_inner = match file_list {
        Some(files) => {
            let tree = build_file_tree(files);
            let tree_html = render_file_tree_html(&tree, current_file);
            format!(
                r##"  <div class="sidebar-tabs">
    <button class="sidebar-tab active" data-tab="files">ファイル</button>
    <button class="sidebar-tab" data-tab="toc">目次</button>
    <button id="sidebar-toggle" class="sidebar-toggle" aria-label="閉じる">×</button>
  </div>
  <div class="sidebar-panel active" id="panel-files">
    <div class="file-list">
{tree_html}    </div>
  </div>
  <div class="sidebar-panel" id="panel-toc">
    <nav id="toc">{toc}</nav>
  </div>"##,
                tree_html = tree_html,
                toc = toc,
            )
        }
        None => {
            format!(
                r##"  <div class="sidebar-header">
    <h2>目次</h2>
    <button id="sidebar-toggle" class="sidebar-toggle" aria-label="目次を閉じる">×</button>
  </div>
  <nav id="toc">{toc}</nav>"##,
                toc = toc,
            )
        }
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
{sidebar_inner}
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
        sidebar_inner = sidebar_inner,
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

/* サイドバー */
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
  display: flex;
  flex-direction: column;
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

/* タブバー */
.sidebar-tabs {
  display: flex;
  align-items: center;
  border-bottom: 1px solid var(--sidebar-border);
  margin-bottom: 0.5rem;
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
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  color: var(--fg);
  text-decoration: none;
  font-size: 0.8125rem;
  transition: background 0.15s;
  word-break: break-all;
}

.file-list a:hover { background: var(--toc-hover-bg); }
.file-tree-file.active a { color: var(--toc-active); font-weight: 600; }

/* ファイルツリー */
.file-tree-dir > summary {
  cursor: pointer;
  font-size: 0.8125rem;
  padding: 0.2rem 0.4rem;
  border-radius: 4px;
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
  .sidebar-tabs .sidebar-toggle { display: block; }
  .sidebar-open { display: block; }
  .content { padding: 1.5rem 1rem; padding-top: 3rem; }
}
"##;

// セキュリティ注記:
// innerHTML使用箇所: updateContent()内でサーバーサイドでサニタイズ済みHTMLを反映。
// XSS防止: pulldown-cmarkのEvent::Html/Event::InlineHtmlを除去し、
// raw HTMLが出力に含まれないようにしている（renderer.rs）。
// DNS Rebinding防止: 127.0.0.1バインド + Host/Originヘッダー検証（server.rs）。
/// ファイルツリーのノード（ディレクトリまたはファイル）
///
/// - `full_path.is_some()`: ファイルノード
/// - `full_path.is_none()`: ディレクトリノード（`children`を持つ）
#[derive(Debug, Clone, PartialEq)]
pub struct FileTreeNode {
    /// 表示名（ディレクトリ名 or ファイル名）
    pub name: String,
    /// ファイルの場合のみ: 完全相対パス
    pub full_path: Option<String>,
    /// 子ノード
    pub children: Vec<FileTreeNode>,
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
                result.push(FileTreeNode {
                    name,
                    full_path: None,
                    children: child.into_tree_nodes(),
                });
            }

            // ファイル（アルファベット順にソート）
            let mut files = self.files;
            files.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, full_path) in files {
                result.push(FileTreeNode {
                    name,
                    full_path: Some(full_path),
                    children: Vec::new(),
                });
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
            if node.full_path.is_some() {
                // ファイルノード
                let is_active = current_file == node.full_path.as_deref();
                let class = if is_active {
                    "file-tree-file active"
                } else {
                    "file-tree-file"
                };
                let escaped_name = html_escape(&node.name);
                let escaped_path = html_escape(node.full_path.as_deref().unwrap_or(""));
                html.push_str(&format!(
                    "<li class=\"{class}\"><a href=\"#\" data-file=\"{path}\"><span class=\"tree-icon\">📄</span>{name}</a></li>\n",
                    class = class,
                    path = escaped_path,
                    name = escaped_name,
                ));
            } else {
                // ディレクトリノード
                let dir_path = if current_path.is_empty() {
                    node.name.clone()
                } else {
                    format!("{}/{}", current_path, node.name)
                };
                let is_open = active_dirs.contains(&dir_path);
                let open_attr = if is_open { " open" } else { "" };
                let escaped_name = html_escape(&node.name);
                html.push_str(&format!(
                    "<li>\n<details class=\"file-tree-dir\"{open}>\n<summary><span class=\"tree-icon-chevron\">▶</span><span class=\"tree-icon\">📁</span>{name}</summary>\n<ul class=\"file-tree-children\">\n",
                    open = open_attr,
                    name = escaped_name,
                ));
                render_nodes(&node.children, html, current_file, active_dirs, &dir_path);
                html.push_str("</ul>\n</details>\n</li>\n");
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

  function showFileFetchErrorBanner(message) {
    var banner = document.getElementById('file-fetch-error-banner');
    if (!banner) {
      banner = document.createElement('div');
      banner.id = 'file-fetch-error-banner';
      banner.style.cssText = 'position:fixed;top:0;left:0;right:0;padding:8px 16px;background:#d32f2f;color:#fff;text-align:center;z-index:9999;font-size:14px;';
      document.body.appendChild(banner);
    }
    banner.textContent = message;
  }

  function hideFileFetchErrorBanner() {
    var banner = document.getElementById('file-fetch-error-banner');
    if (banner) {
      banner.remove();
    }
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
      hideFileFetchErrorBanner();
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
      showFileFetchErrorBanner('ファイルの読み込みに失敗しました。再度お試しください。');
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

  connectWS();
  setupTocTracking();
  if (isDirMode) {
    setupFileList();
    setupTabs();
  }
})();
"##;

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(tree[0].name, "docs");
        assert!(tree[0].full_path.is_none());
        assert_eq!(tree[0].children.len(), 2);

        // docs/guide ディレクトリ（ディレクトリ先）
        assert_eq!(tree[0].children[0].name, "guide");
        assert!(tree[0].children[0].full_path.is_none());
        assert_eq!(tree[0].children[0].children.len(), 1);

        // docs/guide/intro.md
        assert_eq!(tree[0].children[0].children[0].name, "intro.md");
        assert_eq!(
            tree[0].children[0].children[0].full_path.as_deref(),
            Some("docs/guide/intro.md")
        );

        // docs/api.md（ファイル後）
        assert_eq!(tree[0].children[1].name, "api.md");
        assert_eq!(
            tree[0].children[1].full_path.as_deref(),
            Some("docs/api.md")
        );

        // README.md（ルート直下ファイル）
        assert_eq!(tree[1].name, "README.md");
        assert_eq!(tree[1].full_path.as_deref(), Some("README.md"));
        assert!(tree[1].children.is_empty());
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
        assert_eq!(tree[0].name, "CHANGELOG.md");
        assert!(tree[0].full_path.is_some());
        assert_eq!(tree[1].name, "README.md");
        assert!(tree[1].full_path.is_some());
    }

    #[test]
    fn test_深いネストのファイルツリー() {
        let files = vec!["a/b/c/d.md".to_string()];
        let tree = build_file_tree(&files);

        // a/
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "a");
        assert!(tree[0].full_path.is_none());

        // a/b/
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].name, "b");
        assert!(tree[0].children[0].full_path.is_none());

        // a/b/c/
        assert_eq!(tree[0].children[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children[0].name, "c");
        assert!(tree[0].children[0].children[0].full_path.is_none());

        // a/b/c/d.md
        assert_eq!(tree[0].children[0].children[0].children.len(), 1);
        let leaf = &tree[0].children[0].children[0].children[0];
        assert_eq!(leaf.name, "d.md");
        assert_eq!(leaf.full_path.as_deref(), Some("a/b/c/d.md"));
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
        assert_eq!(tree[0].name, "docs");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].name, "guide.md");
        assert_eq!(
            tree[0].children[0].full_path.as_deref(),
            Some("docs/guide.md")
        );
    }

    #[test]
    fn test_ディレクトリモードでタブ構造が生成される() {
        let files = vec!["README.md".to_string()];
        let html = render_page(
            "Test",
            "<p>content</p>",
            "<ul><li>toc</li></ul>",
            false,
            Some(&files),
            Some("README.md"),
        );

        // タブボタンが存在する
        assert!(html.contains("sidebar-tab"));
        assert!(html.contains("data-tab=\"files\""));
        assert!(html.contains("data-tab=\"toc\""));
        // パネルが存在する
        assert!(html.contains("id=\"panel-files\""));
        assert!(html.contains("id=\"panel-toc\""));
    }

    #[test]
    fn test_単一ファイルモードでタブが生成されない() {
        let html = render_page(
            "Test",
            "<p>content</p>",
            "<ul><li>toc</li></ul>",
            false,
            None,
            None,
        );

        // タブボタンのHTML要素が存在しない（CSSクラス定義ではなくHTML構造を検証）
        assert!(!html.contains("data-tab=\"files\""));
        assert!(!html.contains("id=\"panel-files\""));
    }

    #[test]
    fn test_selectfile_fetch失敗時の視覚フィードバックjsが埋め込まれる() {
        let files = vec!["README.md".to_string()];
        let html = render_page(
            "Test",
            "<p>content</p>",
            "<ul><li>toc</li></ul>",
            false,
            Some(&files),
            Some("README.md"),
        );

        assert!(html.contains("function showFileFetchErrorBanner(message)"));
        assert!(html.contains("function hideFileFetchErrorBanner()"));
        assert!(html.contains("file-fetch-error-banner"));
        assert!(html.contains("hideFileFetchErrorBanner();"));
        assert!(html.contains(
            "showFileFetchErrorBanner('ファイルの読み込みに失敗しました。再度お試しください。');"
        ));
    }
}
