#![allow(unused_imports)]

use std::sync::Arc;
use std::time::Duration;
#[cfg(unix)]
use std::{fs, os::unix::fs::symlink};

use futures_util::StreamExt;
use tokio::sync::broadcast;

use markdown_view::renderer::render_markdown;
use markdown_view::server::{AppMode, AppState, BroadcastMessage, WatchService};
use markdown_view::template::UpdateMessage;
use markdown_view::toc::generate_toc;

use super::support::*;

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
async fn test_単一ファイルモード_atomic_save後にwebsocket更新() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("atomic_single.md");
    tokio::fs::write(&file_path, "# Before Atomic Save")
        .await
        .unwrap();

    let (state, addr) = setup_single_file_server_from_path(&file_path).await;
    let watch_service = WatchService::start(state.clone()).await.unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    let _initial_message = next_ws_message(&mut read).await;

    atomic_save_markdown_file(&file_path, "# After Atomic Save");

    let msg = next_ws_message(&mut read).await;
    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(json["content"]
        .as_str()
        .unwrap()
        .contains("After Atomic Save"));
    assert!(watch_service.is_alive());

    watch_service.shutdown().await;
    drop(tmp_dir);
}
#[cfg(unix)]
#[tokio::test]
async fn test_ファイル変更_io_エラーでwebsocketエラー通知() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("watch_io_error.md");
    tokio::fs::write(&file_path, "# Before").await.unwrap();

    let (state, addr) = setup_single_file_server_from_path(&file_path).await;
    let watch_service = WatchService::start(state.clone()).await.unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // 初期メッセージを消費
    let _initial_message = next_ws_message(&mut read).await;

    // notify 発火後、debounce window 内に chmod 0o000 で open(2) を EACCES に落とす
    tokio::fs::write(&file_path, "# After").await.unwrap();
    let Some(permission_guard) = make_file_unreadable(&file_path) else {
        watch_service.shutdown().await;
        drop(tmp_dir);
        return;
    };

    // debounce 経過後に build_change_broadcast_message が走り、Io arm が Error broadcast を発信
    let msg = next_ws_message(&mut read).await;
    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let error = json["error"].as_str().expect("errorフィールドが存在する");
    assert_eq!(
        error,
        "ファイル読み込みエラー (watch_io_error.md): ファイルの読み込みに失敗しました"
    );

    // 権限復元を明示し、後続の shutdown / cleanup に読み取り不可状態を持ち越さない
    drop(permission_guard);
    watch_service.shutdown().await;
    drop(tmp_dir);
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
async fn test_ディレクトリモード_atomic_save後にwebsocket更新() {
    let (state, addr, tmp_dir) = setup_dir_server().await;
    let watch_service = markdown_view::server::WatchService::start(state.clone())
        .await
        .unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    let initial = tokio::time::timeout(Duration::from_millis(500), read.next()).await;
    assert!(
        initial.is_err(),
        "ディレクトリモードでは更新前に初期WebSocketメッセージを送信しない"
    );

    let file_path = tmp_dir.path().join("README.md");
    atomic_save_markdown_file(&file_path, "# README\n\nAfter Atomic Save");

    let msg = next_ws_message(&mut read).await;

    let text = msg
        .into_text()
        .expect("WebSocketメッセージのテキスト変換に失敗");
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSONパースに失敗");
    assert!(json["content"]
        .as_str()
        .unwrap()
        .contains("After Atomic Save"));
    assert_eq!(json["file"].as_str().unwrap(), "README.md");
    assert!(watch_service.is_alive());

    watch_service.shutdown().await;
}
#[cfg(unix)]
#[tokio::test]
async fn test_ディレクトリモード_websocket更新はbackslashファイル名を保持する() {
    let (state, addr, tmp_dir) = setup_dir_server().await;
    let file_path = tmp_dir.path().join("back\\slash.md");
    tokio::fs::write(&file_path, "# Backslash\n\nBefore")
        .await
        .unwrap();
    let watch_service = markdown_view::server::WatchService::start(state.clone())
        .await
        .unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    let initial = tokio::time::timeout(Duration::from_millis(500), read.next()).await;
    assert!(
        initial.is_err(),
        "ディレクトリモードでは更新前に初期WebSocketメッセージを送信しない"
    );

    tokio::fs::write(&file_path, "# Backslash\n\nAfter")
        .await
        .unwrap();

    let msg = next_ws_message(&mut read).await;

    let text = msg
        .into_text()
        .expect("WebSocketメッセージのテキスト変換に失敗");
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSONパースに失敗");
    assert!(json["content"].as_str().unwrap().contains("After"));
    assert_eq!(json["file"].as_str().unwrap(), "back\\slash.md");
    watch_service.shutdown().await;
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
async fn test_websocket_non_utf8ファイルでclose_frameにuser_messageが含まれる() {
    let (_state, addr, _tmp_dir, _file_path) =
        setup_single_file_server_with_bytes("binary.md", &[0xff, 0xfe, 0xfd]).await;

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // サーバーがclose frameを送信するのを受信
    assert_close_frame_message(&mut read, 1003, "このファイルはUTF-8テキストではありません").await;
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
        1008,
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
        1009,
        "ファイルサイズが上限（10MB）を超えています",
    )
    .await;
}
#[cfg(unix)]
#[tokio::test]
async fn test_websocket_ioエラーでclose_frameが1011を返す() {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::PermissionsExt;

    let (_state, addr, _tmp_dir, file_path) =
        setup_single_file_server_with_bytes("unreadable.md", b"# content").await;

    // 読込 IO を誘発: resolve (canonicalize/is_file) はパスするが open(2) が EACCES で失敗
    let original_mode = fs::metadata(&file_path).unwrap().permissions().mode();
    fs::set_permissions(&file_path, Permissions::from_mode(0o000)).unwrap();

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    assert_close_frame_message(&mut read, 1011, "ファイルの読み込みに失敗しました").await;

    // teardown: TempDir drop で失敗しないよう権限を復元
    fs::set_permissions(&file_path, Permissions::from_mode(original_mode)).unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する() {
    // current_thread runtime 前提: tokio::test のデフォルト。flavor = "multi_thread" を
    // 指定すると burst 中に session task が並走してしまい、決定的に Lagged を誘発できない。
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("lagged_io.md");
    tokio::fs::write(&file_path, "# Before lagged")
        .await
        .unwrap();

    let (state, addr) = setup_single_file_server_from_path(&file_path).await;

    let url = format!("ws://{}/ws", addr);
    let (ws_stream, _) = connect_ws(&url, &format!("http://{}", addr)).await.unwrap();
    let (_write, mut read) = ws_stream.split();

    // 初期 Update を消費。session task が rx.subscribe 済みかつ recv ループに入った
    // ことの暗黙的バリアになる。
    let _initial_message = next_ws_message(&mut read).await;

    // recovery 実行中は chmod 0o000 が維持されている必要があるため、burst 直前で取得し
    // assert 完了後に drop して権限復元する。
    let Some(permission_guard) = make_file_unreadable(&file_path) else {
        drop(tmp_dir);
        return;
    };

    // burst 件数 = 容量 16 + 1 = 17。await を挟まず synchronous に発火することで、
    // 受信タスクが起きる前にチャネルが overflow し、次の recv で Lagged(1) が確定する。
    for _ in 0..17 {
        state.tx().send(BroadcastMessage::Refresh).unwrap();
    }

    // 次フレームは Lagged → build_lagged_recovery_message → Io ReadFailed →
    // BroadcastMessage::Error 経路で送出される
    let msg = next_ws_message(&mut read).await;
    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let error = json["error"].as_str().expect("errorフィールドが存在する");
    assert_eq!(
        error,
        "ファイル読み込みエラー (lagged_io.md): ファイルの読み込みに失敗しました"
    );

    // 順序: assert 後に権限復元 → tempdir 自動 drop。recovery 実行中は guard 生存中で
    // chmod 0o000 が維持されている必要がある。
    drop(permission_guard);
    drop(tmp_dir);
}
