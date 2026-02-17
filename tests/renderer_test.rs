use markdown_view::renderer::render_markdown;

#[test]
fn test_基本パラグラフ() {
    let html = render_markdown("Hello, world!", None);
    assert!(html.contains("<p>Hello, world!</p>"));
}

#[test]
fn test_太字と斜体() {
    let html = render_markdown("**bold** and *italic*", None);
    assert!(html.contains("<strong>bold</strong>"));
    assert!(html.contains("<em>italic</em>"));
}

#[test]
fn test_gfmテーブル() {
    let md = "| Name | Age |\n|------|-----|\n| Alice | 30 |";
    let html = render_markdown(md, None);
    assert!(html.contains("<table>"));
    assert!(html.contains("<th>Name</th>"));
    assert!(html.contains("<td>Alice</td>"));
}

#[test]
fn test_タスクリスト() {
    let md = "- [x] Done\n- [ ] Todo";
    let html = render_markdown(md, None);
    assert!(html.contains("checkbox"));
    assert!(html.contains("checked"));
    assert!(html.contains("Todo"));
}

#[test]
fn test_取消線() {
    let md = "~~deleted~~";
    let html = render_markdown(md, None);
    assert!(html.contains("<del>deleted</del>"));
}

#[test]
fn test_コードブロック_ハイライト() {
    let md = "```rust\nfn main() {}\n```";
    let html = render_markdown(md, None);
    // syntectによるclass-basedハイライトが適用される
    assert!(html.contains("<pre"));
    assert!(html.contains("fn"));
}

#[test]
fn test_インラインコード() {
    let md = "Use `println!` macro";
    let html = render_markdown(md, None);
    assert!(html.contains("<code>println!</code>"));
}

#[test]
fn test_空入力() {
    let html = render_markdown("", None);
    assert!(html.is_empty() || html.trim().is_empty());
}

#[test]
fn test_見出し() {
    let md = "# Title\n## Subtitle";
    let html = render_markdown(md, None);
    assert!(html.contains("<h1"));
    assert!(html.contains("<h2"));
}

#[test]
fn test_リンク() {
    let md = "[Rust](https://www.rust-lang.org)";
    let html = render_markdown(md, None);
    assert!(html.contains("href="));
    assert!(html.contains("https://www.rust-lang.org"));
    assert!(html.contains("Rust"));
}

#[test]
fn test_画像() {
    let md = "![alt text](image.png)";
    let html = render_markdown(md, None);
    assert!(html.contains("<img"));
    assert!(html.contains("src="));
    assert!(html.contains("image.png"));
    assert!(html.contains("alt text"));
}

#[test]
fn test_引用ブロック() {
    let md = "> This is a quote";
    let html = render_markdown(md, None);
    assert!(html.contains("<blockquote>"));
}

#[test]
fn test_水平線() {
    let md = "---";
    let html = render_markdown(md, None);
    assert!(html.contains("<hr"));
}

#[test]
fn test_rawhtml無効化() {
    // XSS防止: raw HTMLがそのままレンダリングされないことを確認
    let md = "<script>alert('xss')</script>";
    let html = render_markdown(md, None);
    assert!(!html.contains("<script>"));
}

#[test]
fn test_見出しにidが付与される() {
    let md = "# Hello World";
    let html = render_markdown(md, None);
    assert!(html.contains(r##"id="hello-world""##));
}
