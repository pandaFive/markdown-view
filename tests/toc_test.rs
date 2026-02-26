use markdown_view::toc::generate_toc;

#[test]
fn test_単一見出し() {
    let md = "# Title";
    let toc = generate_toc(md);
    assert!(toc.contains("Title"));
    assert!(toc.contains(r##"href="#title""##));
}

#[test]
fn test_ネストされた見出し() {
    let md = "# Chapter 1\n## Section 1.1\n## Section 1.2\n# Chapter 2";
    let toc = generate_toc(md);
    assert!(toc.contains("Chapter 1"));
    assert!(toc.contains("Section 1.1"));
    assert!(toc.contains("Section 1.2"));
    assert!(toc.contains("Chapter 2"));
    // ネスト構造の確認（ul内にulがある）
    assert!(toc.matches("<ul>").count() >= 2);
}

#[test]
fn test_特殊文字を含む見出し() {
    let md = "# Hello & World <test>";
    let toc = generate_toc(md);
    // IDはスラッグ化される
    assert!(toc.contains("hello-"));
    assert!(toc.contains("world"));
}

#[test]
fn test_重複idの処理() {
    let md = "# Title\n# Title\n# Title";
    let toc = generate_toc(md);
    // 重複IDは連番で区別
    assert!(toc.contains(r##"href="#title""##));
    assert!(toc.contains(r##"href="#title-1""##));
    assert!(toc.contains(r##"href="#title-2""##));
}

#[test]
fn test_見出しなし() {
    let md = "Just a paragraph";
    let toc = generate_toc(md);
    assert!(toc.is_empty());
}

#[test]
fn test_空入力() {
    let toc = generate_toc("");
    assert!(toc.is_empty());
}

#[test]
fn test_深いネスト() {
    let md = "# H1\n## H2\n### H3\n#### H4";
    let toc = generate_toc(md);
    assert!(toc.contains("H1"));
    assert!(toc.contains("H2"));
    assert!(toc.contains("H3"));
    assert!(toc.contains("H4"));
}

#[test]
fn test_見出し画像のaltはid計算に含めない() {
    let md = "# ![logo](x.png) Title";
    let toc = generate_toc(md);
    assert!(toc.contains(r##"href="#title""##));
    assert!(!toc.contains(r##"href="#logo-title""##));
}

#[test]
fn test_tocのネストulは親liの内側に生成される() {
    let md = "# A\n## B";
    let toc = generate_toc(md);
    assert!(toc.contains(r##"<li><a href="#a">A</a><ul>"##));
    assert!(!toc.contains(r##"</li><ul>"##));
}

#[test]
fn test_見出しレベルが飛んでもulの直下にulを作らない() {
    let md = "# A\n### C";
    let toc = generate_toc(md);
    assert!(toc.contains(r##"<li><a href="#a">A</a><ul>"##));
    assert!(!toc.contains("<ul>\n<ul>"));
}

#[test]
fn test_複数行見出しのスラッグが改行をスペースとして扱う() {
    let md = "hello\nworld\n===";
    let toc = generate_toc(md);
    assert!(toc.contains(r##"href="#hello-world""##));
}

#[test]
fn test_slugify_日本語見出しはsection連番で一意化される() {
    let md = "# 日本語見出し\n## 日本語見出し";
    let toc = generate_toc(md);
    assert!(toc.contains(r##"href="#日本語見出し""##));
    assert!(toc.contains(r##"href="#日本語見出し-1""##));
}
