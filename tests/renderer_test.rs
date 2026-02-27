use markdown_view::renderer::{
    generate_unique_id, render_markdown, slugify, syntax_theme_css, validate_theme,
};
use markdown_view::toc::generate_toc;

#[test]
fn test_基本パラグラフ() {
    let html = render_markdown("Hello, world!");
    assert!(html.as_str().contains("<p>Hello, world!</p>"));
}

#[test]
fn test_太字と斜体() {
    let html = render_markdown("**bold** and *italic*");
    assert!(html.as_str().contains("<strong>bold</strong>"));
    assert!(html.as_str().contains("<em>italic</em>"));
}

#[test]
fn test_gfmテーブル() {
    let md = "| Name | Age |\n|------|-----|\n| Alice | 30 |";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<table>"));
    assert!(html.as_str().contains("<th>Name</th>"));
    assert!(html.as_str().contains("<td>Alice</td>"));
}

#[test]
fn test_テーブルalignmentが反映される() {
    let md = "| L | C | R |\n|:--|:-:|--:|\n| 1 | 2 | 3 |";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<th class=\"align-left\">L</th>"));
    assert!(html.as_str().contains("<th class=\"align-center\">C</th>"));
    assert!(html.as_str().contains("<th class=\"align-right\">R</th>"));
    assert!(html.as_str().contains("<td class=\"align-left\">1</td>"));
    assert!(html.as_str().contains("<td class=\"align-center\">2</td>"));
    assert!(html.as_str().contains("<td class=\"align-right\">3</td>"));
}

#[test]
fn test_タスクリスト() {
    let md = "- [x] Done\n- [ ] Todo";
    let html = render_markdown(md);
    assert!(html.as_str().contains("checkbox"));
    assert!(html.as_str().contains("checked"));
    assert!(html.as_str().contains("Todo"));
}

#[test]
fn test_取消線() {
    let md = "~~deleted~~";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<del>deleted</del>"));
}

#[test]
fn test_コードブロック_ハイライト() {
    let md = "```rust\nfn main() {}\n```";
    let html = render_markdown(md);
    // syntectによるclass-basedハイライトが適用される
    assert!(html.as_str().contains("<pre"));
    assert!(html.as_str().contains("class=\"syn-code language-rust\""));
    assert!(html.as_str().contains("fn"));
}

#[test]
fn test_コードハイライトはクラスベースでインラインstyleを出力しない() {
    let md = "```rust\nfn main() {\n    println!(\"hi\");\n}\n```";
    let html = render_markdown(md);
    assert!(html.as_str().contains("class=\"syn-code language-rust\""));
    assert!(html.as_str().contains("class=\"syn-"));
    assert!(!html.as_str().contains("style=\""));
}

#[test]
fn test_未知言語コードブロックはフォールバック描画される() {
    let md = "```unknown-lang\nlet x = 1;\n```";
    let html = render_markdown(md);
    assert!(html
        .as_str()
        .contains("<pre class=\"code-block\"><code class=\"syn-code language-unknown-lang\">"));
    assert!(html.as_str().contains("let x = 1;"));
}

#[test]
fn test_インラインコード() {
    let md = "Use `println!` macro";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<code>println!</code>"));
}

#[test]
fn test_空入力() {
    let html = render_markdown("");
    assert!(html.as_str().is_empty() || html.as_str().trim().is_empty());
}

#[test]
fn test_見出し() {
    let md = "# Title\n## Subtitle";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<h1"));
    assert!(html.as_str().contains("<h2"));
}

#[test]
fn test_リンク() {
    let md = "[Rust](https://www.rust-lang.org)";
    let html = render_markdown(md);
    assert!(html.as_str().contains("href="));
    assert!(html.as_str().contains("https://www.rust-lang.org"));
    assert!(html.as_str().contains("Rust"));
}

#[test]
fn test_順序付きリスト() {
    let md = "1. first\n2. second";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<ol start=\"1\">"));
    assert!(html.as_str().contains("<li>first</li>"));
    assert!(html.as_str().contains("<li>second</li>"));
}

#[test]
fn test_画像() {
    let md = "![alt text](image.png)";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<img"));
    assert!(html.as_str().contains("src="));
    assert!(html.as_str().contains("image.png"));
    assert!(html.as_str().contains("alt text"));
}

#[test]
fn test_引用ブロック() {
    let md = "> This is a quote";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<blockquote>"));
}

#[test]
fn test_水平線() {
    let md = "---";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<hr"));
}

#[test]
fn test_rawhtml無効化() {
    // XSS防止: raw HTMLがそのままレンダリングされないことを確認
    let md = "<script>alert('xss')</script>";
    let html = render_markdown(md);
    assert!(!html.as_str().contains("<script>"));
}

#[test]
fn test_見出しにidが付与される() {
    let md = "# Hello World";
    let html = render_markdown(md);
    assert!(html.as_str().contains(r##"id="hello-world""##));
}

#[test]
fn test_画像alt属性で属性注入されない() {
    let md = r#"![x" onerror="alert(1)](img.png)"#;
    let html = render_markdown(md);
    assert!(html
        .as_str()
        .contains(r##"<img src="img.png" alt="x&quot; onerror=&quot;alert(1)" />"##));
    assert!(!html
        .as_str()
        .contains(r##"<img src="img.png" alt="x" onerror="alert(1)""##));
}

#[test]
fn test_言語指定ありコードブロック終了後のテキストが吸い込まれない() {
    let md = "```rust\nfn main() {}\n```\nAfter";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<pre"));
    assert!(html.as_str().contains("fn"));
    assert!(html.as_str().contains("<p>After</p>"));
}

#[test]
fn test_言語指定なしコードブロックも正しく扱う() {
    let md = "```\nplain\n```\nAfter";
    let html = render_markdown(md);
    assert!(html
        .as_str()
        .contains("<pre class=\"code-block\"><code class=\"syn-code\">plain"));
    assert!(html.as_str().contains("<p>After</p>"));
}

#[test]
fn test_見出し内インライン装飾が見出し要素内に収まる() {
    let md = "# Heading with *em* and `code`";
    let html = render_markdown(md);
    assert!(!html.as_str().contains("<em></em><h1"));
    assert!(html.as_str().contains(
        r##"<h1 id="heading-with-em-and-code">Heading with <em>em</em> and <code>code</code></h1>"##
    ));
}

#[test]
fn test_見出しidとtocリンクがインラインコード付き見出しで一致する() {
    let md = "# Title `x`";
    let html = render_markdown(md);
    let toc = generate_toc(md);
    assert!(html.as_str().contains(r##"id="title-x""##));
    assert!(toc.as_str().contains(r##"href="#title-x""##));
}

#[test]
fn test_複数テーブルでもヘッダセル閉じタグが壊れない() {
    let md = "| A | B |\n|---|---|\n| 1 | 2 |\n\n| C | D |\n|---|---|\n| 3 | 4 |";
    let html = render_markdown(md);
    assert!(html.as_str().contains("<th>A</th>"));
    assert!(html.as_str().contains("<th>C</th>"));
    assert!(!html.as_str().contains("<th>C</td>"));
}

#[test]
fn test_unsafeスキームのリンクは無効化される() {
    let md = "[click](javascript:alert(1))";
    let html = render_markdown(md);
    assert!(html.as_str().contains(r##"<a href="#">click</a>"##));
    assert!(!html.as_str().contains("javascript:alert(1)"));
}

#[test]
fn test_sanitize_hrefのホワイトスペースパディング付き危険urlは無効化される() {
    let html = render_markdown("[click](  javascript:alert(1)  )");
    assert!(html.as_str().contains(r##"<a href="#">click</a>"##));
    assert!(!html.as_str().contains("javascript:alert(1)"));
}

#[test]
fn test_見出し画像入りでもtocリンクが一致する() {
    let md = "# ![logo](x.png) Title";
    let html = render_markdown(md);
    let toc = generate_toc(md);
    assert!(html.as_str().contains(r##"id="title""##));
    assert!(toc.as_str().contains(r##"href="#title""##));
    assert!(!toc.as_str().contains(r##"href="#logo-title""##));
}

#[test]
fn test_テーマ指定でハイライト出力が変わる() {
    let dark = syntax_theme_css(Some("base16-ocean.dark"));
    let light = syntax_theme_css(Some("InspiredGitHub"));
    assert_ne!(dark, light);
}

#[test]
fn test_syntax_theme_css_noneはデフォルトテーマで非空cssを返す() {
    let css = syntax_theme_css(None);
    assert!(!css.trim().is_empty());
    assert!(css.contains(".syn-"));
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
    let html = render_markdown(md);
    let toc = generate_toc(md);
    assert!(html.as_str().contains(r##"<h1 id="section">!!!</h1>"##));
    assert!(html.as_str().contains(r##"<h1 id="section-1">---</h1>"##));
    assert!(toc.as_str().contains(r##"href="#section""##));
    assert!(toc.as_str().contains(r##"href="#section-1""##));
}

#[test]
fn test_複数行見出しでもtocリンクが一致する() {
    let md = "hello\nworld\n===";
    let html = render_markdown(md);
    let toc = generate_toc(md);
    assert!(html.as_str().contains(r##"id="hello-world""##));
    assert!(toc.as_str().contains(r##"href="#hello-world""##));
}

#[test]
fn test_危険なスキームのリンクがすべて無効化される() {
    // data: スキーム
    let html = render_markdown("[click](data:text/html,<script>alert(1)</script>)");
    assert!(html.as_str().contains(r##"href="#""##));
    assert!(!html.as_str().contains("data:text/html"));

    // vbscript: スキーム
    let html = render_markdown("[click](vbscript:msgbox)");
    assert!(html.as_str().contains(r##"href="#""##));
    assert!(!html.as_str().contains("vbscript:"));

    // file: スキーム
    let html = render_markdown("[click](file:///etc/passwd)");
    assert!(html.as_str().contains(r##"href="#""##));
    assert!(!html.as_str().contains("file:///"));
}

#[test]
fn test_大文字混在スキームも無効化される() {
    let html = render_markdown("[click](JAVASCRIPT:alert(1))");
    assert!(html.as_str().contains(r##"href="#""##));
    assert!(!html.as_str().contains("JAVASCRIPT:"));

    let html = render_markdown("[click](JaVaScRiPt:alert(1))");
    assert!(html.as_str().contains(r##"href="#""##));
    assert!(!html.as_str().contains("JaVaScRiPt:"));
}

#[test]
fn test_画像srcもunsafeスキームが無効化される() {
    let html = render_markdown("![img](javascript:alert(1))");
    assert!(html.as_str().contains(r##"src="#""##));
    assert!(!html.as_str().contains("javascript:alert"));
}

#[test]
fn test_html_escapeでシングルクォートがエスケープされる() {
    use markdown_view::renderer::html_escape;
    assert_eq!(html_escape("It's"), "It&#39;s");
    assert_eq!(html_escape("a'b\"c"), "a&#39;b&quot;c");
}

#[test]
fn test_html_escapeで主要な特殊文字がエスケープされる() {
    use markdown_view::renderer::html_escape;
    assert_eq!(
        html_escape("&<>'\"そのまま"),
        "&amp;&lt;&gt;&#39;&quot;そのまま"
    );
}

#[test]
fn test_安全なリンクスキームは許可される() {
    let html = render_markdown("[mail](mailto:user@example.com)");
    assert!(html.as_str().contains("mailto:user@example.com"));

    let html = render_markdown("[tel](tel:+1234567890)");
    assert!(html.as_str().contains("tel:+1234567890"));

    let html = render_markdown("[link](https://example.com)");
    assert!(html.as_str().contains("https://example.com"));

    // 相対パス
    let html = render_markdown("[link](./page.html)");
    assert!(html.as_str().contains("./page.html"));

    // アンカー
    let html = render_markdown("[link](#section)");
    assert!(html.as_str().contains("#section"));
}

#[test]
fn test_画像srcのdata_スキームが無効化される() {
    let html = render_markdown("![img](data:image/png;base64,abc)");
    assert!(html.as_str().contains(r##"src="#""##));
    assert!(!html.as_str().contains("data:image/png"));
}

#[test]
fn test_プロトコル相対urlが無効化される() {
    let html = render_markdown("[click](//evil.example/path)");
    assert!(html.as_str().contains(r##"href="#""##));
    assert!(!html.as_str().contains("//evil.example"));

    let html = render_markdown("![img](//evil.example/img.png)");
    assert!(html.as_str().contains(r##"src="#""##));
    assert!(!html.as_str().contains("//evil.example"));
}

#[test]
fn test_ローカルルートパスのリンクは許可される() {
    let html = render_markdown("[link](/page.html)");
    assert!(html.as_str().contains(r##"href="/page.html""##));
}

#[test]
fn test_空hrefはフォールバックされる() {
    let html = render_markdown("[empty]()");
    assert!(html.as_str().contains(r##"<a href="#">empty</a>"##));
}

#[test]
fn test_slugify_直接テスト_unicode_onlyと空入力() {
    assert_eq!(slugify("日本語のみ"), "日本語のみ");
    assert_eq!(slugify(""), "section");
    assert_eq!(slugify("!!!"), "section");
}

#[test]
fn test_slugify_直接テスト_連続記号は単一ハイフンに正規化される() {
    assert_eq!(slugify("A---B___C"), "a-b-c");
}

#[test]
fn test_generate_unique_id_直接テスト_重複時に連番を付与する() {
    let mut counts = std::collections::HashMap::new();
    assert_eq!(generate_unique_id("section", &mut counts), "section");
    assert_eq!(generate_unique_id("section", &mut counts), "section-1");
    assert_eq!(generate_unique_id("section", &mut counts), "section-2");
}

#[test]
fn test_フルパイプラインxss対策_render_markdownからrender_pageまで() {
    use markdown_view::renderer::syntax_theme_css;
    use markdown_view::template::{render_page, RenderPageParams, SidebarParams};
    use markdown_view::toc::generate_toc;

    let content =
        render_markdown("# Title\n<script>alert('xss')</script>\n[bad](javascript:alert(1))");
    let toc = generate_toc("# Title");
    let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
    let html = render_page(RenderPageParams {
        title: "Test",
        content: &content,
        toc: &toc,
        dark_mode: false,
        syntax_css: &syntax_css,
        sidebar: SidebarParams::SingleFile,
    });

    assert!(!html.as_str().contains("<script>alert('xss')</script>"));
    assert!(html.as_str().contains(r##"<a href="#">bad</a>"##));
}

// --- テンプレート テスト ---

#[test]
fn test_render_pageのタイトルがエスケープされる() {
    use markdown_view::renderer::{render_markdown, syntax_theme_css};
    use markdown_view::template::{render_page, RenderPageParams, SidebarParams};
    use markdown_view::toc::generate_toc;
    let content = render_markdown("xss");
    let toc = generate_toc("# t");
    let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
    let html = render_page(RenderPageParams {
        title: "<script>xss</script>",
        content: &content,
        toc: &toc,
        dark_mode: false,
        syntax_css: &syntax_css,
        sidebar: SidebarParams::SingleFile,
    });
    assert!(html.as_str().contains("&lt;script&gt;xss&lt;/script&gt;"));
    assert!(!html
        .as_str()
        .contains("<script>xss</script> - markdown-view"));
}

#[test]
fn test_render_pageのダークモード() {
    use markdown_view::renderer::{render_markdown, syntax_theme_css};
    use markdown_view::template::{render_page, RenderPageParams, SidebarParams};
    use markdown_view::toc::generate_toc;
    let content = render_markdown("x");
    let toc = generate_toc("# t");
    let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
    let light = render_page(RenderPageParams {
        title: "t",
        content: &content,
        toc: &toc,
        dark_mode: false,
        syntax_css: &syntax_css,
        sidebar: SidebarParams::SingleFile,
    });
    let dark = render_page(RenderPageParams {
        title: "t",
        content: &content,
        toc: &toc,
        dark_mode: true,
        syntax_css: &syntax_css,
        sidebar: SidebarParams::SingleFile,
    });
    assert!(light.contains(r#"data-theme="light""#));
    assert!(dark.contains(r#"data-theme="dark""#));
}

#[test]
fn test_render_pageの基本構造() {
    use markdown_view::renderer::{render_markdown, syntax_theme_css};
    use markdown_view::template::{render_page, RenderPageParams, SidebarParams};
    use markdown_view::toc::generate_toc;
    let content = render_markdown("Hello");
    let toc = generate_toc("# H1");
    let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
    let html = render_page(RenderPageParams {
        title: "Test",
        content: &content,
        toc: &toc,
        dark_mode: false,
        syntax_css: &syntax_css,
        sidebar: SidebarParams::SingleFile,
    });
    assert!(html.as_str().contains("<!DOCTYPE html>"));
    assert!(html.as_str().contains("<p>Hello</p>"));
    assert!(html.as_str().contains("<a href=\"#h1\">H1</a>"));
    assert!(html.as_str().contains("Test - markdown-view"));
}
