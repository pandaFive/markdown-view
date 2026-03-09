use crate::renderer::html_escape;

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
