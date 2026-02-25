use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use markdown_view::server::{AppMode, AppState};

// ==============================
// 単一ファイルモード テスト
// ==============================

#[tokio::test]
async fn test_indexページ取得() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Test\n\nHello world").await;

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
    let (_state, addr, _tmp_dir) = setup_single_file_server("**bold** text").await;

    let resp = reqwest::get(format!("http://{}/api/content", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    let content = json["content"].as_str().unwrap();
    assert!(content.contains("<strong>bold</strong>"));
}

#[tokio::test]
async fn test_httpは許可されないhostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Host Check").await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());

    for path in ["/", "/api/content"] {
        let resp = client
            .get(format!("http://{}{}", addr, path))
            .header("Host", &attack_host)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn test_websocket接続() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;

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
    let (state, addr, _tmp_dir) = setup_single_file_server("initial").await;

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
async fn test_存在しないファイル時は500を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let non_existent = tmp_dir.path().join("non_existent.md");

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState {
        mode: AppMode::SingleFile(non_existent),
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

    for path in ["/", "/api/content"] {
        let resp = reqwest::get(format!("http://{}{}", addr, path))
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    }
}

#[tokio::test]
async fn test_ファイル変更でwebsocket更新() {
    // 一時ファイルを作成
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("watch_test.md");
    tokio::fs::write(&file_path, "# Before").await.unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState {
        mode: AppMode::SingleFile(file_path.clone()),
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
    markdown_view::watcher::watch_path(state.clone())
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
    drop(tmp_dir);
}

#[tokio::test]
async fn test_websocketは異なるoriginを拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;

    let url = format!("ws://{}/ws", addr);
    let result = connect_ws(&url, "https://evil.example").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_websocketはrebind相当のhost_origin一致を拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;

    let url = format!("ws://{}/ws", addr);
    let rebinding_authority = format!("evil.example:{}", addr.port());
    let rebinding_origin = format!("http://{}", rebinding_authority);
    let result = connect_ws_with_host(&url, &rebinding_origin, Some(&rebinding_authority)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_websocket切断時に購読が速やかに解放される() {
    let (state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;
    let url = format!("ws://{}/ws", addr);
    let (mut ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();

    // 初期メッセージを受信して購読開始を確定
    let _ = tokio::time::timeout(Duration::from_secs(5), ws_stream.next())
        .await
        .unwrap();

    drop(ws_stream);

    // クライアント切断後、receiver_count が0へ戻ること
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if state.tx.receiver_count() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn test_ファイルサイズ上限超過で413を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let large_file = tmp_dir.path().join("large.md");

    // MAX_FILE_SIZE(10MB) + 1バイトのファイルを作成
    let content = "x".repeat(10 * 1024 * 1024 + 1);
    tokio::fs::write(&large_file, &content).await.unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState {
        mode: AppMode::SingleFile(large_file),
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

    for path in ["/", "/api/content"] {
        let resp = reqwest::get(format!("http://{}{}", addr, path))
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    }
}

#[tokio::test]
async fn test_セキュリティヘッダが設定されている() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Test").await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();

    // X-Content-Type-Options
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );

    // X-Frame-Options
    assert_eq!(resp.headers().get("x-frame-options").unwrap(), "DENY");

    // Content-Security-Policy
    let csp = resp
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("frame-ancestors 'none'"));
    assert!(csp.contains("object-src 'none'"));
    assert!(!csp.contains("data:"));
}

#[tokio::test]
async fn test_単一ファイルモードの後方互換_api_filesは空配列() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Test").await;

    let resp = reqwest::get(format!("http://{}/api/files", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: Vec<String> = resp.json().await.unwrap();
    assert!(json.is_empty());
}

// ==============================
// ディレクトリモード テスト
// ==============================

#[tokio::test]
async fn test_ディレクトリモード_indexページ取得() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

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
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

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
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

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
async fn test_ディレクトリモード_トラバーサル攻撃拒否() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=../../etc/passwd", addr))
        .await
        .unwrap();
    // NotFoundまたはForbidden
    assert!(
        resp.status() == reqwest::StatusCode::NOT_FOUND
            || resp.status() == reqwest::StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn test_ディレクトリモード_存在しないファイル() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=nonexistent.md", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_ディレクトリモード_非mdファイル拒否() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=notes.txt", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_ディレクトリモード_隠しファイルの直接アクセスが拒否される() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    // 隠しディレクトリ内のファイル
    let resp = reqwest::get(format!(
        "http://{}/api/content?file=.hidden/secret.md",
        addr
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // index経由でも同様
    let resp = reqwest::get(format!("http://{}/?file=.hidden/secret.md", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_ディレクトリモード_ファイル指定でindex取得() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/?file=docs/guide.md", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(body.contains("Guide"));
}

#[tokio::test]
async fn test_ディレクトリモード_websocket更新にfileフィールドが含まれる() {
    let (state, addr, tmp_dir) = setup_dir_server().await;

    // WebSocket接続
    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // ディレクトリモードではWebSocket初期メッセージは送信されないので、
    // broadcastで更新を送信してテスト
    let file_path = tmp_dir.path().join("README.md");
    markdown_view::server::notify_update(&state, &file_path).await;

    let msg = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(json["content"].as_str().unwrap().contains("README"));
    assert_eq!(json["file"].as_str().unwrap(), "README.md");
}

#[tokio::test]
async fn test_ディレクトリモード_api_filesは不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());

    let resp = client
        .get(format!("http://{}/api/files", addr))
        .header("Host", &attack_host)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
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
    let state = Arc::new(AppState {
        mode: AppMode::Directory(tmp_dir.path().to_path_buf()),
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
    let state = Arc::new(AppState {
        mode: AppMode::Directory(tmp_dir.path().to_path_buf()),
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
async fn test_ディレクトリモード_websocket初期メッセージが送信されない() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // ディレクトリモードでは接続直後にメッセージが送信されないことを確認
    let result = tokio::time::timeout(Duration::from_millis(500), read.next()).await;
    // タイムアウトする（メッセージなし）
    assert!(
        result.is_err(),
        "ディレクトリモードではWS初期メッセージは送信されないはず"
    );
}

#[tokio::test]
async fn test_ディレクトリモード_readmeがデフォルト表示される() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    let content = json["content"].as_str().unwrap();
    assert!(content.contains("README"));
}

// ==============================
// ヘルパー関数
// ==============================

/// 単一ファイルモードのテスト用サーバーセットアップ
async fn setup_single_file_server(
    markdown_content: &str,
) -> (Arc<AppState>, std::net::SocketAddr, tempfile::TempDir) {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("test.md");
    tokio::fs::write(&file_path, markdown_content)
        .await
        .unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState {
        mode: AppMode::SingleFile(file_path),
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

    (state, addr, tmp_dir)
}

/// ディレクトリモードのテスト用サーバーセットアップ
async fn setup_dir_server() -> (Arc<AppState>, std::net::SocketAddr, tempfile::TempDir) {
    let tmp_dir = tempfile::tempdir().unwrap();

    // ファイル構造を作成
    tokio::fs::write(
        tmp_dir.path().join("README.md"),
        "# README\n\nThis is readme.",
    )
    .await
    .unwrap();
    tokio::fs::write(tmp_dir.path().join("notes.txt"), "not markdown")
        .await
        .unwrap();
    tokio::fs::create_dir_all(tmp_dir.path().join("docs"))
        .await
        .unwrap();
    tokio::fs::write(
        tmp_dir.path().join("docs/guide.md"),
        "# Guide\n\nThis is guide.",
    )
    .await
    .unwrap();
    tokio::fs::create_dir_all(tmp_dir.path().join(".hidden"))
        .await
        .unwrap();
    tokio::fs::write(tmp_dir.path().join(".hidden/secret.md"), "# Secret")
        .await
        .unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState {
        mode: AppMode::Directory(tmp_dir.path().to_path_buf()),
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

    (state, addr, tmp_dir)
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
    connect_ws_with_host(url, origin, None).await
}

async fn connect_ws_with_host(
    url: &str,
    origin: &str,
    host: Option<&str>,
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
    if let Some(host) = host {
        request
            .headers_mut()
            .insert("Host", host.parse().expect("Hostヘッダは妥当な値"));
    }
    tokio_tungstenite::connect_async(request).await
}
