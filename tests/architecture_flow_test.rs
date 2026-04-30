use markdown_view::architecture::{application_flow, FlowNode};

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
    let startup = flow
        .find("起動フロー")
        .expect("起動フロー node should exist");

    assert!(
        startup
            .descendants()
            .iter()
            .any(|node| node.module() == Some("src/main.rs") && node.function() == Some("main")),
        "起動フローは main.rs の main を参照する必要がある"
    );
    assert!(
        flow.descendants().iter().any(|node| {
            node.module() == Some("src/renderer/render.rs") && node.function() == Some("render")
        }),
        "レンダリング処理は renderer::render を参照する必要がある"
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
