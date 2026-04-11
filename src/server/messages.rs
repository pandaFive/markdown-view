//! WebSocket/HTTP応答で共有するサーバーメッセージ型とAPIエラー型を管理する。

use axum::http::StatusCode;
use axum::response::Json;

use crate::template::{error_message_json, MemoUpdateMessage, UpdateMessage};

/// HTTP APIエラー応答の共通型
pub(super) type ApiError = (StatusCode, Json<serde_json::Value>);

/// WebSocket broadcastメッセージ
#[derive(Debug, Clone)]
pub enum BroadcastMessage {
    /// コンテンツ更新
    Update(UpdateMessage),
    /// メモ更新
    MemoUpdate(MemoUpdateMessage),
    /// クライアントに再取得を促す
    Refresh,
    /// エラー通知
    Error(String),
}

impl BroadcastMessage {
    pub(super) fn to_json(&self) -> Result<String, serde_json::Error> {
        match self {
            BroadcastMessage::Update(update) => serde_json::to_string(update),
            BroadcastMessage::MemoUpdate(update) => serde_json::to_string(update),
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
    use crate::renderer::render_markdown;
    use crate::template::MemoUpdateMessage;

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

    #[test]
    fn test_broadcast_message_memo_updateのjson直列化() {
        let json = BroadcastMessage::MemoUpdate(MemoUpdateMessage::new(
            "memo text".to_string(),
            render_markdown("memo text"),
            "docs/guide.md".to_string(),
        ))
        .to_json()
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "memo_update");
        assert_eq!(value["file"], "docs/guide.md");
        assert_eq!(value["raw"], "memo text");
        assert!(value["html"].is_string());
        assert!(value.get("refresh").is_none());
        assert!(value.get("error").is_none());
        assert!(value.get("content").is_none());
    }
}
