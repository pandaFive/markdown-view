use markdown_view::renderer::render_markdown;
use markdown_view::toc::generate_toc;

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

#[test]
fn test_画像alt属性で属性注入されない() {
    let md = r#"![x" onerror="alert(1)](img.png)"#;
    let html = render_markdown(md, None);
    assert!(html.contains(r##"<img src="img.png" alt="x&quot; onerror=&quot;alert(1)" />"##));
    assert!(!html.contains(r##"<img src="img.png" alt="x" onerror="alert(1)""##));
}

#[test]
fn test_言語指定ありコードブロック終了後のテキストが吸い込まれない() {
    let md = "```rust\nfn main() {}\n```\nAfter";
    let html = render_markdown(md, None);
    assert!(html.contains("<pre"));
    assert!(html.contains("fn"));
    assert!(html.contains("<p>After</p>"));
}

#[test]
fn test_言語指定なしコードブロックも正しく扱う() {
    let md = "```\nplain\n```\nAfter";
    let html = render_markdown(md, None);
    assert!(html.contains("<pre class=\"code-block\"><code>plain"));
    assert!(html.contains("<p>After</p>"));
}

#[test]
fn test_見出し内インライン装飾が見出し要素内に収まる() {
    let md = "# Heading with *em* and `code`";
    let html = render_markdown(md, None);
    assert!(!html.contains("<em></em><h1"));
    assert!(html.contains(
        r##"<h1 id="heading-with-em-and-code">Heading with <em>em</em> and <code>code</code></h1>"##
    ));
}

#[test]
fn test_見出しidとtocリンクがインラインコード付き見出しで一致する() {
    let md = "# Title `x`";
    let html = render_markdown(md, None);
    let toc = generate_toc(md);
    assert!(html.contains(r##"id="title-x""##));
    assert!(toc.contains(r##"href="#title-x""##));
}

#[test]
fn test_複数テーブルでもヘッダセル閉じタグが壊れない() {
    let md = "| A | B |\n|---|---|\n| 1 | 2 |\n\n| C | D |\n|---|---|\n| 3 | 4 |";
    let html = render_markdown(md, None);
    assert!(html.contains("<th>A</th>"));
    assert!(html.contains("<th>C</th>"));
    assert!(!html.contains("<th>C</td>"));
}

#[test]
fn test_unsafeスキームのリンクは無効化される() {
    let md = "[click](javascript:alert(1))";
    let html = render_markdown(md, None);
    assert!(html.contains(r##"<a href="#">click</a>"##));
    assert!(!html.contains("javascript:alert(1)"));
}

#[test]
fn test_見出し画像入りでもtocリンクが一致する() {
    let md = "# ![logo](x.png) Title";
    let html = render_markdown(md, None);
    let toc = generate_toc(md);
    assert!(html.contains(r##"id="title""##));
    assert!(toc.contains(r##"href="#title""##));
    assert!(!toc.contains(r##"href="#logo-title""##));
}

#[test]
fn test_テーマ指定でハイライト出力が変わる() {
    let md = "```rust\nfn main() {}\n```";
    let dark = render_markdown(md, Some("base16-ocean.dark"));
    let light = render_markdown(md, Some("InspiredGitHub"));
    assert_ne!(dark, light);
}

#[test]
fn test_空スラッグ見出しにフォールバックidを付与する() {
    let md = "# !!!\n# ---";
    let html = render_markdown(md, None);
    let toc = generate_toc(md);
    assert!(html.contains(r##"<h1 id="section">!!!</h1>"##));
    assert!(html.contains(r##"<h1 id="section-1">---</h1>"##));
    assert!(toc.contains(r##"href="#section""##));
    assert!(toc.contains(r##"href="#section-1""##));
}
