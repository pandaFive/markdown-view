use std::path::PathBuf;

/// CLI引数パース用のヘルパー
/// clap の try_parse_from を使って引数をパースする
fn parse_args(args: &[&str]) -> Result<markdown_view::cli::Args, clap::Error> {
    use clap::Parser;
    markdown_view::cli::Args::try_parse_from(args)
}

#[test]
fn test_基本引数_ファイルパスのみ() {
    let args = parse_args(&["markdown-view", "README.md"]).unwrap();
    assert_eq!(args.file, PathBuf::from("README.md"));
    // デフォルト値の確認
    assert_eq!(args.port, 3000);
    assert!(!args.no_open);
    assert!(!args.dark);
    assert!(args.theme.is_none());
}

#[test]
fn test_ポート指定_短縮オプション() {
    let args = parse_args(&["markdown-view", "README.md", "-p", "8080"]).unwrap();
    assert_eq!(args.port, 8080);
}

#[test]
fn test_ポート指定_長形式オプション() {
    let args = parse_args(&["markdown-view", "README.md", "--port", "9090"]).unwrap();
    assert_eq!(args.port, 9090);
}

#[test]
fn test_ブラウザ自動起動なし() {
    let args = parse_args(&["markdown-view", "README.md", "--no-open"]).unwrap();
    assert!(args.no_open);
}

#[test]
fn test_ダークモード() {
    let args = parse_args(&["markdown-view", "README.md", "--dark"]).unwrap();
    assert!(args.dark);
}

#[test]
fn test_テーマ指定() {
    let args = parse_args(&["markdown-view", "README.md", "--theme", "Solarized (dark)"]).unwrap();
    assert_eq!(args.theme.as_deref(), Some("Solarized (dark)"));
}

#[test]
fn test_全オプション組み合わせ() {
    let args = parse_args(&[
        "markdown-view",
        "docs/spec.md",
        "-p",
        "4000",
        "--no-open",
        "--dark",
        "--theme",
        "Monokai",
    ])
    .unwrap();
    assert_eq!(args.file, PathBuf::from("docs/spec.md"));
    assert_eq!(args.port, 4000);
    assert!(args.no_open);
    assert!(args.dark);
    assert_eq!(args.theme.as_deref(), Some("Monokai"));
}

#[test]
fn test_ファイル引数なしはエラー() {
    let result = parse_args(&["markdown-view"]);
    assert!(result.is_err());
}

#[test]
fn test_不正ポート番号() {
    let result = parse_args(&["markdown-view", "README.md", "-p", "abc"]);
    assert!(result.is_err());
}
