use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use markdown_view::renderer::render_markdown;
use markdown_view::server::{AppMode, AppState, BroadcastMessage};
use markdown_view::template::UpdateMessage;
use markdown_view::toc::generate_toc;

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
    assert!(json.get("file").is_none());
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
        let json: serde_json::Value = resp.json().await.unwrap();
        assert!(json["error"].as_str().is_some());
    }
}

#[tokio::test]
async fn test_単一ファイルモードでfileクエリは無視される() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Single Mode").await;

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
    state
        .tx()
        .send(BroadcastMessage::Update(UpdateMessage::new(
            render_markdown("updated"),
            generate_toc("# updated"),
            None,
        )))
        .unwrap();

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
    let file_path = tmp_dir.path().join("deleted.md");
    tokio::fs::write(&file_path, "# before delete")
        .await
        .unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState::new(
        AppMode::new_single_file(&file_path).unwrap(),
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

    // AppMode生成後にファイルが消えたケースを再現
    tokio::fs::remove_file(&file_path).await.unwrap();

    for path in ["/", "/api/content"] {
        let resp = reqwest::get(format!("http://{}{}", addr, path))
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::INTERNAL_SERVER_ERROR);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(
            json["error"].as_str().unwrap(),
            "ファイルの読み込みに失敗しました"
        );
    }
}

#[tokio::test]
async fn test_non_utf8ファイル読み込み時は422を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("binary.md");
    tokio::fs::write(&file_path, vec![0xff, 0xfe, 0xfd])
        .await
        .unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState::new(
        AppMode::new_single_file(&file_path).unwrap(),
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

    for path in ["/", "/api/content"] {
        let resp = reqwest::get(format!("http://{}{}", addr, path))
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(
            json["error"].as_str().unwrap(),
            "このファイルはUTF-8テキストではありません"
        );
    }
}

#[tokio::test]
async fn test_ファイル変更でwebsocket更新() {
    // 一時ファイルを作成
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("watch_test.md");
    tokio::fs::write(&file_path, "# Before").await.unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState::new(
        AppMode::new_single_file(&file_path).unwrap(),
        false,
        None,
        tx,
    ));

    // サーバー起動
    let router = markdown_view::server::create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // ファイル監視開始
    let _watch_handle = markdown_view::watcher::watch_path(state.clone())
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
async fn test_websocketはoriginポート不一致を拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# WS Test").await;

    let url = format!("ws://{}/ws", addr);
    let wrong_port_origin = format!("http://localhost:{}", addr.port() + 1);
    let result = connect_ws(&url, &wrong_port_origin).await;
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
            if state.tx().receiver_count() == 0 {
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
    let state = Arc::new(AppState::new(
        AppMode::new_single_file(&large_file).unwrap(),
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

    for path in ["/", "/api/content"] {
        let resp = reqwest::get(format!("http://{}{}", addr, path))
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(
            json["error"].as_str().unwrap(),
            "ファイルサイズが上限（10MB）を超えています"
        );
    }
}

#[tokio::test]
async fn test_ファイルサイズ上限ちょうど10mbは200を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let limit_file = tmp_dir.path().join("limit.md");

    // MAX_FILE_SIZE(10MB) ちょうどのファイル
    let content = "x".repeat(10 * 1024 * 1024);
    tokio::fs::write(&limit_file, &content).await.unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState::new(
        AppMode::new_single_file(&limit_file).unwrap(),
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

    for path in ["/", "/api/content"] {
        let resp = reqwest::get(format!("http://{}{}", addr, path))
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
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
    assert!(csp.contains("script-src 'sha256-"));
    assert!(csp.contains("style-src 'sha256-"));
    assert!(csp.contains("frame-ancestors 'none'"));
    assert!(csp.contains("object-src 'none'"));
    assert!(!csp.contains("script-src 'unsafe-inline'"));
    assert!(!csp.contains("style-src 'unsafe-inline'"));
    assert!(!csp.contains("data:"));
    assert_eq!(
        resp.headers()
            .get("x-markdown-view-security-warning")
            .unwrap(),
        "none"
    );
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
async fn test_ディレクトリモード_api_content_file空文字は404を返す() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].as_str().is_some());
}

#[tokio::test]
async fn test_ディレクトリモード_トラバーサル攻撃拒否() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=../../etc/passwd", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_ディレクトリモード_存在しないファイル() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=nonexistent.md", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].as_str().is_some());
}

#[tokio::test]
async fn test_ディレクトリモード_非mdファイル拒否() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content?file=notes.txt", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
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
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    // index経由でも同様
    let resp = reqwest::get(format!("http://{}/?file=.hidden/secret.md", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
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
async fn test_ディレクトリモード_アクティブファイルマーカーが表示される() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(body.contains("class=\"file-tree-file active\""));
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
    let state = Arc::new(AppState::new(
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
    let state = Arc::new(AppState::new(
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
    let state = Arc::new(AppState::new(
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
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

    let resp = reqwest::get(format!("http://{}/api/content", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    let content = json["content"].as_str().unwrap();
    assert!(content.contains("README"));
}

#[tokio::test]
async fn test_監視エラーがwebsocketクライアントにエラーjsonとして届く() {
    let (state, addr, _tmp_dir) = setup_single_file_server("# Error Test").await;

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // 単一ファイルモード: 初期メッセージを消費
    let _ = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .unwrap();

    // broadcastでエラーJSONを送信（watcher.rsのbroadcast_errorと同じ形式）
    state
        .tx()
        .send(BroadcastMessage::Error(
            "ファイル監視エラー: テスト用エラー".to_string(),
        ))
        .unwrap();

    // WebSocketでエラーJSONを受信
    let msg = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(json["error"].as_str().unwrap().contains("テスト用エラー"));
    // contentフィールドは存在しない
    assert!(json.get("content").is_none());
}

#[tokio::test]
async fn test_ディレクトリモード_ファイルツリーにディレクトリ構造が含まれる() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

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

#[tokio::test]
async fn test_ディレクトリモード_タブuiが表示される() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;

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
        body.contains("id=\"panel-toc\""),
        "目次パネルが存在するべき"
    );
}

#[tokio::test]
async fn test_単一ファイルモード_タブが表示されない() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Test\n\nHello").await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    // HTML構造にタブ要素が含まれない（CSSクラス定義ではなくHTML構造を検証）
    assert!(
        !body.contains("data-tab=\"files\""),
        "単一ファイルモードではタブは不要"
    );
    assert!(
        !body.contains("id=\"panel-files\""),
        "単一ファイルモードではファイルパネルは不要"
    );
}

// ==============================
// WebSocket close frame テスト
// ==============================

#[tokio::test]
async fn test_websocket_non_utf8ファイルでclose_frameにuser_messageが含まれる() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("binary.md");
    // 非UTF-8バイト列を書き込む
    tokio::fs::write(&file_path, vec![0xff, 0xfe, 0xfd])
        .await
        .unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState::new(
        AppMode::new_single_file(&file_path).unwrap(),
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

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // サーバーがclose frameを送信するのを受信
    let msg = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    match msg {
        tokio_tungstenite::tungstenite::Message::Close(Some(frame)) => {
            assert_eq!(
                frame.code,
                tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Error
            );
            let reason: &str = frame.reason.as_ref();
            assert_eq!(reason, "このファイルはUTF-8テキストではありません");
        }
        other => panic!("Close frameを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_websocket_削除済みファイルでclose_frameにuser_messageが含まれる() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("deleted.md");
    tokio::fs::write(&file_path, "# before delete")
        .await
        .unwrap();

    let (tx, _rx) = broadcast::channel(16);
    let state = Arc::new(AppState::new(
        AppMode::new_single_file(&file_path).unwrap(),
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

    // AppMode生成後にファイルを削除
    tokio::fs::remove_file(&file_path).await.unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    let msg = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    match msg {
        tokio_tungstenite::tungstenite::Message::Close(Some(frame)) => {
            assert_eq!(
                frame.code,
                tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Error
            );
            let reason: &str = frame.reason.as_ref();
            assert_eq!(reason, "ファイルの読み込みに失敗しました");
        }
        other => panic!("Close frameを期待したが {:?} を受信", other),
    }
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
    let state = Arc::new(AppState::new(
        AppMode::new_single_file(&file_path).unwrap(),
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
    let state = Arc::new(AppState::new(
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
