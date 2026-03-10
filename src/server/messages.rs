//! WebSocket/HTTP応答で共有するサーバーメッセージを管理する。

use crate::template::{error_message_json, UpdateMessage};

/// ファイルサイズ上限: OOM防止
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
const FILE_SIZE_LIMIT_MB: u64 = MAX_FILE_SIZE / 1024 / 1024;

/// ファイルサイズ超過時のユーザー向けエラーメッセージを返す。
pub(super) fn file_size_limit_error_message() -> String {
    format!(
        "ファイルサイズが上限（{}MB）を超えています",
        FILE_SIZE_LIMIT_MB
    )
}

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
