//! WebSocketセッションの送受信ループを管理する。

use std::sync::Arc;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use tokio::sync::broadcast;

use super::files::build_lagged_recovery_message;
use super::files::load_initial_socket_update;
use super::messages::BroadcastMessage;
use super::state::AppState;
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

pub(super) async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.tx().subscribe();

    let initial = match load_initial_socket_update(state.as_ref()).await {
        Ok(update) => update,
        Err(error) => {
            let _ = send_close_frame(&mut socket, error.close_code(), error.reason()).await;
            return;
        }
    };
    if let Some(update) = initial {
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
                        let recovery = build_lagged_recovery_message(state.as_ref()).await;
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
