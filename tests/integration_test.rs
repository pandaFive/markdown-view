use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

#[tokio::test]
async fn test_indexページ取得() {
    let (state, addr) = setup_server("# Test\n\nHello world").await;
    let _ = state; // stateを保持してサーバーを維持

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
    let (state, addr) = setup_server("**bold** text").await;
    let _ = state;

    let resp = reqwest::get(format!("http://{}/api/content", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    let content = json["content"].as_str().unwrap();
    assert!(content.contains("<strong>bold</strong>"));
}

#[tokio::test]
async fn test_websocket接続() {
    let (state, addr) = setup_server("# WS Test").await;
    let _ = state;

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (mut _write, mut read) = ws_stream.split();

    // 接続直後に初期コンテンツが送信される
    let msg = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(json["content"].as_str().unwrap().contains("WS Test"));
    assert!(!json["toc"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn test_websocketブロードキャスト受信() {
    let (state, addr) = setup_server("initial").await;

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // 初期メッセージを消費
    let _ = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .unwrap();

    // broadcastで更新を送信
    let update = serde_json::json!({
        "content": "<p>updated</p>",
        "toc": ""
    });
    state.tx.send(update.to_string()).unwrap();

    // WebSocketで受信
    let msg = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let text = msg.into_text().unwrap();
    assert!(text.contains("updated"));
}

#[tokio::test]
async fn test_存在しないファイル() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let non_existent = tmp_dir.path().join("non_existent.md");

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(markdown_view::server::AppState {
        file_path: non_existent,
        dark_mode: false,
        theme: None,
        tx,
    });

    let router = markdown_view::server::create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains("ファイルアクセスエラー"));
}

#[tokio::test]
async fn test_ファイル変更でwebsocket更新() {
    // 一時ファイルを作成
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("watch_test.md");
    tokio::fs::write(&file_path, "# Before").await.unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(markdown_view::server::AppState {
        file_path: file_path.clone(),
        dark_mode: false,
        theme: None,
        tx,
    });

    // サーバー起動
    let router = markdown_view::server::create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // ファイル監視開始
    markdown_view::watcher::watch_file(state.clone())
        .await
        .unwrap();

    // WebSocket接続
    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // 初期メッセージを消費
    let _ = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .unwrap();

    // ファイルを変更
    tokio::fs::write(&file_path, "# After Change")
        .await
        .unwrap();

    // WebSocketで更新を受信（debounce 300ms + αのタイムアウト）
    let msg = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(json["content"].as_str().unwrap().contains("After Change"));

    // tmp_dirをleakしてテスト中に削除されないようにする
    std::mem::forget(tmp_dir);
}

#[tokio::test]
async fn test_websocketは異なるoriginを拒否する() {
    let (state, addr) = setup_server("# WS Test").await;
    let _ = state;

    let url = format!("ws://{}/ws", addr);
    let result = connect_ws(&url, "https://evil.example").await;
    assert!(result.is_err());
}

/// テスト用サーバーをセットアップするヘルパー
async fn setup_server(
    markdown_content: &str,
) -> (Arc<markdown_view::server::AppState>, std::net::SocketAddr) {
    // 一時ファイルにMarkdownを書き込む
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("test.md");
    tokio::fs::write(&file_path, markdown_content)
        .await
        .unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(markdown_view::server::AppState {
        file_path,
        dark_mode: false,
        theme: None,
        tx,
    });

    let router = markdown_view::server::create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // tempfileのownershipはstateの寿命と一緒
    // tmp_dirをleakしてテスト中に削除されないようにする
    std::mem::forget(tmp_dir);

    (state, addr)
}

async fn connect_ws(
    url: &str,
    origin: &str,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    tokio_tungstenite::tungstenite::Error,
> {
    let mut request = url.into_client_request()?;
    request
        .headers_mut()
        .insert("Origin", origin.parse().expect("Originヘッダは妥当な値"));
    tokio_tungstenite::connect_async(request).await
}
