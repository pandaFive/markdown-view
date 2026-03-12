use crate::renderer::{render_markdown, SanitizedHtml};

/// コンテンツ更新用JSONメッセージ構造体（HTTP API・WebSocket共用）
#[derive(serde::Serialize, Debug, Clone)]
pub struct UpdateMessage {
    content: SanitizedHtml,
    toc: SanitizedHtml,
    /// ディレクトリモード時の変更ファイル相対パス（単一ファイルモードはNone）
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
}

impl UpdateMessage {
    /// 更新メッセージを生成する
    pub fn new(content: SanitizedHtml, toc: SanitizedHtml, file: Option<String>) -> Self {
        Self { content, toc, file }
    }

    /// コンテンツHTMLを返す
    pub fn content(&self) -> &SanitizedHtml {
        &self.content
    }

    /// TOC HTMLを返す
    pub fn toc(&self) -> &SanitizedHtml {
        &self.toc
    }

    /// ディレクトリモード時の変更ファイル相対パスを返す
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// fileフィールドを置き換えた新しいメッセージを返す
    pub fn with_file(mut self, file: Option<String>) -> Self {
        self.file = file;
        self
    }
}

/// メモ取得・保存応答用JSONメッセージ構造体
#[derive(serde::Serialize, Debug, Clone)]
pub struct MemoResponse {
    raw: String,
    html: SanitizedHtml,
    /// ディレクトリモード時の対象ファイル相対パス（単一ファイルモードはNone）
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
}

impl MemoResponse {
    /// メモ応答を生成する
    pub fn new(raw: String, html: SanitizedHtml, file: Option<String>) -> Self {
        Self { raw, html, file }
    }

    /// 空メモ応答を生成する
    pub fn empty(file: Option<String>) -> Self {
        Self::new(String::new(), render_markdown(""), file)
    }

    /// 生のメモ文字列を返す
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// プレビューHTMLを返す
    pub fn html(&self) -> &SanitizedHtml {
        &self.html
    }

    /// ディレクトリモード時の対象ファイル相対パスを返す
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }
}

/// エラーJSONを生成する
pub fn error_message_json(message: impl AsRef<str>) -> serde_json::Value {
    serde_json::json!({ "error": message.as_ref() })
}
