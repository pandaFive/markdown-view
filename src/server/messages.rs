//! WebSocket/HTTP応答で共有するサーバーメッセージ型とAPIエラー型を管理する。

use axum::http::StatusCode;
use axum::response::Json;

use crate::template::{error_message_json, UpdateMessage};

/// HTTP APIエラー応答の共通型
pub(super) type ApiError = (StatusCode, Json<serde_json::Value>);

/// WebSocket broadcastメッセージ
#[derive(Debug, Clone)]
pub enum BroadcastMessage {
    /// コンテンツ更新
    Update(UpdateMessage),
    /// クライアントに再取得を促す
    Refresh,
    /// エラー通知
    Error(String),
}

impl BroadcastMessage {
    pub(super) fn to_json(&self) -> Result<String, serde_json::Error> {
        match self {
            BroadcastMessage::Update(update) => serde_json::to_string(update),
            BroadcastMessage::Refresh => serde_json::to_string(&serde_json::json!({
                "refresh": true
            })),
            BroadcastMessage::Error(message) => serde_json::to_string(&error_message_json(message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broadcast_message_refreshのjson直列化() {
        let json = BroadcastMessage::Refresh.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value, serde_json::json!({ "refresh": true }));
    }

    #[test]
    fn test_broadcast_message_errorのjson直列化() {
        let json = BroadcastMessage::Error("watcher error".to_string())
            .to_json()
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value, serde_json::json!({ "error": "watcher error" }));
    }
}
