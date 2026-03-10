//! WebSocketセッションと変更通知ブロードキャストを管理する。

use std::path::Path;
use std::sync::Arc;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use tokio::sync::{broadcast, mpsc};

use super::files::{read_and_render_file, revalidate_single_file_target};
use super::messages::BroadcastMessage;
use super::state::AppState;
use crate::template::error_message_json;
use crate::watcher::WatchEvent;

async fn notify_ws_internal_error(socket: &mut WebSocket, message: &str) -> bool {
    let payload = serde_json::to_string(&error_message_json(message))
        .unwrap_or_else(|_| r#"{"error":"内部エラーが発生しました"}"#.to_string());
    if let Err(e) = socket.send(Message::Text(payload.into())).await {
        tracing::warn!("[markdown-view] WebSocket内部エラー通知送信失敗: {}", e);
        return false;
    }
    true
}

pub(super) async fn lagged_recovery_message(state: &AppState) -> BroadcastMessage {
    if let Some(file_path) = state.mode().single_file() {
        let validated_path = match revalidate_single_file_target(file_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("[markdown-view] WebSocket再送信時のファイル検証失敗: {}", e);
                return BroadcastMessage::Error(format!("ファイル検証エラー: {}", e));
            }
        };
        match read_and_render_file(&validated_path).await {
            Ok(update) => BroadcastMessage::Update(update),
            Err(e) => {
                let file_label = file_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| file_path.display().to_string());
                tracing::warn!(
                    "[markdown-view] WebSocket再送信読み込みエラー ({}): {}",
                    file_label,
                    e
                );
                BroadcastMessage::Error(format!(
                    "ファイル読み込みエラー ({}): {}",
                    file_label,
                    e.user_message()
                ))
            }
        }
    } else {
        BroadcastMessage::Refresh
    }
}

pub(super) async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.tx().subscribe();

    if let Some(file_path) = state.mode().single_file() {
        let validated_path = match revalidate_single_file_target(file_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("[markdown-view] WebSocket初期ファイル検証失敗: {}", e);
                let _ = send_close_frame(&mut socket, 1008, "ファイル検証に失敗しました").await;
                return;
            }
        };
        let update = match read_and_render_file(&validated_path).await {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("[markdown-view] WebSocket初期読み込みエラー: {}", e);
                let _ = send_close_frame(&mut socket, e.close_code(), e.user_message()).await;
                return;
            }
        };
        let msg = match BroadcastMessage::Update(update).to_json() {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!("[markdown-view] JSONシリアライズエラー: {}", e);
                if !notify_ws_internal_error(&mut socket, "更新メッセージの直列化に失敗しました")
                    .await
                {
                    return;
                }
                let _ = send_close_frame(&mut socket, 1011, "内部エラー").await;
                return;
            }
        };
        if let Err(e) = socket.send(Message::Text(msg.into())).await {
            tracing::warn!("[markdown-view] WebSocket初期送信エラー: {}", e);
            return;
        }
    }

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) => {
                        break;
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if let Err(e) = socket.send(Message::Pong(payload)).await {
                            tracing::warn!("[markdown-view] WebSocket pong送信エラー: {}", e);
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::warn!("[markdown-view] WebSocket受信エラー: {}", e);
                        break;
                    }
                    None => {
                        break;
                    }
                }
            }
            recv = rx.recv() => {
                match recv {
                    Ok(msg) => {
                        let json = match msg.to_json() {
                            Ok(json) => json,
                            Err(e) => {
                                tracing::warn!("[markdown-view] WebSocketメッセージJSON化エラー: {}", e);
                                if !notify_ws_internal_error(
                                    &mut socket,
                                    "WebSocketメッセージの直列化に失敗しました",
                                )
                                .await
                                {
                                    break;
                                }
                                continue;
                            }
                        };
                        if let Err(e) = socket.send(Message::Text(json.into())).await {
                            tracing::warn!("[markdown-view] WebSocket送信エラー: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(
                            "[markdown-view] WebSocketクライアントが{}メッセージ遅延",
                            n
                        );
                        let recovery = lagged_recovery_message(state.as_ref()).await;
                        let payload = match recovery.to_json() {
                            Ok(json) => json,
                            Err(e) => {
                                tracing::warn!("[markdown-view] 遅延回復メッセージの直列化に失敗: {}", e);
                                if !notify_ws_internal_error(
                                    &mut socket,
                                    "遅延回復メッセージの直列化に失敗しました",
                                )
                                .await
                                {
                                    break;
                                }
                                continue;
                            }
                        };
                        if let Err(e) = socket.send(Message::Text(payload.into())).await {
                            tracing::warn!("[markdown-view] WebSocket遅延回復メッセージ送信エラー: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        }
    }
}

async fn send_close_frame(socket: &mut WebSocket, code: u16, reason: impl Into<String>) -> bool {
    let reason = reason.into();
    if let Err(e) = socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })))
        .await
    {
        tracing::warn!("[markdown-view] WebSocket closeフレーム送信エラー: {}", e);
        return false;
    }
    true
}

/// ファイル変更時にbroadcastで全クライアントに通知する
///
/// ディレクトリモードでは変更ファイルの相対パスを`file`フィールドに含め、
/// クライアント側でアクティブタブの更新判定に使用する。
/// ファイル検証や読み込みに失敗した場合はエラーメッセージをbroadcastする。
/// 受信者がゼロの場合は早期リターンする。
pub async fn notify_update(state: &AppState, changed_file: &Path) {
    if state.tx().receiver_count() == 0 {
        return;
    }

    let read_path: &Path = if let Some(expected) = state.mode().single_file() {
        match revalidate_single_file_target(expected) {
            Ok(_) => changed_file,
            Err(e) => {
                let file_label = changed_file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| changed_file.display().to_string());
                tracing::warn!(
                    "[markdown-view] 更新時ファイル検証失敗 ({}): {}",
                    file_label,
                    e
                );
                let msg =
                    BroadcastMessage::Error(format!("ファイル検証エラー ({}): {}", file_label, e));
                let _ = state.tx().send(msg);
                return;
            }
        }
    } else {
        changed_file
    };

    let relative_path = state.mode().relative_path_of(changed_file);

    if state.mode().is_directory() && relative_path.is_none() {
        tracing::warn!(
            "[markdown-view] 相対パス算出失敗のためブロードキャストをスキップ: {}",
            changed_file.display()
        );
        return;
    }

    let msg = match read_and_render_file(read_path).await {
        Ok(update) => BroadcastMessage::Update(update.with_file(relative_path)),
        Err(e) => {
            let file_label = relative_path
                .as_deref()
                .map(|s| s.to_string())
                .or_else(|| {
                    changed_file
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| changed_file.display().to_string());
            tracing::warn!(
                "[markdown-view] 更新時読み込みエラー ({}): {}",
                file_label,
                e
            );
            BroadcastMessage::Error(format!(
                "ファイル読み込みエラー ({}): {}",
                file_label,
                e.user_message()
            ))
        }
    };
    let _ = state.tx().send(msg);
}

/// 監視イベントをWebSocketブロードキャストへ転送する
pub fn spawn_watch_event_forwarder(
    state: Arc<AppState>,
    mut rx: mpsc::Receiver<WatchEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                WatchEvent::FileChanged(changed_path) => {
                    notify_update(&state, &changed_path).await;
                }
                WatchEvent::Error(error_msg) => {
                    broadcast_error(&state, &error_msg);
                }
            }
        }
        tracing::info!("[markdown-view] ファイル変更通知タスクが終了しました");
    })
}

fn broadcast_error(state: &AppState, error_msg: &str) {
    let _ = state.tx().send(BroadcastMessage::Error(format!(
        "ファイル監視エラー: {}",
        error_msg
    )));
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use tokio::sync::{broadcast, mpsc};

    use super::*;
    use crate::server::{AppMode, AppState, MAX_FILE_SIZE};

    #[tokio::test]
    async fn test_notify_update_ディレクトリモードで相対パス算出失敗時は送信をスキップ() {
        let base_dir = tempfile::tempdir().unwrap();
        std::fs::write(base_dir.path().join("README.md"), "# README").unwrap();

        let outside_dir = tempfile::tempdir().unwrap();
        let outside_file = outside_dir.path().join("outside.md");
        std::fs::write(&outside_file, "# outside").unwrap();
        let outside_canonical = outside_file.canonicalize().unwrap();

        let state = create_directory_state(base_dir.path());
        let mut rx = state.tx().subscribe();

        notify_update(&state, &outside_canonical).await;
        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn test_notify_update_ディレクトリモードで読み込み失敗時はerrorを送信する() {
        let base_dir = tempfile::tempdir().unwrap();
        let target = base_dir.path().join("README.md");
        std::fs::write(&target, "# before").unwrap();

        let state = create_directory_state(base_dir.path());
        let mut rx = state.tx().subscribe();

        std::fs::remove_file(&target).unwrap();
        notify_update(&state, &target).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイル読み込みエラー"));
                assert!(
                    message.contains("README.md"),
                    "エラーメッセージにファイル名が含まれるべき: {}",
                    message
                );
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_ファイル名不明時はdisplay表示がエラーに含まれる() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("dummy.md");
        std::fs::write(&file_path, "# dummy").unwrap();

        let state = create_single_file_state(&file_path);
        let mut rx = state.tx().subscribe();

        notify_update(&state, Path::new("/")).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(
                    message.contains("/"),
                    "ファイル名不明時はdisplay()表示が含まれるべき: {}",
                    message
                );
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで読み込み失敗時はファイル名を含むエラーを送信する(
    ) {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("test.md", "# test");
        let mut rx = state.tx().subscribe();

        std::fs::remove_file(&file_path).unwrap();
        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイル検証エラー"));
                assert!(message.contains("test.md"));
                assert!(!message.contains("No such file"));
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードでサイズ超過時はtoo_largeエラーを送信する() {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("large.md", "# large");
        let mut rx = state.tx().subscribe();

        let file = std::fs::File::options()
            .write(true)
            .open(&file_path)
            .unwrap();
        file.set_len(MAX_FILE_SIZE + 1).unwrap();

        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイルサイズが上限"));
                assert!(message.contains("large.md"));
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで非utf8ファイルはnot_utf8エラーを送信する() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("binary.md", "# valid");
        let mut rx = state.tx().subscribe();

        std::fs::write(&file_path, b"\xff\xfe\x80\x81").unwrap();
        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("UTF-8"));
                assert!(message.contains("binary.md"));
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで受信者ゼロ時は送信をスキップする() {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("test.md", "# test");
        let rx = state.tx().subscribe();
        drop(rx);

        std::fs::remove_file(&file_path).unwrap();
        notify_update(&state, &file_path).await;

        let mut rx = state.tx().subscribe();
        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで正常更新時はupdateを送信する() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("hello.md", "# hello world");
        let mut rx = state.tx().subscribe();

        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Update(update) => {
                assert!(update.content().as_str().contains("hello world"));
                assert!(update.toc().as_str().contains("hello-world"));
                assert!(update.file().is_none());
            }
            other => panic!("Updateメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_spawn_watch_event_forwarder_監視エラーをbroadcastする() {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("test.md", "# test");
        let state = Arc::new(state);
        let mut broadcast_rx = state.tx().subscribe();
        let (tx, rx) = mpsc::channel(4);
        let forwarder = spawn_watch_event_forwarder(state.clone(), rx);

        tx.send(WatchEvent::Error("テストエラー".to_string()))
            .await
            .unwrap();
        drop(tx);

        let received = broadcast_rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイル監視エラー: テストエラー"));
                assert!(file_path.exists());
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }

        forwarder.await.unwrap();
    }

    #[tokio::test]
    async fn test_lagged_recovery_message_単一ファイルモードは再読み込みしたupdateを返す() {
        let (_dir, _file_path, state) = create_single_file_state_with_fixture("test.md", "# title");

        let msg = lagged_recovery_message(&state).await;
        match msg {
            BroadcastMessage::Update(update) => {
                assert!(update.content().as_str().contains("title"));
            }
            other => panic!("Updateを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_lagged_recovery_message_ディレクトリモードはrefreshを返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# title").unwrap();
        let state = create_directory_state(dir.path());

        let msg = lagged_recovery_message(&state).await;
        assert!(matches!(msg, BroadcastMessage::Refresh));
    }

    #[tokio::test]
    async fn test_lagged_recovery_message_単一ファイル読み込み失敗時はerrorを返す() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("missing.md", "# title");

        std::fs::remove_file(&file_path).unwrap();
        let msg = lagged_recovery_message(&state).await;
        match msg {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイル検証エラー"));
            }
            other => panic!("Errorを期待したが {:?} を受信", other),
        }
    }

    fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    fn create_single_file_state(file_path: &Path) -> AppState {
        let (tx, _rx) = broadcast::channel(16);
        AppState::new(
            AppMode::new_single_file(file_path).unwrap(),
            false,
            None,
            tx,
        )
    }

    fn create_directory_state(dir_path: &Path) -> AppState {
        let (tx, _rx) = broadcast::channel(16);
        AppState::new(AppMode::new_directory(dir_path).unwrap(), false, None, tx)
    }

    fn create_single_file_state_with_fixture(
        name: &str,
        content: &str,
    ) -> (tempfile::TempDir, PathBuf, AppState) {
        let (dir, file_path) = create_markdown_fixture(name, content);
        let state = create_single_file_state(&file_path);
        (dir, file_path, state)
    }
}
