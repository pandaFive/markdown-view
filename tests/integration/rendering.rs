use super::support::{setup_dir_server, setup_single_file_server};

#[tokio::test]
async fn test_ディレクトリモード_タブuiが表示される() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    // タブボタン
    assert!(body.contains("sidebar-tab"), "タブボタンが存在するべき");
    // パネル
    assert!(
        body.contains("id=\"panel-files\""),
        "ファイルパネルが存在するべき"
    );
    assert!(
        body.contains("<div class=\"sidebar-panel active\" id=\"panel-files\">"),
        "ファイルタブが初期表示されるべき"
    );
    assert!(
        body.contains("<div class=\"sidebar-utility\">"),
        "ファイル検索領域が存在するべき"
    );
    assert!(
        body.contains("id=\"file-filter\""),
        "ファイル検索入力が存在するべき"
    );
    assert!(
        body.contains("id=\"file-filter-summary\""),
        "ファイル検索サマリーが存在するべき"
    );
    assert!(
        body.contains("id=\"panel-toc\""),
        "目次パネルが存在するべき"
    );
    assert!(
        body.contains("id=\"panel-memo\""),
        "メモパネルが存在するべき"
    );
}
#[tokio::test]
async fn test_単一ファイルモード_タブが表示されない() {
    let (_state, addr, _server, _tmp_dir) = setup_single_file_server("# Test\n\nHello").await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    // 単一ファイルモードでも目次/メモタブは表示される
    assert!(
        !body.contains("data-tab=\"files\""),
        "単一ファイルモードではファイルタブは不要"
    );
    assert!(
        !body.contains("id=\"panel-files\""),
        "単一ファイルモードではファイルパネルは不要"
    );
    assert!(body.contains("data-tab=\"toc\""));
    assert!(body.contains("data-tab=\"memo\""));
    assert!(body.contains("id=\"panel-memo\""));
}
#[tokio::test]
async fn test_本文htmlにソース行番号属性と引用ボタンが含まれる() {
    let (_state, addr, _server, _tmp_dir) =
        setup_single_file_server("# Heading\n\nLine one\n\nLine two").await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(body.contains("data-source-start-line=\"1\""));
    assert!(body.contains("data-source-end-line=\"1\""));
    assert!(body.contains("id=\"quote-selection-action\""));
    assert!(body.contains("function buildQuoteMarkdownFromSelection()"));
}
