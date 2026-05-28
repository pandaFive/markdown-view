#[cfg(unix)]
use std::{fs, os::unix::fs::symlink};

#[cfg(unix)]
use super::support::make_file_unreadable;
use super::support::{
    assert_json_error_for_paths, setup_single_file_server, setup_single_file_server_from_path,
    setup_single_file_server_with_bytes,
};

#[tokio::test]
async fn test_indexページ取得() {
    let (_state, addr, _server, _tmp_dir) = setup_single_file_server("# Test\n\nHello world").await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(body.contains("<!DOCTYPE html>"));
    assert!(body.contains("Test"));
    assert!(body.contains("Hello world"));
    assert!(body.contains("markdown-view"));
}
#[tokio::test]
async fn test_apiコンテンツ取得() {
    let (_state, addr, _server, _tmp_dir) = setup_single_file_server("**bold** text").await;

    let resp = reqwest::get(format!("http://{}/api/content", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    let content = json["content"].as_str().unwrap();
    assert!(content.contains("<strong>"));
    assert!(content.contains("bold"));
    assert!(json.get("file").is_none());
}
#[tokio::test]
async fn test_単一ファイルモードでfileクエリは無視される() {
    let (_state, addr, _server, _tmp_dir) = setup_single_file_server("# Single Mode").await;

    let resp = reqwest::get(format!(
        "http://{}/api/content?file=does-not-matter.md",
        addr
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["content"].as_str().unwrap().contains("Single Mode"));
    assert!(json.get("file").is_none());
}
#[cfg(unix)]
#[tokio::test]
async fn test_api_content_open権限エラーは検証エラーで404を返す() {
    let (_state, addr, _server, _tmp_dir, file_path) =
        setup_single_file_server_with_bytes("unreadable.md", b"# content").await;

    // 解決済みhandleを保持できない状態ではpath再openへ戻さず、検証エラーとして止める。
    let Some(_permission_guard) = make_file_unreadable(&file_path) else {
        return;
    };

    assert_json_error_for_paths(
        addr,
        &["/api/content"],
        reqwest::StatusCode::NOT_FOUND,
        None,
    )
    .await;
}
#[tokio::test]
async fn test_存在しないファイル時は404を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("deleted.md");
    tokio::fs::write(&file_path, "# before delete")
        .await
        .unwrap();
    let (_state, addr, _server) = setup_single_file_server_from_path(&file_path).await;

    // AppMode生成後にファイルが消えたケースを再現
    tokio::fs::remove_file(&file_path).await.unwrap();

    assert_json_error_for_paths(
        addr,
        &["/"],
        reqwest::StatusCode::NOT_FOUND,
        Some("表示可能なMarkdownファイルが見つかりません"),
    )
    .await;
    assert_json_error_for_paths(
        addr,
        &["/api/content"],
        reqwest::StatusCode::NOT_FOUND,
        Some("指定したファイルが見つかりません"),
    )
    .await;
}
#[tokio::test]
async fn test_non_utf8ファイル読み込み時は422を返す() {
    let (_state, addr, _server, _tmp_dir, _file_path) =
        setup_single_file_server_with_bytes("binary.md", &[0xff, 0xfe, 0xfd]).await;

    assert_json_error_for_paths(
        addr,
        &["/", "/api/content"],
        reqwest::StatusCode::UNPROCESSABLE_ENTITY,
        Some("このファイルはUTF-8テキストではありません"),
    )
    .await;
}
#[tokio::test]
async fn test_ファイルサイズ上限超過で413を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let large_file = tmp_dir.path().join("large.md");

    // MAX_FILE_SIZE(10MB) + 1バイトのファイルを作成
    let content = "x".repeat(10 * 1024 * 1024 + 1);
    tokio::fs::write(&large_file, &content).await.unwrap();

    let (_state, addr, _server) = setup_single_file_server_from_path(&large_file).await;

    assert_json_error_for_paths(
        addr,
        &["/", "/api/content"],
        reqwest::StatusCode::PAYLOAD_TOO_LARGE,
        Some("ファイルサイズが上限（10MB）を超えています"),
    )
    .await;
}
#[tokio::test]
async fn test_ファイルサイズ上限ちょうど10mbは200を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let limit_file = tmp_dir.path().join("limit.md");

    // MAX_FILE_SIZE(10MB) ちょうどのファイル
    let content = "x".repeat(10 * 1024 * 1024);
    tokio::fs::write(&limit_file, &content).await.unwrap();

    let (_state, addr, _server) = setup_single_file_server_from_path(&limit_file).await;

    for path in ["/", "/api/content"] {
        let resp = reqwest::get(format!("http://{}{}", addr, path))
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
    }
}
#[cfg(unix)]
#[tokio::test]
async fn test_単一ファイルモード_シンボリックリンク差し替えを拒否する() {
    let (_state, addr, _server, tmp_dir) = setup_single_file_server("# Test").await;
    let file_path = tmp_dir.path().join("test.md");
    let outside_path = tmp_dir.path().join("outside.md");
    tokio::fs::write(&outside_path, "# Outside").await.unwrap();

    fs::remove_file(&file_path).unwrap();
    symlink(&outside_path, &file_path).unwrap();

    assert_json_error_for_paths(
        addr,
        &["/", "/api/content"],
        reqwest::StatusCode::NOT_FOUND,
        None,
    )
    .await;
}
#[tokio::test]
async fn test_単一ファイルモードの後方互換_api_filesは空配列() {
    let (_state, addr, _server, _tmp_dir) = setup_single_file_server("# Test").await;

    let resp = reqwest::get(format!("http://{}/api/files", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: Vec<String> = resp.json().await.unwrap();
    assert!(json.is_empty());
}
#[tokio::test]
async fn test_単一ファイルモードの後方互換_api_searchは空結果() {
    let (_state, addr, _server, _tmp_dir) = setup_single_file_server("# Test").await;

    let resp = reqwest::get(format!("http://{}/api/search?q=test", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["query"].as_str().unwrap(), "test");
    assert_eq!(json["results"].as_array().unwrap().len(), 0);
    assert_eq!(json["searched_files"].as_u64().unwrap(), 0);
    assert_eq!(json["skipped_files"].as_u64().unwrap(), 0);
}
#[tokio::test]
async fn test_単一ファイルモード_api_searchは長すぎるqueryを400で拒否する() {
    let (_state, addr, _server, _tmp_dir) = setup_single_file_server("# Test").await;
    let client = reqwest::Client::new();
    let query = "あ".repeat(257);

    let resp = client
        .get(format!("http://{}/api/search", addr))
        .query(&[("q", &query)])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let body = resp.text().await.unwrap();
    assert!(!body.contains(&query));
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["error"].as_str(), Some("検索クエリが長すぎます"));
}
