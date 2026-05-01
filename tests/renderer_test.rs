use markdown_view::renderer::{
    extract_headings, generate_unique_id, render_document, render_markdown, slugify,
    syntax_theme_css, validate_theme,
};
use markdown_view::toc::generate_toc;

fn normalize_source_markup(html: &str) -> String {
    let mut normalized = html.to_string();

    while let Some(start) = normalized.find("<span data-source-start-line=\"") {
        let end = normalized[start..]
            .find('>')
            .map(|offset| start + offset + 1)
            .expect("source span should have closing angle bracket");
        normalized.replace_range(start..end, "");
    }

    while let Some(start) = normalized.find(" data-source-start-line=\"") {
        let end_attr = " data-source-end-line=\"";
        let second_attr_start = normalized[start..]
            .find(end_attr)
            .map(|offset| start + offset)
            .expect("end line attribute should exist");
        let value_start = second_attr_start + end_attr.len();
        let end = normalized[value_start..]
            .find('"')
            .map(|offset| value_start + offset + 1)
            .expect("end line attribute should close");
        normalized.replace_range(start..end, "");
    }

    while let Some(start) = normalized.find(" data-line-block-start=\"") {
        let end_attr = " data-line-block-end=\"";
        let second_attr_start = normalized[start..]
            .find(end_attr)
            .map(|offset| start + offset)
            .expect("data-line-block-end attribute should exist when -start exists");
        let value_start = second_attr_start + end_attr.len();
        let end = normalized[value_start..]
            .find('"')
            .map(|offset| value_start + offset + 1)
            .expect("data-line-block-end attribute should close");
        normalized.replace_range(start..end, "");
    }

    normalized = normalized.replace(" data-line-block", "");

    normalized.replace("</span>", "")
}

fn source_line_attrs(html: &str, tag_name: &str) -> Option<(usize, usize)> {
    let needle = format!("<{tag_name}");
    let start = html.find(&needle)?;
    let end = html[start..].find('>').map(|offset| start + offset)?;
    let tag = &html[start..=end];

    let start_line = extract_attr_value(tag, "data-source-start-line")?
        .parse()
        .ok()?;
    let end_line = extract_attr_value(tag, "data-source-end-line")?
        .parse()
        .ok()?;
    Some((start_line, end_line))
}

fn extract_attr_value<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!(r#"{attr}=""#);
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"').map(|offset| start + offset)?;
    Some(&tag[start..end])
}

#[test]
fn test_基本パラグラフ() {
    let html = normalize_source_markup(render_markdown("Hello, world!").as_str());
    assert!(html.contains("<p>Hello, world!</p>"));
}

#[test]
fn test_太字と斜体() {
    let html = normalize_source_markup(render_markdown("**bold** and *italic*").as_str());
    assert!(html.contains("<strong>bold</strong>"));
    assert!(html.contains("<em>italic</em>"));
}

#[test]
fn test_gfmテーブル() {
    let md = "| Name | Age |\n|------|-----|\n| Alice | 30 |";
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(html.contains("<table>"));
    assert!(html.contains("<th>Name</th>"));
    assert!(html.contains("<td>Alice</td>"));
}

#[test]
fn test_テーブルalignmentが反映される() {
    let md = "| L | C | R |\n|:--|:-:|--:|\n| 1 | 2 | 3 |";
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(html.contains("<th class=\"align-left\">L</th>"));
    assert!(html.contains("<th class=\"align-center\">C</th>"));
    assert!(html.contains("<th class=\"align-right\">R</th>"));
    assert!(html.contains("<td class=\"align-left\">1</td>"));
    assert!(html.contains("<td class=\"align-center\">2</td>"));
    assert!(html.contains("<td class=\"align-right\">3</td>"));
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
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(html.contains("<del>deleted</del>"));
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
fn test_コードブロックにソース行番号属性が付与される() {
    let md = "```rust\nfn main() {}\n```";
    let html = render_markdown(md);
    assert!(html.as_str().contains(
        r#"<pre class="code-block" data-line-block data-source-start-line="1" data-source-end-line="3">"#
    ));
}

#[test]
fn test_コードブロック直後の見出しもline_block属性を維持する() {
    let md = "```unknown-lang\n<x>\n```\n\n# Next";
    let html = render_markdown(md);

    assert!(html.as_str().contains(
        r#"<pre class="code-block" data-line-block data-source-start-line="1" data-source-end-line="3"><code class="syn-code language-unknown-lang">&lt;x&gt;"#
    ));
    assert!(html.as_str().contains(
        r#"<h1 id="next" data-line-block data-source-start-line="5" data-source-end-line="5">"#
    ));
}

#[test]
fn test_ソース行番号属性の値が複数行入力でも正確() {
    let md = "# Heading\n\nLine one\nLine two\n\n```rust\nfn main() {}\nprintln!(\"x\");\n```";
    let html = render_markdown(md);

    assert_eq!(source_line_attrs(html.as_str(), "h1"), Some((1, 1)));
    assert!(html
        .as_str()
        .contains(r#"<span data-source-start-line="3" data-source-end-line="3">Line one</span>"#));
    assert!(html
        .as_str()
        .contains(r#"<span data-source-start-line="4" data-source-end-line="4">Line two</span>"#));
    assert_eq!(source_line_attrs(html.as_str(), "pre"), Some((6, 9)));
}

#[test]
fn test_段落にdata_line_block属性が付与される() {
    let html = render_markdown("Hello paragraph");
    assert!(
        html.as_str().contains("<p data-line-block"),
        "段落 <p> に data-line-block 属性が付くこと: {}",
        html.as_str()
    );
}

#[test]
fn test_見出しにdata_line_block属性が付与される() {
    let html = render_markdown("# 見出し");
    assert!(
        html.as_str().contains("data-line-block"),
        "見出し <h1> に data-line-block 属性が付くこと: {}",
        html.as_str()
    );
    // <h1 id="..." data-line-block ...> の順序確認（class位置はidの後）
    assert!(html.as_str().contains("<h1 id="));
}

#[test]
fn test_blockquoteにdata_line_block属性が付与される() {
    let html = render_markdown("> 引用");
    assert!(
        html.as_str().contains("<blockquote data-line-block"),
        "<blockquote> に data-line-block 属性が付くこと: {}",
        html.as_str()
    );
}

#[test]
fn test_ul要素にdata_line_block属性が付与される() {
    let html = render_markdown("- item1\n- item2");
    assert!(
        html.as_str().contains("<ul data-line-block"),
        "<ul> に data-line-block 属性が付くこと: {}",
        html.as_str()
    );
    assert!(
        html.as_str().contains("<li data-line-block"),
        "<li> に data-line-block 属性が付くこと: {}",
        html.as_str()
    );
}

#[test]
fn test_ol要素にdata_line_block属性が付与される() {
    let html = render_markdown("1. first\n2. second");
    assert!(
        html.as_str().contains("<ol start=\"1\" data-line-block"),
        "<ol> に data-line-block 属性が付くこと: {}",
        html.as_str()
    );
}

#[test]
fn test_table要素にdata_line_block属性が付与される() {
    let html = render_markdown("| a | b |\n|---|---|\n| 1 | 2 |");
    assert!(
        html.as_str().contains("<table data-line-block"),
        "<table> に data-line-block 属性が付くこと: {}",
        html.as_str()
    );
}

#[test]
fn test_インライン要素にはdata_line_blockが付かない() {
    // span / code / em / strong / a にはdata-line-blockが付かない
    let html =
        render_markdown("通常の**強調**と `コード` と *斜体* と [リンク](http://example.com)");
    let html_str = html.as_str();
    assert!(
        !html_str.contains("<span data-line-block"),
        "<span> にdata-line-blockが付いてはならない: {}",
        html_str
    );
    assert!(
        !html_str.contains("<code data-line-block"),
        "インライン <code> にdata-line-blockが付いてはならない: {}",
        html_str
    );
    assert!(
        !html_str.contains("<em data-line-block"),
        "<em> にdata-line-blockが付いてはならない: {}",
        html_str
    );
    assert!(
        !html_str.contains("<strong data-line-block"),
        "<strong> にdata-line-blockが付いてはならない: {}",
        html_str
    );
    assert!(
        !html_str.contains("<a data-line-block"),
        "<a> にdata-line-blockが付いてはならない: {}",
        html_str
    );
}

#[test]
fn test_blockコンテナにはdata_source_lineが付かない_quote選択範囲広がり防止() {
    // `<ul>` / `<table>` 等のコンテナ要素に `data-source-start-line`/`end-line` を付けると、
    // memo.js の getSelectionLineRange() が祖先範囲を拾い、引用 `Lx-Ly` が広がる回帰を起こす。
    // コンテナは `data-line-block-start`/`end` のみ持ち、`data-source-*` は付与しない。
    let html = render_markdown("- item1\n- item2");
    let html_str = html.as_str();
    assert!(
        !html_str.contains("<ul data-source-start-line"),
        "<ul> に data-source-start-line を付けてはならない（quote範囲広がり回帰防止）: {}",
        html_str
    );
    assert!(
        !html_str.contains("<li data-source-start-line"),
        "<li> に data-source-start-line を付けてはならない: {}",
        html_str
    );

    let table = render_markdown("| a |\n|---|\n| b |");
    let table_str = table.as_str();
    assert!(
        !table_str.contains("<table data-source-start-line"),
        "<table> に data-source-start-line を付けてはならない: {}",
        table_str
    );

    let para = render_markdown("Hello");
    assert!(
        !para.as_str().contains("<p data-source-start-line"),
        "<p> に data-source-start-line を付けてはならない: {}",
        para.as_str()
    );

    let bq = render_markdown("> quote");
    assert!(
        !bq.as_str().contains("<blockquote data-source-start-line"),
        "<blockquote> に data-source-start-line を付けてはならない: {}",
        bq.as_str()
    );
}

#[test]
fn test_blockコンテナはdata_line_block_start_endを持つ() {
    // ジャンプ先用の範囲属性。quote集計には混入しない独立attribute
    let html = render_markdown("Hello paragraph");
    assert!(
        html.as_str()
            .contains("<p data-line-block data-line-block-start=\"1\" data-line-block-end=\"1\""),
        "<p> は data-line-block-start/end を持つ: {}",
        html.as_str()
    );

    let list = render_markdown("- item");
    assert!(
        list.as_str()
            .contains("<ul data-line-block data-line-block-start=\"1\" data-line-block-end=\"1\""),
        "<ul> は data-line-block-start/end を持つ: {}",
        list.as_str()
    );
    assert!(
        list.as_str()
            .contains("<li data-line-block data-line-block-start=\"1\" data-line-block-end=\"1\""),
        "<li> は data-line-block-start/end を持つ: {}",
        list.as_str()
    );
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
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(
        html.contains("<pre class=\"code-block\"><code class=\"syn-code language-unknown-lang\">")
    );
    assert!(html.contains("let x = 1;"));
}

#[test]
fn test_インラインコード() {
    let md = "Use `println!` macro";
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(html.contains("<code>println!</code>"));
}

#[test]
fn test_空入力() {
    let html = render_markdown("");
    assert!(html.as_str().is_empty() || html.as_str().trim().is_empty());
}

#[test]
fn test_大量入力1mbでもパニックせず描画できる() {
    let mut md = String::with_capacity(1024 * 1024 + 64);
    md.push_str("# Large\n\n");
    md.push_str(&"a".repeat(1024 * 1024));

    let html = normalize_source_markup(render_markdown(&md).as_str());
    assert!(html.contains("<h1 id=\"large\">Large</h1>"));
    assert!(!html.is_empty());
}

#[test]
fn test_深いネスト500階層でもパニックせず描画できる() {
    let mut md = String::new();
    for _ in 0..500 {
        md.push_str("> ");
    }
    md.push_str("deep");

    let html = render_markdown(&md);
    assert!(html.as_str().contains("deep"));
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
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(html.contains("<ol start=\"1\">"));
    assert!(html.contains("<li>first</li>"));
    assert!(html.contains("<li>second</li>"));
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
fn test_画像alt内のsoftbreakとhardbreakは空白として扱う() {
    let soft_break =
        normalize_source_markup(render_markdown("![first\nsecond](./pic.png)").as_str());
    let hard_break =
        normalize_source_markup(render_markdown("![first  \nsecond](./pic.png)").as_str());

    assert_eq!(soft_break, hard_break);
    assert!(soft_break.contains(r#"<img src="./pic.png" alt="first second" />"#));
}

#[test]
fn test_外部画像urlは既定で無効化される() {
    let html = render_markdown("![remote](https://evil.example/track.png)");
    assert!(html.as_str().contains(r##"src="#""##));
    assert!(!html.as_str().contains("https://evil.example/track.png"));
}

#[test]
fn test_リンクは外部urlを許可し画像は拒否する() {
    let html = render_markdown("[click](https://example.com) ![img](https://example.com/pic.png)");
    // リンクhrefは外部URLを保持する
    assert!(html.as_str().contains(r#"href="https://example.com""#));
    // 画像srcは外部URLを無効化する
    assert!(html.as_str().contains(r##"src="#""##));
    assert!(!html.as_str().contains("https://example.com/pic.png"));
}

#[test]
fn test_画像srcのmailtoとtelスキームは拒否される() {
    let html_mailto = render_markdown("![img](mailto:test@example.com)");
    assert!(html_mailto.as_str().contains(r##"src="#""##));
    let html_tel = render_markdown("![img](tel:+1234567890)");
    assert!(html_tel.as_str().contains(r##"src="#""##));
}

#[test]
fn test_引用ブロック() {
    let md = "> This is a quote";
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(html.contains("<blockquote>"));
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
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(html.contains("<pre"));
    assert!(html.contains("fn"));
    assert!(html.contains("<p>After</p>"));
}

#[test]
fn test_言語指定なしコードブロックも正しく扱う() {
    let md = "```\nplain\n```\nAfter";
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(html.contains("<pre class=\"code-block\"><code class=\"syn-code\">plain"));
    assert!(html.contains("<p>After</p>"));
}

#[test]
fn test_見出し内インライン装飾が見出し要素内に収まる() {
    let md = "# Heading with *em* and `code`";
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(!html.contains("<em></em><h1"));
    assert!(html.contains(
        r##"<h1 id="heading-with-em-and-code">Heading with <em>em</em> and <code>code</code></h1>"##
    ));
}

#[test]
fn test_render_markdown_複合入力の公開api出力を固定する() {
    let md = "# Title `x`\n\n[link](https://example.com) ![img](https://example.com/pic.png)\n\n| L | R |\n|:--|--:|\n| A & B | `code` |\n\n```unknown-lang\n<a>\n```";
    let html = render_markdown(md);

    assert_eq!(
        html.as_str(),
        concat!(
            r#"<h1 id="title-x" data-line-block data-source-start-line="1" data-source-end-line="1"><span data-source-start-line="1" data-source-end-line="1">Title </span><code data-source-start-line="1" data-source-end-line="1">x</code></h1>"#,
            "\n",
            r##"<p data-line-block data-line-block-start="3" data-line-block-end="3"><a href="https://example.com"><span data-source-start-line="3" data-source-end-line="3">link</span></a><span data-source-start-line="3" data-source-end-line="3"> </span><img src="#" alt="img" /></p>"##,
            "\n",
            r#"<table data-line-block data-line-block-start="5" data-line-block-end="7">"#,
            "\n",
            "<thead>\n",
            r#"<th class="align-left"><span data-source-start-line="5" data-source-end-line="5">L</span></th>"#,
            "\n",
            r#"<th class="align-right"><span data-source-start-line="5" data-source-end-line="5">R</span></th>"#,
            "\n",
            "</thead>\n",
            "<tr>\n",
            r#"<td class="align-left"><span data-source-start-line="7" data-source-end-line="7">A &amp; B</span></td>"#,
            "\n",
            r#"<td class="align-right"><code data-source-start-line="7" data-source-end-line="7">code</code></td>"#,
            "\n",
            "</tr>\n",
            "</table>\n",
            r#"<pre class="code-block" data-line-block data-source-start-line="9" data-source-end-line="11"><code class="syn-code language-unknown-lang">&lt;a&gt;"#,
            "\n",
            "</code></pre>\n",
        )
    );
}

#[test]
fn test_見出しとコードブロックの行属性は観測性強化後も維持される() {
    let md = "# Title\n\n```unknown-lang\n<x>&\n```";
    let html = render_markdown(md);
    let html = html.as_str();

    assert!(html.contains(
        r#"<h1 id="title" data-line-block data-source-start-line="1" data-source-end-line="1">"#
    ));
    assert!(html.contains(
        r#"<pre class="code-block" data-line-block data-source-start-line="3" data-source-end-line="5"><code class="syn-code language-unknown-lang">&lt;x&gt;&amp;"#
    ));
}

#[test]
fn test_render_markdown_主要event_dispatchの出力を固定する() {
    let md = "> **strong** *em* ~~del~~  \n> soft\n\n---\n\n3. three\n4. four\n\n- item\n- [x] done\n- [ ] todo";
    let html = normalize_source_markup(render_markdown(md).as_str());

    assert_eq!(
        html,
        concat!(
            "<blockquote>\n",
            "<p><strong>strong</strong> <em>em</em> <del>del</del><br />\n",
            "soft</p>\n",
            "</blockquote>\n",
            "<hr />\n",
            r#"<ol start="3">"#,
            "\n",
            "<li>three</li>\n",
            "<li>four</li>\n",
            "</ol>\n",
            "<ul>\n",
            "<li>item</li>\n",
            r#"<li><input type="checkbox" checked="" disabled="" /> done</li>"#,
            "\n",
            r#"<li><input type="checkbox" disabled="" /> todo</li>"#,
            "\n",
            "</ul>\n",
        )
    );
}

#[test]
fn test_画像titleと画像内リンクはimg属性とaltに閉じる() {
    let md = r#"![logo [site](https://example.com)](./pic.png "caption")"#;
    let html = normalize_source_markup(render_markdown(md).as_str());

    assert!(html.contains(r#"<img src="./pic.png" alt="logo site" title="caption" />"#));
    assert!(!html.contains("<a href="));
    assert!(!html.contains("https://example.com"));
}

#[test]
fn test_リンク付き画像はa要素でimgを包む() {
    let md = r#"[![logo](./pic.png "caption")](https://example.com)"#;
    let html = normalize_source_markup(render_markdown(md).as_str());

    assert_eq!(
        html,
        r#"<p><a href="https://example.com"><img src="./pic.png" alt="logo" title="caption" /></a></p>
"#
    );
}

#[test]
fn test_重複見出しidはrender_markdown経由でも連番になる() {
    let html = normalize_source_markup(render_markdown("# Hello\n# Hello\n# Hello").as_str());

    assert_eq!(
        html,
        concat!(
            r#"<h1 id="hello">Hello</h1>"#,
            "\n",
            r#"<h1 id="hello-1">Hello</h1>"#,
            "\n",
            r#"<h1 id="hello-2">Hello</h1>"#,
            "\n",
        )
    );
}

#[test]
fn test_連続テーブルでalignmentが次のテーブルへ漏れない() {
    let md = "| L | R |\n|:--|--:|\n| a | b |\n\n| C | D |\n|---|---|\n| c | d |";
    let html = normalize_source_markup(render_markdown(md).as_str());

    assert!(html.contains(r#"<th class="align-left">L</th>"#));
    assert!(html.contains(r#"<th class="align-right">R</th>"#));
    assert!(html.contains("<th>C</th>"));
    assert!(html.contains("<th>D</th>"));
    assert!(!html.contains(r#"<th class="align-left">C</th>"#));
    assert!(!html.contains(r#"<th class="align-right">D</th>"#));
    assert!(!html.contains(r#"<td class="align-left">c</td>"#));
    assert!(!html.contains(r#"<td class="align-right">d</td>"#));
}

#[test]
fn test_テーブルセル内画像はtableとimage状態を混同しない() {
    let md = r#"| media |
|:---|
| ![logo](./logo.png "caption") |"#;
    let html = normalize_source_markup(render_markdown(md).as_str());

    assert!(html.contains("<table>"));
    assert!(html.contains(r#"<th class="align-left">media</th>"#));
    assert!(html.contains(
        r#"<td class="align-left"><img src="./logo.png" alt="logo" title="caption" /></td>"#
    ));
    assert!(html.contains("</table>"));
}

#[test]
fn test_見出し内リンクと装飾のid生成とhtmlを固定する() {
    let md = "# A [Rust](https://www.rust-lang.org \"site\") *lang* `code`";
    let html = normalize_source_markup(render_markdown(md).as_str());

    assert!(html.contains(
        r#"<h1 id="a-rust-lang-code">A <a href="https://www.rust-lang.org" title="site">Rust</a> <em>lang</em> <code>code</code></h1>"#
    ));
}

#[test]
fn test_同一入力内でlinkとimageのurl_policy差分を固定する() {
    let html = normalize_source_markup(render_markdown(
        "[safe](mailto:user@example.com) [bad](data:text/html,<script>x</script>) ![remote](https://example.com/p.png) ![local](./local.png)",
    ).as_str());
    let html_str = html.as_str();

    assert!(html_str.contains(r#"href="mailto:user@example.com""#));
    assert!(html_str.contains(r##"<a href="#">bad</a>"##));
    assert!(html_str.contains(r##"<img src="#" alt="remote" />"##));
    assert!(html_str.contains(r#"<img src="./local.png" alt="local" />"#));
    assert!(!html_str.contains("data:text/html"));
    assert!(!html_str.contains("https://example.com/p.png"));
}

#[test]
fn test_renderer_state境界が連続構文で漏れない() {
    let md = concat!(
        "# Head ![logo](./logo.png \"caption\") `code`\n",
        "\n",
        "![remote](https://example.com/p.png)\n",
        "\n",
        "| L | R |\n",
        "|:--|--:|\n",
        "| a | b |\n",
        "\n",
        "```unknown-lang\n",
        "<x>\n",
        "```\n",
        "\n",
        "After",
    );
    let html = normalize_source_markup(render_markdown(md).as_str());

    assert!(html.contains(
        r#"<h1 id="head-code">Head <img src="./logo.png" alt="logo" title="caption" /> <code>code</code></h1>"#
    ));
    assert!(html.contains(r##"<p><img src="#" alt="remote" /></p>"##));
    assert!(html.contains(r#"<th class="align-left">L</th>"#));
    assert!(html.contains(r#"<th class="align-right">R</th>"#));
    assert!(html.contains(r#"<td class="align-left">a</td>"#));
    assert!(html.contains(r#"<td class="align-right">b</td>"#));
    assert!(html.contains(
        r#"<pre class="code-block"><code class="syn-code language-unknown-lang">&lt;x&gt;"#
    ));
    assert!(html.contains("<p>After</p>"));
    assert!(!html.contains("https://example.com/p.png"));
    assert!(!html.contains("<x>"));
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
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(html.contains("<th>A</th>"));
    assert!(html.contains("<th>C</th>"));
    assert!(!html.contains("<th>C</td>"));
}

#[test]
fn test_unsafeスキームのリンクは無効化される() {
    let md = "[click](javascript:alert(1))";
    let html = normalize_source_markup(render_markdown(md).as_str());
    assert!(html.contains(r##"<a href="#">click</a>"##));
    assert!(!html.contains("javascript:alert(1)"));
}

#[test]
fn test_sanitize_hrefのホワイトスペースパディング付き危険urlは無効化される() {
    let html =
        normalize_source_markup(render_markdown("[click](  javascript:alert(1)  )").as_str());
    assert!(html.contains(r##"<a href="#">click</a>"##));
    assert!(!html.contains("javascript:alert(1)"));
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
    let html = normalize_source_markup(render_markdown(md).as_str());
    let toc = generate_toc(md);
    assert!(html.contains(r##"<h1 id="section">!!!</h1>"##));
    assert!(html.contains(r##"<h1 id="section-1">---</h1>"##));
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
fn test_render_documentは見出し画像code_softbreakで本文とtocのidを共有する() {
    let md = concat!(
        "![logo](x.png) Title `code`\n",
        "continued\n",
        "====\n",
        "\n",
        "![logo](x.png) Title `code` continued\n",
        "====\n",
    );

    let document = render_document(md);

    assert_eq!(document.headings.len(), 2);
    assert_eq!(document.headings[0].text, "Title code continued");
    assert_eq!(document.headings[0].id, "title-code-continued");
    assert_eq!(document.headings[1].id, "title-code-continued-1");
    assert!(document
        .content
        .as_str()
        .contains(r##"id="title-code-continued""##));
    assert!(document
        .content
        .as_str()
        .contains(r##"id="title-code-continued-1""##));
    assert!(document
        .toc
        .as_str()
        .contains(r##"href="#title-code-continued""##));
    assert!(document
        .toc
        .as_str()
        .contains(r##"href="#title-code-continued-1""##));
    assert!(!document.toc.as_str().contains("logo-title"));
}

#[test]
fn test_render_documentはhardbreak見出しでも本文とtocのidを共有する() {
    let md = "First  \nSecond\n====";
    let document = render_document(md);

    assert_eq!(document.headings.len(), 1);
    assert_eq!(document.headings[0].text, "First Second");
    assert_eq!(document.headings[0].id, "first-second");
    assert!(document.content.as_str().contains(r##"id="first-second""##));
    assert!(document.toc.as_str().contains(r##"href="#first-second""##));
}

#[test]
fn test_render_documentはraw_htmlを破棄しtocをescapeする() {
    let md = "# Hello <script>alert(1)</script> & World";
    let document = render_document(md);

    assert!(!document.content.as_str().contains("<script>"));
    assert!(!document.toc.as_str().contains("<script>"));
    assert!(!document.toc.as_str().contains("alert(1)"));
    assert!(document.toc.as_str().contains("Hello"));
    assert!(document.toc.as_str().contains("&amp; World"));
    assert!(document.toc.as_str().contains(r##"href="#hello-world""##));
}

#[test]
fn test_extract_headingsはrender_documentのheadingsと一致する() {
    let md = "# A `code`\n\n## ![logo](x.png) B\n\n# A `code`";

    let document = render_document(md);
    let headings = extract_headings(md);

    assert_eq!(headings, document.headings);
    assert_eq!(headings[0].id, "a-code");
    assert_eq!(headings[1].id, "b");
    assert_eq!(headings[2].id, "a-code-1");
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
    let html = normalize_source_markup(render_markdown("[empty]()").as_str());
    assert!(html.contains(r##"<a href="#">empty</a>"##));
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
    use markdown_view::template::{render_page, MemoResponse, RenderPageParams, SidebarParams};
    use markdown_view::toc::generate_toc;

    let content =
        render_markdown("# Title\n<script>alert('xss')</script>\n[bad](javascript:alert(1))");
    let toc = generate_toc("# Title");
    let memo = MemoResponse::empty(None);
    let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
    let html = render_page(RenderPageParams {
        title: "Test",
        content: &content,
        toc: &toc,
        memo: &memo,
        dark_mode: false,
        syntax_css: &syntax_css,
        sidebar: SidebarParams::SingleFile,
    });

    assert!(!html.as_str().contains("<script>alert('xss')</script>"));
    let normalized = normalize_source_markup(html.as_str());
    assert!(normalized.contains(r##"<a href="#">bad</a>"##));
}

// --- テンプレート テスト ---

#[test]
fn test_render_pageのタイトルがエスケープされる() {
    use markdown_view::renderer::{render_markdown, syntax_theme_css};
    use markdown_view::template::{render_page, MemoResponse, RenderPageParams, SidebarParams};
    use markdown_view::toc::generate_toc;
    let content = render_markdown("xss");
    let toc = generate_toc("# t");
    let memo = MemoResponse::empty(None);
    let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
    let html = render_page(RenderPageParams {
        title: "<script>xss</script>",
        content: &content,
        toc: &toc,
        memo: &memo,
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
    use markdown_view::template::{render_page, MemoResponse, RenderPageParams, SidebarParams};
    use markdown_view::toc::generate_toc;
    let content = render_markdown("x");
    let toc = generate_toc("# t");
    let memo = MemoResponse::empty(None);
    let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
    let light = render_page(RenderPageParams {
        title: "t",
        content: &content,
        toc: &toc,
        memo: &memo,
        dark_mode: false,
        syntax_css: &syntax_css,
        sidebar: SidebarParams::SingleFile,
    });
    let dark = render_page(RenderPageParams {
        title: "t",
        content: &content,
        toc: &toc,
        memo: &memo,
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
    use markdown_view::template::{render_page, MemoResponse, RenderPageParams, SidebarParams};
    use markdown_view::toc::generate_toc;
    let content = render_markdown("Hello");
    let toc = generate_toc("# H1");
    let memo = MemoResponse::empty(None);
    let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
    let html = render_page(RenderPageParams {
        title: "Test",
        content: &content,
        toc: &toc,
        memo: &memo,
        dark_mode: false,
        syntax_css: &syntax_css,
        sidebar: SidebarParams::SingleFile,
    });
    assert!(html.as_str().contains("<!DOCTYPE html>"));
    let normalized = normalize_source_markup(html.as_str());
    assert!(normalized.contains("<p>Hello</p>"));
    assert!(html.as_str().contains("<a href=\"#h1\">H1</a>"));
    assert!(html.as_str().contains("Test - markdown-view"));
    assert!(html.as_str().contains("id=\"document-title\""));
    assert!(html.as_str().contains("id=\"live-status\""));
    assert!(html.as_str().contains("id=\"reading-progress-bar\""));
    assert!(html.as_str().contains("id=\"toc-filter\""));
}
