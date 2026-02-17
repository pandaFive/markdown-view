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
