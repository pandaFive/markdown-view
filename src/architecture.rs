//! markdown-view自体の処理フローを表す軽量AST。
//!
//! Rust構文のASTではなく、起動・HTTP・監視・ブラウザ更新という
//! アプリケーション上の意味を持つ処理単位を表現する。

/// アプリケーション処理フローのノード。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowNode {
    /// 子ノードを束ねる処理グループ。
    Group {
        name: &'static str,
        children: Vec<FlowNode>,
    },
    /// 特定モジュール・関数に対応する処理ステップ。
    Step {
        name: &'static str,
        module: &'static str,
        function: Option<&'static str>,
    },
}

impl FlowNode {
    /// 処理グループを作成する。
    pub fn group(name: &'static str, children: Vec<FlowNode>) -> Self {
        Self::Group { name, children }
    }

    /// 処理ステップを作成する。
    pub fn step(name: &'static str, module: &'static str, function: Option<&'static str>) -> Self {
        Self::Step {
            name,
            module,
            function,
        }
    }

    /// ノード名を返す。
    pub fn name(&self) -> &'static str {
        match self {
            Self::Group { name, .. } | Self::Step { name, .. } => name,
        }
    }

    /// 対応するソースモジュールを返す。
    pub fn module(&self) -> Option<&'static str> {
        match self {
            Self::Group { .. } => None,
            Self::Step { module, .. } => Some(module),
        }
    }

    /// 対応する関数名を返す。
    pub fn function(&self) -> Option<&'static str> {
        match self {
            Self::Group { .. } => None,
            Self::Step { function, .. } => *function,
        }
    }

    /// 直下の子ノード名を返す。
    pub fn child_names(&self) -> Vec<&'static str> {
        match self {
            Self::Group { children, .. } => children.iter().map(Self::name).collect(),
            Self::Step { .. } => Vec::new(),
        }
    }

    /// 自分自身を含む子孫ノードを深さ優先で返す。
    pub fn descendants(&self) -> Vec<&FlowNode> {
        let mut nodes = Vec::new();
        self.collect_descendants(&mut nodes);
        nodes
    }

    /// 指定名のノードを子孫から検索する。
    pub fn find(&self, name: &str) -> Option<&FlowNode> {
        if self.name() == name {
            return Some(self);
        }

        match self {
            Self::Group { children, .. } => children.iter().find_map(|child| child.find(name)),
            Self::Step { .. } => None,
        }
    }

    /// Mermaid flowchart TD形式へ変換する。
    pub fn to_mermaid(&self) -> String {
        let mut output = String::from("flowchart TD\n");
        let mut next_id = 0;
        self.write_mermaid(&mut output, None, &mut next_id);
        output
    }

    fn collect_descendants<'a>(&'a self, nodes: &mut Vec<&'a FlowNode>) {
        nodes.push(self);
        if let Self::Group { children, .. } = self {
            for child in children {
                child.collect_descendants(nodes);
            }
        }
    }

    fn write_mermaid(&self, output: &mut String, parent_id: Option<usize>, next_id: &mut usize) {
        let current_id = *next_id;
        *next_id += 1;

        output.push_str(&format!(
            "  n{}[\"{}\"]\n",
            current_id,
            escape_mermaid_label(self.name())
        ));

        if let Some(parent_id) = parent_id {
            output.push_str(&format!("  n{} --> n{}\n", parent_id, current_id));
        }

        if let Self::Group { children, .. } = self {
            for child in children {
                child.write_mermaid(output, Some(current_id), next_id);
            }
        }
    }
}

/// markdown-viewの主要処理フローASTを返す。
pub fn application_flow() -> FlowNode {
    FlowNode::group(
        "markdown-view",
        vec![
            startup_flow(),
            http_render_flow(),
            watch_update_flow(),
            browser_websocket_flow(),
        ],
    )
}

fn startup_flow() -> FlowNode {
    FlowNode::group(
        "起動フロー",
        vec![
            FlowNode::step("ログ初期化", "src/main.rs", None),
            FlowNode::step("CLI引数解析", "src/cli.rs", None),
            FlowNode::step("パス検証", "src/main.rs", None),
            FlowNode::group(
                "モード判定",
                vec![
                    FlowNode::step("単一ファイルモード", "src/server/state.rs", None),
                    FlowNode::step("ディレクトリモード", "src/server/state.rs", None),
                ],
            ),
            FlowNode::step("テーマ検証", "src/renderer/mod.rs", None),
            FlowNode::step("AppState作成", "src/server/state.rs", None),
            FlowNode::step("監視サービス開始", "src/server/watch.rs", None),
            FlowNode::step("localhostサーバーbind", "src/main.rs", None),
            FlowNode::step(
                "ルーター作成",
                "src/server/routes.rs",
                Some("create_router"),
            ),
            FlowNode::step("HTTPサーバー起動", "src/main.rs", Some("main")),
        ],
    )
}

fn http_render_flow() -> FlowNode {
    FlowNode::group(
        "HTTPレンダリングフロー",
        vec![
            FlowNode::step("Host検証", "src/server/guards.rs", None),
            FlowNode::step(
                "ルーティング",
                "src/server/routes.rs",
                Some("create_router"),
            ),
            FlowNode::step("Markdown読み込み", "src/server/files/content.rs", None),
            FlowNode::step(
                "render_markdown",
                "src/renderer/mod.rs",
                Some("render_markdown"),
            ),
            FlowNode::step("Markdownイベント処理", "src/renderer/render.rs", None),
            FlowNode::step("TOC生成", "src/renderer/toc.rs", None),
            FlowNode::step("ページHTML生成", "src/template/page.rs", None),
            FlowNode::step("HTTPレスポンス", "src/server/routes.rs", None),
        ],
    )
}

fn watch_update_flow() -> FlowNode {
    FlowNode::group(
        "ファイル監視更新フロー",
        vec![
            FlowNode::step("ファイル監視", "src/watcher/runtime.rs", None),
            FlowNode::step("debounce処理", "src/watcher/strategy.rs", None),
            FlowNode::step("変更ファイル解決", "src/server/watch.rs", None),
            FlowNode::step("Markdown再読み込み", "src/server/files/content.rs", None),
            FlowNode::step(
                "render_markdown",
                "src/renderer/mod.rs",
                Some("render_markdown"),
            ),
            FlowNode::step("TOC再生成", "src/renderer/toc.rs", None),
            FlowNode::step(
                "WebSocket broadcast",
                "src/server/broadcast.rs",
                Some("notify_update"),
            ),
        ],
    )
}

fn browser_websocket_flow() -> FlowNode {
    FlowNode::group(
        "ブラウザWebSocketフロー",
        vec![
            FlowNode::step("初期HTML読み込み", "src/template/page.rs", None),
            FlowNode::step(
                "クライアント初期化",
                "src/template/assets/js/bootstrap.js",
                None,
            ),
            FlowNode::step("WebSocket接続", "src/template/assets/js/websocket.js", None),
            FlowNode::step(
                "更新メッセージ受信",
                "src/template/assets/js/websocket.js",
                None,
            ),
            FlowNode::step("本文差し替え", "src/template/assets/js/content.js", None),
            FlowNode::step("メモ同期", "src/template/assets/js/memo.js", None),
        ],
    )
}

fn escape_mermaid_label(label: &str) -> String {
    label.replace('\\', "\\\\").replace('"', "\\\"")
}
