#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
#[cfg(unix)]
use std::{fs, os::unix::fs::symlink};

use futures_util::{stream::SplitStream, StreamExt};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use markdown_view::renderer::render_markdown;
use markdown_view::server::{AppMode, AppState, BroadcastMessage, WatchService};
use markdown_view::template::UpdateMessage;
use markdown_view::toc::generate_toc;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type WsReadHalf = SplitStream<WsStream>;

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
    assert!(content.contains("<strong>"));
    assert!(content.contains("bold"));
    assert!(json.get("file").is_none());
}

#[tokio::test]
async fn test_apiメモ_未作成時は空を返す() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;

    let resp = reqwest::get(format!("http://{}/api/memo", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["raw"], "");
    assert_eq!(json["html"], "");
    assert!(json.get("file").is_none());
}

#[tokio::test]
async fn test_apiメモ_保存と再取得ができる() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "> quote\n\n出典: [test.md](#memo) L1-L2"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), 200);
    let saved: serde_json::Value = save.json().await.unwrap();
    assert_eq!(saved["raw"], "> quote\n\n出典: [test.md](#memo) L1-L2");
    assert!(saved["html"].as_str().unwrap().contains("<blockquote"));
    assert!(
        tokio::fs::try_exists(tmp_dir.path().join(".test.md.memo.md"))
            .await
            .unwrap()
    );

    let get = client
        .get(format!("http://{}/api/memo", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let fetched: serde_json::Value = get.json().await.unwrap();
    assert_eq!(fetched["raw"], "> quote\n\n出典: [test.md](#memo) L1-L2");
}

#[tokio::test]
async fn test_apiメモ_保存成功時にmemo_updateをbroadcastする() {
    let (state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let mut rx = state.tx().subscribe();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "live memo"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), 200);

    let received = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("memo update should be broadcast")
        .expect("broadcast receive should succeed");

    match received {
        BroadcastMessage::MemoUpdate(update) => {
            assert_eq!(update.file(), "test.md");
            assert_eq!(update.raw(), "live memo");
            assert!(update.html().as_str().contains("live memo"));
        }
        other => panic!("MemoUpdateメッセージを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_apiメモ_保存失敗時はmemo_updateをbroadcastしない() {
    let (state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let mut rx = state.tx().subscribe();
    let oversized = "a".repeat((markdown_view::server::MAX_FILE_SIZE as usize) + 1);

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": oversized
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

    assert!(matches!(
        rx.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn test_apiメモ_空白のみ保存で既存メモが削除される() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let memo_path = tmp_dir.path().join(".test.md.memo.md");

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "keep me"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), 200);
    assert!(tokio::fs::try_exists(&memo_path).await.unwrap());

    let delete = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "  \n  "
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), 200);
    let deleted: serde_json::Value = delete.json().await.unwrap();
    assert_eq!(deleted["raw"], "");
    assert_eq!(deleted["html"], "");
    assert!(!tokio::fs::try_exists(&memo_path).await.unwrap());

    let get = client
        .get(format!("http://{}/api/memo", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let fetched: serde_json::Value = get.json().await.unwrap();
    assert_eq!(fetched["raw"], "");
    assert_eq!(fetched["html"], "");
}

#[tokio::test]
async fn test_apiメモ_jsonエスケープで膨らんでも上限内rawなら保存できる() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let raw = "\\".repeat(6 * 1024 * 1024);

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": raw
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 200);
    let saved: serde_json::Value = save.json().await.unwrap();
    assert_eq!(saved["raw"].as_str().unwrap().len(), 6 * 1024 * 1024);
}

#[tokio::test]
async fn test_apiメモ_10mb超過は413で拒否する() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let raw = "a".repeat((10 * 1024 * 1024) + 1);

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": raw
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["error"], "メモサイズが上限（10MB）を超えています");
}

#[tokio::test]
async fn test_indexページ取得_壊れたメモがあっても本文表示は継続する() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let memo_path = tmp_dir.path().join(".test.md.memo.md");
    tokio::fs::write(&memo_path, [0xff, 0xfe, 0xfd])
        .await
        .unwrap();

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(body.contains("Body"));
    assert!(body.contains("id=\"memo-editor\""));
}

#[tokio::test]
async fn test_apiメモ_getは旧保存先をそのまま読み込む() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let legacy_memo_path = tmp_dir.path().join(".markdown-view/memos/test.md");
    let sidecar_memo_path = tmp_dir.path().join(".test.md.memo.md");
    tokio::fs::create_dir_all(legacy_memo_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_memo_path, "legacy memo")
        .await
        .unwrap();

    let resp = reqwest::get(format!("http://{}/api/memo", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["raw"], "legacy memo");
    assert!(!tokio::fs::try_exists(&sidecar_memo_path).await.unwrap());
    assert!(tokio::fs::try_exists(&legacy_memo_path).await.unwrap());
}

#[tokio::test]
async fn test_apiメモ_putは旧保存先から新sidecarへ自動移行する() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let legacy_memo_path = tmp_dir.path().join(".markdown-view/memos/test.md");
    let sidecar_memo_path = tmp_dir.path().join(".test.md.memo.md");
    tokio::fs::create_dir_all(legacy_memo_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_memo_path, "legacy memo")
        .await
        .unwrap();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "updated memo"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), 200);

    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["raw"], "updated memo");
    assert!(tokio::fs::try_exists(&sidecar_memo_path).await.unwrap());
    assert!(!tokio::fs::try_exists(&legacy_memo_path).await.unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn test_apiメモ_空白保存はunsafeなlegacyがあってもsidecar削除を優先する() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let sidecar_memo_path = tmp_dir.path().join(".test.md.memo.md");
    tokio::fs::write(&sidecar_memo_path, "memo").await.unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    tokio::fs::create_dir_all(outside_dir.path().join("memos"))
        .await
        .unwrap();
    tokio::fs::write(outside_dir.path().join("memos/test.md"), "legacy")
        .await
        .unwrap();
    symlink(outside_dir.path(), tmp_dir.path().join(".markdown-view")).unwrap();

    let delete = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "   \n"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(delete.status(), 200);
    let deleted: serde_json::Value = delete.json().await.unwrap();
    assert_eq!(deleted["raw"], "");
    assert!(!tokio::fs::try_exists(&sidecar_memo_path).await.unwrap());
    assert!(tokio::fs::try_exists(tmp_dir.path().join(".markdown-view"))
        .await
        .unwrap());

    let get = reqwest::get(format!("http://{}/api/memo", addr))
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let fetched: serde_json::Value = get.json().await.unwrap();
    assert_eq!(fetched["raw"], "");
}

#[cfg(unix)]
#[tokio::test]
async fn test_apiメモ_空白保存でsafe_legacy削除失敗ならエラーにする() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let sidecar_memo_path = tmp_dir.path().join(".test.md.memo.md");
    tokio::fs::write(&sidecar_memo_path, "memo").await.unwrap();
    let legacy_memo_path = tmp_dir.path().join(".markdown-view/memos/test.md");
    tokio::fs::create_dir_all(legacy_memo_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_memo_path, "legacy").await.unwrap();

    let legacy_parent = legacy_memo_path.parent().unwrap();
    let original_mode = fs::metadata(legacy_parent).unwrap().permissions().mode();
    fs::set_permissions(legacy_parent, fs::Permissions::from_mode(0o555)).unwrap();

    let delete = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "   \n"
        }))
        .send()
        .await
        .unwrap();

    fs::set_permissions(legacy_parent, fs::Permissions::from_mode(original_mode)).unwrap();

    assert_eq!(delete.status(), reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    let json: serde_json::Value = delete.json().await.unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
    assert!(tokio::fs::try_exists(&sidecar_memo_path).await.unwrap());
    assert!(tokio::fs::try_exists(&legacy_memo_path).await.unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn test_apiメモ_unsafeなlegacy_symlinkがあってもsidecar保存を継続できる() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();

    let outside_dir = tempfile::tempdir().unwrap();
    symlink(outside_dir.path(), tmp_dir.path().join(".markdown-view")).unwrap();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "memo"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 200);
    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["raw"], "memo");
    assert!(
        tokio::fs::try_exists(tmp_dir.path().join(".test.md.memo.md"))
            .await
            .unwrap()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_apiメモ_single_file_既存legacyがあればreadonlyでも更新継続できる() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let legacy_memo_path = tmp_dir.path().join(".markdown-view/memos/test.md");
    tokio::fs::create_dir_all(legacy_memo_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_memo_path, "legacy memo")
        .await
        .unwrap();

    let original_mode = fs::metadata(tmp_dir.path()).unwrap().permissions().mode();
    fs::set_permissions(tmp_dir.path(), fs::Permissions::from_mode(0o555)).unwrap();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "updated memo"
        }))
        .send()
        .await
        .unwrap();

    fs::set_permissions(tmp_dir.path(), fs::Permissions::from_mode(original_mode)).unwrap();

    assert_eq!(save.status(), 200);
    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["raw"], "updated memo");
    assert_eq!(
        tokio::fs::read_to_string(&legacy_memo_path).await.unwrap(),
        "updated memo"
    );
    assert!(
        !tokio::fs::try_exists(tmp_dir.path().join(".test.md.memo.md"))
            .await
            .unwrap()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_apiメモ_保存成功後のlegacy削除失敗は成功扱いにする() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();
    let legacy_memo_path = tmp_dir.path().join(".markdown-view/memos/test.md");
    tokio::fs::create_dir_all(legacy_memo_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_memo_path, "legacy memo")
        .await
        .unwrap();

    let first = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "updated memo"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), 200);

    let legacy_parent = legacy_memo_path.parent().unwrap();
    let original_mode = fs::metadata(legacy_parent).unwrap().permissions().mode();
    fs::set_permissions(legacy_parent, fs::Permissions::from_mode(0o555)).unwrap();

    let second = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "updated again"
        }))
        .send()
        .await
        .unwrap();

    fs::set_permissions(legacy_parent, fs::Permissions::from_mode(original_mode)).unwrap();

    assert_eq!(second.status(), 200);
    let json: serde_json::Value = second.json().await.unwrap();
    assert_eq!(json["raw"], "updated again");
}

#[tokio::test]
async fn test_apiメモ_長いファイル名でもlegacyへfallbackして保存できる() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = tmp_dir.path().join(&file_name);
    tokio::fs::write(&file_path, "# Long").await.unwrap();
    let (_state, addr) = setup_single_file_server_from_path(&file_path).await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "memo"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 200);
    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["raw"], "memo");
    let mut entries = tokio::fs::read_dir(tmp_dir.path()).await.unwrap();
    let mut memo_count = 0usize;
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".memo.md") {
            memo_count += 1;
            assert!(name.len() <= 255);
        }
    }
    assert_eq!(memo_count, 1);
}

#[tokio::test]
async fn test_apiメモ_長いファイル名で未作成時は空を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = tmp_dir.path().join(&file_name);
    tokio::fs::write(&file_path, "# Long").await.unwrap();
    let (_state, addr) = setup_single_file_server_from_path(&file_path).await;

    let resp = reqwest::get(format!("http://{}/api/memo", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["raw"], "");
    assert_eq!(json["html"], "");
}

#[tokio::test]
async fn test_apiメモ_長いファイル名のlegacyメモは空白保存で削除できる() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = tmp_dir.path().join(&file_name);
    tokio::fs::write(&file_path, "# Long").await.unwrap();
    let (_state, addr) = setup_single_file_server_from_path(&file_path).await;
    let client = reqwest::Client::new();
    let legacy_path = tmp_dir.path().join(".markdown-view/memos").join(&file_name);
    tokio::fs::create_dir_all(legacy_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&legacy_path, "memo").await.unwrap();

    let delete = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": " \n "
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(delete.status(), 200);
    let json: serde_json::Value = delete.json().await.unwrap();
    assert_eq!(json["raw"], "");
    assert!(!tokio::fs::try_exists(&legacy_path).await.unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn test_apiメモ_directory_mode_書込不可サブディレクトリではlegacyへfallbackする() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let docs_dir = tmp_dir.path().join("docs");
    tokio::fs::create_dir_all(&docs_dir).await.unwrap();
    tokio::fs::write(tmp_dir.path().join("README.md"), "# README\n\nRoot")
        .await
        .unwrap();
    tokio::fs::write(docs_dir.join("guide.md"), "# Guide\n\nBody")
        .await
        .unwrap();

    let original_mode = fs::metadata(&docs_dir).unwrap().permissions().mode();
    fs::set_permissions(&docs_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let state = build_dir_state(tmp_dir.path());
    let addr = spawn_test_server(state).await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "file": "docs/guide.md",
            "raw": "memo"
        }))
        .send()
        .await
        .unwrap();

    fs::set_permissions(&docs_dir, fs::Permissions::from_mode(original_mode)).unwrap();

    assert_eq!(save.status(), 200);
    let json: serde_json::Value = save.json().await.unwrap();
    assert_eq!(json["raw"], "memo");
    assert!(!tokio::fs::try_exists(docs_dir.join(".guide.md.memo.md"))
        .await
        .unwrap());
    assert!(
        tokio::fs::try_exists(tmp_dir.path().join(".markdown-view/memos/docs/guide.md"))
            .await
            .unwrap()
    );
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
    let msg = next_ws_message(&mut read).await;

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
    let _initial_message = next_ws_message(&mut read).await;

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
    let msg = next_ws_message(&mut read).await;

    let text = msg.into_text().unwrap();
    assert!(text.contains("updated"));
}

#[tokio::test]
async fn test_存在しないファイル時は404を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("deleted.md");
    tokio::fs::write(&file_path, "# before delete")
        .await
        .unwrap();
    let (_state, addr) = setup_single_file_server_from_path(&file_path).await;

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
    let (_state, addr, _tmp_dir, _file_path) =
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
async fn test_ファイル変更でwebsocket更新() {
    // 一時ファイルを作成
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("watch_test.md");
    tokio::fs::write(&file_path, "# Before").await.unwrap();

    let (state, addr) = setup_single_file_server_from_path(&file_path).await;

    // ファイル監視開始
    let watch_service = WatchService::start(state.clone()).await.unwrap();

    // WebSocket接続
    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // 初期メッセージを消費
    let _initial_message = next_ws_message(&mut read).await;

    // ファイルを変更
    tokio::fs::write(&file_path, "# After Change")
        .await
        .unwrap();

    // WebSocketで更新を受信（debounce 300ms + αのタイムアウト）
    let msg = next_ws_message(&mut read).await;

    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(json["content"].as_str().unwrap().contains("After Change"));
    watch_service.shutdown().await;
    drop(tmp_dir);
}

#[tokio::test]
async fn test_ファイル削除でwebsocketエラー通知() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("watch_delete.md");
    tokio::fs::write(&file_path, "# Before").await.unwrap();

    let (state, addr) = setup_single_file_server_from_path(&file_path).await;

    let watch_service = WatchService::start(state.clone()).await.unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    let _initial_message = next_ws_message(&mut read).await;

    tokio::fs::remove_file(&file_path).await.unwrap();

    let msg = next_ws_message(&mut read).await;

    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let error = json["error"].as_str().expect("errorフィールドが存在する");
    assert!(error.contains("ファイル検証エラー"));
    assert!(error.contains("watch_delete.md"));
    watch_service.shutdown().await;
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
    tokio::time::timeout(Duration::from_secs(5), ws_stream.next())
        .await
        .expect("初期メッセージ受信がタイムアウト");

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

    let (_state, addr) = setup_single_file_server_from_path(&large_file).await;

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

    let (_state, addr) = setup_single_file_server_from_path(&limit_file).await;

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
    assert!(csp.contains("img-src 'self'"));
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

#[cfg(unix)]
#[tokio::test]
async fn test_単一ファイルモード_シンボリックリンク差し替えを拒否する() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Test").await;
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
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Test").await;

    let resp = reqwest::get(format!("http://{}/api/files", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: Vec<String> = resp.json().await.unwrap();
    assert!(json.is_empty());
}

#[tokio::test]
async fn test_単一ファイルモードの後方互換_api_searchは空結果() {
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Test").await;

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
async fn test_ディレクトリモード_検索apiは複数ファイルから結果を返す() {
    let tmp_dir = tempfile::tempdir().unwrap();
    tokio::fs::write(
        tmp_dir.path().join("README.md"),
        "# README\n\nAlpha note appears here.",
    )
    .await
    .unwrap();
    tokio::fs::create_dir_all(tmp_dir.path().join("docs"))
        .await
        .unwrap();
    tokio::fs::write(
        tmp_dir.path().join("docs/guide.md"),
        "# Guide\n\nAnother alpha note appears there.",
    )
    .await
    .unwrap();

    let state = build_dir_state(tmp_dir.path());
    let addr = spawn_test_server(state).await;

    let resp = reqwest::get(format!("http://{}/api/search?q=alpha%20note", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["query"].as_str().unwrap(), "alpha note");
    assert_eq!(json["searched_files"].as_u64().unwrap(), 2);
    assert_eq!(json["skipped_files"].as_u64().unwrap(), 0);

    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["file"].as_str().unwrap(), "README.md");
    assert_eq!(results[1]["file"].as_str().unwrap(), "docs/guide.md");
    assert!(results[0]["current"]
        .as_str()
        .unwrap()
        .contains("Alpha note appears here."));
}

#[tokio::test]
async fn test_ディレクトリモード_検索apiは巨大ファイルをスキップする() {
    let tmp_dir = tempfile::tempdir().unwrap();
    tokio::fs::write(
        tmp_dir.path().join("README.md"),
        "# README\n\nAlpha note appears here.",
    )
    .await
    .unwrap();
    tokio::fs::write(
        tmp_dir.path().join("large.md"),
        "a".repeat((10 * 1024 * 1024) + 1),
    )
    .await
    .unwrap();

    let state = build_dir_state(tmp_dir.path());
    let addr = spawn_test_server(state).await;

    let resp = reqwest::get(format!("http://{}/api/search?q=alpha%20note", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["searched_files"].as_u64().unwrap(), 1);
    assert_eq!(json["skipped_files"].as_u64().unwrap(), 1);
    assert_eq!(json["results"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_ディレクトリモード_api_searchは不正hostを拒否する() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;
    let client = reqwest::Client::new();
    let attack_host = format!("evil.example:{}", addr.port());

    let resp = client
        .get(format!("http://{}/api/search?q=readme", addr))
        .header("Host", &attack_host)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
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
async fn test_ディレクトリモード_メモapiのパストラバーサルを拒否する() {
    let (_state, addr, _tmp_dir) = setup_dir_server().await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "file": "../../etc/passwd",
            "raw": "attack"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), reqwest::StatusCode::NOT_FOUND);

    let get = client
        .get(format!("http://{}/api/memo?file=../../etc/passwd", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), reqwest::StatusCode::NOT_FOUND);
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
    let watch_service = markdown_view::server::WatchService::start(state.clone())
        .await
        .unwrap();

    // WebSocket接続
    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    let file_path = tmp_dir.path().join("README.md");
    tokio::fs::write(&file_path, "# README\n\nUpdated content")
        .await
        .unwrap();

    let msg = next_ws_message(&mut read).await;

    let text = msg
        .into_text()
        .expect("WebSocketメッセージのテキスト変換に失敗");
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSONパースに失敗");
    assert!(json["content"]
        .as_str()
        .unwrap()
        .contains("Updated content"));
    assert_eq!(json["file"].as_str().unwrap(), "README.md");
    watch_service.shutdown().await;
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
    let _initial_message = next_ws_message(&mut read).await;

    // broadcastでエラーJSONを送信（watcher.rsのbroadcast_errorと同じ形式）
    state
        .tx()
        .send(BroadcastMessage::Error(
            "ファイル監視エラー: テスト用エラー".to_string(),
        ))
        .unwrap();

    // WebSocketでエラーJSONを受信
    let msg = next_ws_message(&mut read).await;

    let text = msg
        .into_text()
        .expect("WebSocketメッセージのテキスト変換に失敗");
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSONパースに失敗");
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
    let (_state, addr, _tmp_dir) = setup_single_file_server("# Test\n\nHello").await;

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
    let (_state, addr, _tmp_dir) =
        setup_single_file_server("# Heading\n\nLine one\n\nLine two").await;

    let resp = reqwest::get(format!("http://{}/", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(body.contains("data-source-start-line=\"1\""));
    assert!(body.contains("data-source-end-line=\"1\""));
    assert!(body.contains("id=\"quote-selection-action\""));
    assert!(body.contains("function buildQuoteMarkdownFromSelection()"));
}

// ==============================
// WebSocket close frame テスト
// ==============================

#[tokio::test]
async fn test_websocket_non_utf8ファイルでclose_frameにuser_messageが含まれる() {
    let (_state, addr, _tmp_dir, _file_path) =
        setup_single_file_server_with_bytes("binary.md", &[0xff, 0xfe, 0xfd]).await;

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // サーバーがclose frameを送信するのを受信
    assert_close_frame_message(
        &mut read,
        tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Unsupported,
        "このファイルはUTF-8テキストではありません",
    )
    .await;
}

#[tokio::test]
async fn test_websocket_削除済みファイルでclose_frameにuser_messageが含まれる() {
    let (_state, addr, _tmp_dir, file_path) =
        setup_single_file_server_with_bytes("deleted.md", b"# before delete").await;

    // AppMode生成後にファイルを削除
    tokio::fs::remove_file(&file_path).await.unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    assert_close_frame_message(
        &mut read,
        tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Policy,
        "ファイル検証に失敗しました: ファイルが見つかりません",
    )
    .await;
}

#[tokio::test]
async fn test_websocket_サイズ超過ファイルでclose_frameにuser_messageが含まれる() {
    let content = "x".repeat(10 * 1024 * 1024 + 1);
    let (_state, addr, _tmp_dir, _file_path) =
        setup_single_file_server_with_bytes("large.md", content.as_bytes()).await;

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    assert_close_frame_message(
        &mut read,
        tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Size,
        "ファイルサイズが上限（10MB）を超えています",
    )
    .await;
}

// ==============================
// ヘルパー関数
// ==============================

fn build_single_file_state(file_path: &Path) -> Arc<AppState> {
    let (tx, _rx) = broadcast::channel(16);
    Arc::new(AppState::new(
        AppMode::new_single_file(file_path).unwrap(),
        false,
        None,
        tx,
    ))
}

fn build_dir_state(base_dir: &Path) -> Arc<AppState> {
    let (tx, _rx) = broadcast::channel(16);
    Arc::new(AppState::new(
        AppMode::new_directory(base_dir).unwrap(),
        false,
        None,
        tx,
    ))
}

async fn spawn_test_server(state: Arc<AppState>) -> std::net::SocketAddr {
    let router = markdown_view::server::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    addr
}

async fn setup_single_file_server_from_path(
    file_path: &Path,
) -> (Arc<AppState>, std::net::SocketAddr) {
    let state = build_single_file_state(file_path);
    let addr = spawn_test_server(state.clone()).await;
    (state, addr)
}

async fn setup_single_file_server_with_bytes(
    file_name: &str,
    content: &[u8],
) -> (
    Arc<AppState>,
    std::net::SocketAddr,
    tempfile::TempDir,
    std::path::PathBuf,
) {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join(file_name);
    tokio::fs::write(&file_path, content).await.unwrap();
    let (state, addr) = setup_single_file_server_from_path(&file_path).await;
    (state, addr, tmp_dir, file_path)
}

async fn assert_json_error_for_paths(
    addr: std::net::SocketAddr,
    paths: &[&str],
    expected_status: reqwest::StatusCode,
    expected_message: Option<&str>,
) {
    for path in paths {
        let resp = reqwest::get(format!("http://{}{}", addr, path))
            .await
            .unwrap();
        assert_eq!(resp.status(), expected_status);
        let json: serde_json::Value = resp.json().await.unwrap();
        match expected_message {
            Some(message) => assert_eq!(json["error"].as_str().unwrap(), message),
            None => assert!(json["error"].as_str().is_some()),
        }
    }
}

/// 単一ファイルモードのテスト用サーバーセットアップ
async fn setup_single_file_server(
    markdown_content: &str,
) -> (Arc<AppState>, std::net::SocketAddr, tempfile::TempDir) {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("test.md");
    tokio::fs::write(&file_path, markdown_content)
        .await
        .unwrap();

    let (state, addr) = setup_single_file_server_from_path(&file_path).await;

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

    let state = build_dir_state(tmp_dir.path());
    let addr = spawn_test_server(state.clone()).await;

    (state, addr, tmp_dir)
}

async fn connect_ws(
    url: &str,
    origin: &str,
) -> Result<
    (
        WsStream,
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
        WsStream,
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

async fn next_ws_message(read: &mut WsReadHalf) -> tokio_tungstenite::tungstenite::Message {
    tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .expect("WebSocketメッセージ受信がタイムアウト")
        .expect("WebSocketストリームが予期せず終了")
        .expect("WebSocketメッセージの読み取りに失敗")
}

async fn assert_close_frame_message(
    read: &mut WsReadHalf,
    expected_code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode,
    expected_reason: &str,
) {
    let msg = next_ws_message(read).await;
    match msg {
        tokio_tungstenite::tungstenite::Message::Close(Some(frame)) => {
            assert_eq!(frame.code, expected_code);
            let reason: &str = frame.reason.as_ref();
            assert_eq!(reason, expected_reason);
        }
        other => panic!("Close frameを期待したが {:?} を受信", other),
    }
}
