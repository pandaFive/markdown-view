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
            _ => panic!("docs ディレクトリを期待"),
        }
    }
}
