use markdown_view::renderer::{render_markdown, validate_theme};
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
fn test_テーブルalignmentが反映される() {
    let md = "| L | C | R |\n|:--|:-:|--:|\n| 1 | 2 | 3 |";
    let html = render_markdown(md, None);
    assert!(html.contains("<th style=\"text-align:left\">L</th>"));
    assert!(html.contains("<th style=\"text-align:center\">C</th>"));
    assert!(html.contains("<th style=\"text-align:right\">R</th>"));
    assert!(html.contains("<td style=\"text-align:left\">1</td>"));
    assert!(html.contains("<td style=\"text-align:center\">2</td>"));
    assert!(html.contains("<td style=\"text-align:right\">3</td>"));
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
fn test_有効なテーマ名の検証が成功する() {
    assert!(validate_theme("base16-ocean.dark").is_ok());
    assert!(validate_theme("InspiredGitHub").is_ok());
}

#[test]
fn test_無効なテーマ名の検証が利用可能テーマ一覧を返す() {
    let result = validate_theme("nonexistent-theme");
    assert!(result.is_err());
    let available = result.unwrap_err();
    assert!(!available.is_empty());
    assert!(available.iter().any(|t| t == "base16-ocean.dark"));
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

#[test]
fn test_複数行見出しでもtocリンクが一致する() {
    let md = "hello\nworld\n===";
    let html = render_markdown(md, None);
    let toc = generate_toc(md);
    assert!(html.contains(r##"id="hello-world""##));
    assert!(toc.contains(r##"href="#hello-world""##));
}

#[test]
fn test_危険なスキームのリンクがすべて無効化される() {
    // data: スキーム
    let html = render_markdown("[click](data:text/html,<script>alert(1)</script>)", None);
    assert!(html.contains(r##"href="#""##));
    assert!(!html.contains("data:text/html"));

    // vbscript: スキーム
    let html = render_markdown("[click](vbscript:msgbox)", None);
    assert!(html.contains(r##"href="#""##));
    assert!(!html.contains("vbscript:"));

    // file: スキーム
    let html = render_markdown("[click](file:///etc/passwd)", None);
    assert!(html.contains(r##"href="#""##));
    assert!(!html.contains("file:///"));
}

#[test]
fn test_大文字混在スキームも無効化される() {
    let html = render_markdown("[click](JAVASCRIPT:alert(1))", None);
    assert!(html.contains(r##"href="#""##));
    assert!(!html.contains("JAVASCRIPT:"));

    let html = render_markdown("[click](JaVaScRiPt:alert(1))", None);
    assert!(html.contains(r##"href="#""##));
    assert!(!html.contains("JaVaScRiPt:"));
}

#[test]
fn test_画像srcもunsafeスキームが無効化される() {
    let html = render_markdown("![img](javascript:alert(1))", None);
    assert!(html.contains(r##"src="#""##));
    assert!(!html.contains("javascript:alert"));
}

#[test]
fn test_html_escapeでシングルクォートがエスケープされる() {
    use markdown_view::renderer::html_escape;
    assert_eq!(html_escape("It's"), "It&#39;s");
    assert_eq!(html_escape("a'b\"c"), "a&#39;b&quot;c");
}

#[test]
fn test_安全なリンクスキームは許可される() {
    let html = render_markdown("[mail](mailto:user@example.com)", None);
    assert!(html.contains("mailto:user@example.com"));

    let html = render_markdown("[tel](tel:+1234567890)", None);
    assert!(html.contains("tel:+1234567890"));

    let html = render_markdown("[link](https://example.com)", None);
    assert!(html.contains("https://example.com"));

    // 相対パス
    let html = render_markdown("[link](./page.html)", None);
    assert!(html.contains("./page.html"));

    // アンカー
    let html = render_markdown("[link](#section)", None);
    assert!(html.contains("#section"));
}

#[test]
fn test_画像srcのdata_スキームが無効化される() {
    let html = render_markdown("![img](data:image/png;base64,abc)", None);
    assert!(html.contains(r##"src="#""##));
    assert!(!html.contains("data:image/png"));
}

#[test]
fn test_プロトコル相対urlが無効化される() {
    let html = render_markdown("[click](//evil.example/path)", None);
    assert!(html.contains(r##"href="#""##));
    assert!(!html.contains("//evil.example"));

    let html = render_markdown("![img](//evil.example/img.png)", None);
    assert!(html.contains(r##"src="#""##));
    assert!(!html.contains("//evil.example"));
}

#[test]
fn test_ローカルルートパスのリンクは許可される() {
    let html = render_markdown("[link](/page.html)", None);
    assert!(html.contains(r##"href="/page.html""##));
}

// --- テンプレート テスト ---

#[test]
fn test_render_pageのタイトルがエスケープされる() {
    use markdown_view::template::{render_page, RenderPageParams};
    let html = render_page(RenderPageParams {
        title: "<script>xss</script>",
        content: "",
        toc: "",
        dark_mode: false,
        file_list: None,
        current_file: None,
    });
    assert!(html.contains("&lt;script&gt;xss&lt;/script&gt;"));
    assert!(!html.contains("<script>xss</script> - markdown-view"));
}

#[test]
fn test_render_pageのダークモード() {
    use markdown_view::template::{render_page, RenderPageParams};
    let light = render_page(RenderPageParams {
        title: "t",
        content: "",
        toc: "",
        dark_mode: false,
        file_list: None,
        current_file: None,
    });
    let dark = render_page(RenderPageParams {
        title: "t",
        content: "",
        toc: "",
        dark_mode: true,
        file_list: None,
        current_file: None,
    });
    assert!(light.contains(r#"data-theme="light""#));
    assert!(dark.contains(r#"data-theme="dark""#));
}

#[test]
fn test_render_pageの基本構造() {
    use markdown_view::template::{render_page, RenderPageParams};
    let html = render_page(RenderPageParams {
        title: "Test",
        content: "<p>Hello</p>",
        toc: "<ul><li>H1</li></ul>",
        dark_mode: false,
        file_list: None,
        current_file: None,
    });
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("<p>Hello</p>"));
    assert!(html.contains("<ul><li>H1</li></ul>"));
    assert!(html.contains("Test - markdown-view"));
}
