use markdown_view::architecture::{application_flow, FlowNode};

fn docs_mermaid_block() -> String {
    let docs = include_str!("../docs/architecture-flow.md");
    let start_marker = "```mermaid\n";
    let start = docs
        .find(start_marker)
        .expect("architecture-flow.md should contain a mermaid block")
        + start_marker.len();
    let rest = &docs[start..];
    let end = rest
        .find("\n```")
        .expect("architecture-flow.md mermaid block should be closed");

    format!("{}\n", &rest[..end])
}

#[test]
fn test_処理フローastに主要4フローが含まれる() {
    let flow = application_flow();
    let names = flow.child_names();

    assert_eq!(
        names,
        vec![
            "起動フロー",
            "HTTPレンダリングフロー",
            "ファイル監視更新フロー",
            "ブラウザWebSocketフロー",
        ]
    );
}

#[test]
fn test_処理フローastはモジュール参照を保持する() {
    let flow = application_flow();
    let descendants = flow.descendants();

    assert!(
        descendants
            .iter()
            .any(|node| node.module() == Some("src/main.rs") && node.function() == Some("main")),
        "起動境界は main.rs の main を参照する必要がある"
    );
    assert!(
        descendants.iter().any(|node| {
            node.module() == Some("src/server/routes.rs")
                && node.function() == Some("create_router")
        }),
        "HTTP境界は routes.rs の create_router を参照する必要がある"
    );
    assert!(
        descendants.iter().any(|node| {
            node.module() == Some("src/renderer/mod.rs")
                && node.function() == Some("render_markdown")
        }),
        "Markdownレンダリング境界は renderer::mod.rs の render_markdown を参照する必要がある"
    );
    assert!(
        descendants.iter().any(|node| {
            node.module() == Some("src/server/broadcast.rs")
                && node.function() == Some("notify_update")
        }),
        "更新通知境界は broadcast.rs の notify_update を参照する必要がある"
    );
}

#[test]
fn test_httpレンダリングフローはhost検証として表現する() {
    let flow = application_flow();
    let http = flow
        .find("HTTPレンダリングフロー")
        .expect("HTTPレンダリングフロー node should exist");

    assert!(http.child_names().contains(&"Host検証"));
    assert!(!http.child_names().contains(&"Host/Origin検証"));
}

#[test]
fn test_処理フローastをmermaidに変換できる() {
    let mermaid = application_flow().to_mermaid();

    assert!(mermaid.starts_with("flowchart TD\n"));
    assert!(mermaid.contains("[\"markdown-view\"]"));
    assert!(mermaid.contains("[\"起動フロー\"]"));
    assert!(mermaid.contains("[\"HTTPレンダリングフロー\"]"));
    assert!(mermaid.contains("[\"render_markdown\"]"));
    assert!(mermaid.contains("-->"));
}

#[test]
fn test_docs_mermaidはarchitecture_ast出力と一致する() {
    assert_eq!(docs_mermaid_block(), application_flow().to_mermaid());
}

#[test]
fn test_ノード検索は子孫までたどる() {
    let flow = FlowNode::group(
        "root",
        vec![FlowNode::group(
            "child",
            vec![FlowNode::step("target", "m", None)],
        )],
    );

    assert_eq!(flow.find("target").map(FlowNode::name), Some("target"));
}
