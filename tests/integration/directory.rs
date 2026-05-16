use std::sync::Arc;

use tokio::sync::broadcast;

use markdown_view::server::{AppMode, AppState};

use super::support::setup_dir_server;

#[tokio::test]
async fn test_ディレクトリモード_indexページ取得() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(body.contains("<!DOCTYPE html>"));
    // README.mdがデフォルト表示される
    assert!(body.contains("README"));
    // ファイル一覧が含まれる
    assert!(body.contains("data-dir-mode=\"true\""));
    assert!(body.contains("data-file=\"README.md\""));
}
#[tokio::test]
async fn test_ディレクトリモード_ファイル一覧api() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/files", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let files: Vec<String> = resp.json().await.unwrap();
    assert!(files.contains(&"README.md".to_string()));
    assert!(files.contains(&"docs/guide.md".to_string()));
    // 非mdファイルは含まれない
    assert!(!files.iter().any(|f| f.ends_with(".txt")));
    // 隠しファイルは含まれない
    assert!(!files.iter().any(|f| f.starts_with('.')));
}
#[tokio::test]
async fn test_ディレクトリモード_ファイル指定コンテンツ取得() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=docs/guide.md", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    let content = json["content"].as_str().unwrap();
    assert!(content.contains("Guide"));
    // fileフィールドが含まれる
    assert_eq!(json["file"].as_str().unwrap(), "docs/guide.md");
}
#[tokio::test]
async fn test_ディレクトリモード_api_content_file空文字は404を返す() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].as_str().is_some());
}
#[tokio::test]
async fn test_ディレクトリモード_存在しないファイル() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=nonexistent.md", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].as_str().is_some());
}
#[tokio::test]
async fn test_ディレクトリモード_非mdファイル拒否() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=notes.txt", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}
#[tokio::test]
async fn test_ディレクトリモード_ファイル指定でindex取得() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/?file=docs/guide.md", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(body.contains("Guide"));
}
#[tokio::test]
async fn test_ディレクトリモード_アクティブファイルマーカーが表示される() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(body.contains("class=\"file-tree-file active\""));
}
#[tokio::test]
async fn test_ディレクトリモード_ファイル名のhtmlエスケープ() {
    let tmp_dir = tempfile::tempdir().unwrap();

    // HTMLエスケープが必要な文字（&）を含むファイル名を作成
    let special_filename = "A&B notes.md";
    tokio::fs::write(tmp_dir.path().join(special_filename), "# A&B")
        .await
        .unwrap();
    tokio::fs::write(tmp_dir.path().join("README.md"), "# README")
        .await
        .unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState::new_with_tokio_memo_fs(
        AppMode::new_directory(tmp_dir.path()).unwrap(),
        false,
        None,
        tx,
    ));

    let router = markdown_view::server::create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    // &がそのまま出力されず、エスケープされていること
    // data-file属性とリンクテキストの両方でエスケープされる
    assert!(body.contains("A&amp;B notes.md"));
    // 生の&がファイル名として出力されていないこと（data-file="A&B"のような形式がないこと）
    assert!(!body.contains("data-file=\"A&B notes.md\""));
}
#[tokio::test]
async fn test_ディレクトリモード_空ディレクトリで404を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    // .mdファイルを1つも置かない

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState::new_with_tokio_memo_fs(
        AppMode::new_directory(tmp_dir.path()).unwrap(),
        false,
        None,
        tx,
    ));

    let router = markdown_view::server::create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // indexが404を返す
    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    // api/contentも404を返す（デフォルトファイルがない）
    let resp = reqwest::get(format!("http://{}/api/content", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}
#[tokio::test]
async fn test_ディレクトリモード_readmeなし時はアルファベット順最初のファイルがデフォルト() {
    let tmp_dir = tempfile::tempdir().unwrap();

    // README.mdを作成せず、複数の.mdファイルを配置
    tokio::fs::write(tmp_dir.path().join("zebra.md"), "# Zebra")
        .await
        .unwrap();
    tokio::fs::write(tmp_dir.path().join("alpha.md"), "# Alpha")
        .await
        .unwrap();
    tokio::fs::write(tmp_dir.path().join("beta.md"), "# Beta")
        .await
        .unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState::new_with_tokio_memo_fs(
        AppMode::new_directory(tmp_dir.path()).unwrap(),
        false,
        None,
        tx,
    ));

    let router = markdown_view::server::create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // indexでalphaがデフォルト表示される
    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("Alpha"),
        "README.mdなし時はアルファベット順最初のファイルが表示されるべき"
    );
    // data-current-fileがalpha.mdであること
    assert!(body.contains("data-current-file=\"alpha.md\""));

    // api/contentでもalphaが返る
    let resp = reqwest::get(format!("http://{}/api/content", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["content"].as_str().unwrap().contains("Alpha"));
    assert_eq!(json["file"].as_str().unwrap(), "alpha.md");
}
#[tokio::test]
async fn test_ディレクトリモード_readmeがデフォルト表示される() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    let content = json["content"].as_str().unwrap();
    assert!(content.contains("README"));
}
#[tokio::test]
async fn test_ディレクトリモード_ファイルツリーにディレクトリ構造が含まれる() {
    let (_state, addr, _server, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    // ディレクトリがdetails/summaryで表現される
    assert!(
        body.contains("<details"),
        "ディレクトリノードにdetails要素が必要"
    );
    assert!(
        body.contains("<summary>"),
        "ディレクトリノードにsummary要素が必要"
    );
    // docsフォルダが存在する
    assert!(body.contains("docs"), "docsフォルダが表示されるべき");
}
