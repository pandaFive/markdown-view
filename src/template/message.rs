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
    /// メモ応答を生成する。HTML は内部で raw から描画され、整合性が保証される。
    pub fn from_raw(raw: String, file: Option<String>) -> Self {
        let html = render_markdown(&raw);
        Self { raw, html, file }
    }

    /// 空メモ応答を生成する
    pub fn empty(file: Option<String>) -> Self {
        Self::from_raw(String::new(), file)
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

/// WebSocket向けメモ更新JSONメッセージ構造体
#[derive(serde::Serialize, Debug, Clone)]
pub struct MemoUpdateMessage {
    #[serde(rename = "type")]
    message_type: &'static str,
    file: String,
}

impl MemoUpdateMessage {
    /// メモ更新メッセージを生成する
    pub fn new(file: String) -> Self {
        Self {
            message_type: "memo_update",
            file,
        }
    }

    /// 対象ファイル相対パスを返す
    pub fn file(&self) -> &str {
        &self.file
    }
}

/// エラーJSONを生成する
pub fn error_message_json(message: impl AsRef<str>) -> serde_json::Value {
    serde_json::json!({ "error": message.as_ref() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_raw_htmlがrender_markdownと一致する() {
        let inputs = ["", "# 見出し", "段落\n\n- リスト1\n- リスト2", "> 引用"];
        for input in inputs {
            let memo = MemoResponse::from_raw(input.to_string(), None);
            assert_eq!(
                memo.html(),
                &render_markdown(input),
                "from_raw({:?}).html() must equal render_markdown({:?})",
                input,
                input
            );
        }
    }

    #[test]
    fn test_from_raw_rawフィールドは入力を改変せず保持する() {
        let input = "  raw\nメモ\n\n複数行  ";
        let memo = MemoResponse::from_raw(input.to_string(), None);
        assert_eq!(memo.raw(), input);
    }

    #[test]
    fn test_from_raw_fileフィールドが保持される() {
        let memo_some = MemoResponse::from_raw(String::new(), Some("docs/api.md".to_string()));
        assert_eq!(memo_some.file(), Some("docs/api.md"));

        let memo_none = MemoResponse::from_raw(String::new(), None);
        assert_eq!(memo_none.file(), None);
    }

    #[test]
    fn test_empty_は空文字列をfrom_rawしたものと等価() {
        let file = Some("note.md".to_string());
        let by_empty = MemoResponse::empty(file.clone());
        let by_from_raw = MemoResponse::from_raw(String::new(), file.clone());

        assert_eq!(by_empty.raw(), by_from_raw.raw());
        assert_eq!(by_empty.html(), by_from_raw.html());
        assert_eq!(by_empty.file(), by_from_raw.file());
        assert_eq!(by_empty.raw(), "");
    }
}
