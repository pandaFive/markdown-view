use std::path::Path;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use tokio::sync::broadcast;

use super::files::{read_and_render_file, revalidate_single_file_target};
use super::{AppState, BroadcastMessage};
use crate::template::error_message_json;

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
                if let Err(send_err) = socket
                    .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                        code: 1008,
                        reason: "ファイル検証に失敗しました".into(),
                    })))
                    .await
                {
                    tracing::warn!(
                        "[markdown-view] WebSocket closeフレーム送信エラー: {}",
                        send_err
                    );
                }
                return;
            }
        };
        let update = match read_and_render_file(&validated_path).await {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("[markdown-view] WebSocket初期読み込みエラー: {}", e);
                if let Err(e) = socket
                    .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                        code: e.close_code(),
                        reason: e.user_message().into(),
                    })))
                    .await
                {
                    tracing::warn!("[markdown-view] WebSocket closeフレーム送信エラー: {}", e);
                }
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
                if let Err(e) = socket
                    .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                        code: 1011,
                        reason: "内部エラー".into(),
                    })))
                    .await
                {
                    tracing::warn!("[markdown-view] WebSocket closeフレーム送信エラー: {}", e);
                }
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
