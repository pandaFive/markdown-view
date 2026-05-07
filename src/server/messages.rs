//! WebSocket/HTTP応答で共有するサーバーメッセージ型とAPIエラー型を管理する。

use axum::http::StatusCode;
use axum::response::Json;

use crate::template::{error_message_json, MemoUpdateMessage, UpdateMessage};

#[derive(serde::Serialize, Debug, Clone)]
pub struct LaggedRecoveryMessage {
    #[serde(flatten)]
    update: UpdateMessage,
    memo_refresh: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    memo_file: Option<String>,
}

impl LaggedRecoveryMessage {
    pub fn new(update: UpdateMessage, memo_file: Option<String>) -> Self {
        Self {
            update,
            memo_refresh: true,
            memo_file,
        }
    }
}

/// HTTP APIエラー応答の共通型
pub(super) type ApiError = (StatusCode, Json<serde_json::Value>);

/// WebSocket broadcastメッセージ
#[derive(Debug, Clone)]
pub enum BroadcastMessage {
    /// コンテンツ更新
    Update(UpdateMessage),
    /// メモ更新
    MemoUpdate(MemoUpdateMessage),
    /// 遅延回復時に本文更新とメモ再取得を同時に通知
    LaggedRecovery(LaggedRecoveryMessage),
    /// クライアントに再取得を促す
    Refresh,
    /// エラー通知
    Error(String),
}

pub(super) struct BroadcastMessageLogSummary {
    pub message_kind: &'static str,
    pub has_file: bool,
}

impl BroadcastMessage {
    pub(super) fn to_json(&self) -> Result<String, serde_json::Error> {
        match self {
            BroadcastMessage::Update(update) => serde_json::to_string(update),
            BroadcastMessage::MemoUpdate(update) => serde_json::to_string(update),
            BroadcastMessage::LaggedRecovery(message) => serde_json::to_string(message),
            BroadcastMessage::Refresh => serde_json::to_string(&serde_json::json!({
                "refresh": true
            })),
            BroadcastMessage::Error(message) => serde_json::to_string(&error_message_json(message)),
        }
    }

    pub(super) fn log_summary(&self) -> BroadcastMessageLogSummary {
        match self {
            BroadcastMessage::Update(update) => BroadcastMessageLogSummary {
                message_kind: "update",
                has_file: update.file().is_some(),
            },
            BroadcastMessage::MemoUpdate(_) => BroadcastMessageLogSummary {
                message_kind: "memo_update",
                has_file: true,
            },
            BroadcastMessage::LaggedRecovery(message) => BroadcastMessageLogSummary {
                message_kind: "lagged_recovery",
                has_file: message.memo_file.is_some(),
            },
            BroadcastMessage::Refresh => BroadcastMessageLogSummary {
                message_kind: "refresh",
                has_file: false,
            },
            BroadcastMessage::Error(_) => BroadcastMessageLogSummary {
                message_kind: "error",
                has_file: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let json =
            BroadcastMessage::MemoUpdate(MemoUpdateMessage::new("docs/guide.md".to_string()))
                .to_json()
                .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "memo_update");
        assert_eq!(value["file"], "docs/guide.md");
        assert!(value.get("raw").is_none());
        assert!(value.get("html").is_none());
        assert!(value.get("refresh").is_none());
        assert!(value.get("error").is_none());
        assert!(value.get("content").is_none());
    }

    #[test]
    fn test_broadcast_message_lagged_recoveryのjson直列化() {
        let json = BroadcastMessage::LaggedRecovery(LaggedRecoveryMessage::new(
            UpdateMessage::new(
                crate::renderer::render_markdown("# title"),
                crate::toc::generate_toc("# title"),
                None,
            ),
            None,
        ))
        .to_json()
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(value["content"].as_str().unwrap().contains("title"));
        assert!(value["toc"].as_str().unwrap().contains("title"));
        assert_eq!(value["memo_refresh"], true);
        assert!(value.get("memo_file").is_none());
    }

    #[test]
    fn test_broadcast_message_log_summaryは全variantの種別とfile有無を返す() {
        let update_with_file = BroadcastMessage::Update(UpdateMessage::new(
            crate::renderer::render_markdown("# title"),
            crate::toc::generate_toc("# title"),
            Some("docs/guide.md".to_string()),
        ));
        assert_log_summary(update_with_file, "update", true);

        let update_without_file = BroadcastMessage::Update(UpdateMessage::new(
            crate::renderer::render_markdown("# title"),
            crate::toc::generate_toc("# title"),
            None,
        ));
        assert_log_summary(update_without_file, "update", false);

        assert_log_summary(
            BroadcastMessage::MemoUpdate(MemoUpdateMessage::new("docs/guide.md".to_string())),
            "memo_update",
            true,
        );

        let recovery_with_memo_file = BroadcastMessage::LaggedRecovery(LaggedRecoveryMessage::new(
            UpdateMessage::new(
                crate::renderer::render_markdown("# title"),
                crate::toc::generate_toc("# title"),
                None,
            ),
            Some("docs/guide.md".to_string()),
        ));
        assert_log_summary(recovery_with_memo_file, "lagged_recovery", true);

        let recovery_without_memo_file =
            BroadcastMessage::LaggedRecovery(LaggedRecoveryMessage::new(
                UpdateMessage::new(
                    crate::renderer::render_markdown("# title"),
                    crate::toc::generate_toc("# title"),
                    None,
                ),
                None,
            ));
        assert_log_summary(recovery_without_memo_file, "lagged_recovery", false);

        assert_log_summary(BroadcastMessage::Refresh, "refresh", false);
        assert_log_summary(
            BroadcastMessage::Error("secret error detail".to_string()),
            "error",
            false,
        );
    }

    fn assert_log_summary(
        message: BroadcastMessage,
        expected_message_kind: &'static str,
        expected_has_file: bool,
    ) {
        let summary = message.log_summary();

        assert_eq!(summary.message_kind, expected_message_kind);
        assert_eq!(summary.has_file, expected_has_file);
    }
}
