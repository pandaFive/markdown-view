//! Markdown→HTML変換とTOC生成を担当するモジュール。
//!
//! このモジュールとそのサブモジュールは [`SanitizedHtml`] の構築権を持つ
//! 信頼境界を構成する。サブモジュールの追加はセキュリティ影響を伴う。

mod highlight;
mod line;
mod render;
mod security;
mod state;

use std::sync::OnceLock;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use syntect::highlighting::ThemeSet;
use syntect::html::{css_for_theme_with_class_style, ClassStyle};
use syntect::parsing::SyntaxSet;

pub mod toc;

pub use security::html_escape;

/// サニタイズ済みHTMLを表すnewtype
///
/// `render_markdown` / `generate_toc` が主たる生成経路。
/// コンストラクタは `pub(in crate::renderer)` とし、
/// `renderer`モジュールツリー内でのみ構築可能にする。
/// 生文字列の混入を型で防止する。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct SanitizedHtml(String);

impl SanitizedHtml {
    /// サニタイズ済みHTML文字列として参照する
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// サニタイズ済みHTMLから構築する（rendererモジュール内部専用）
    ///
    /// 呼び出し側がHTMLのサニタイズを保証する必要がある。
    /// 外部からの生文字列に対して使用してはならない。
    pub(in crate::renderer) fn from_sanitized_html(html: String) -> Self {
        Self(html)
    }
}

/// Markdownテキストを HTML に変換する
///
/// - GFM拡張（テーブル、タスクリスト、取消線）対応
/// - コードブロックはsyntectでクラスベースハイライト
/// - 見出しにはスラッグIDを付与
/// - raw HTMLは完全に除去される（XSS防止のため出力に含めない）
pub fn render_markdown(input: &str) -> SanitizedHtml {
    if input.is_empty() {
        return SanitizedHtml::from_sanitized_html(String::new());
    }

    render::Renderer::new(input, render::RenderOptions::default()).render()
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

/// テーマ名が有効か検証する（起動時のfail-fast用）
///
/// 無効な場合は利用可能なテーマ名の一覧を返す。
pub fn validate_theme(name: &str) -> Result<(), Vec<String>> {
    let ts = theme_set();
    if ts.themes.contains_key(name) {
        Ok(())
    } else {
        Err(ts.themes.keys().cloned().collect())
    }
}

/// syntectテーマからクラスベースのCSSを生成する
///
/// テーマが見つからない場合やCSS生成に失敗した場合は空文字列を返す。
/// 空文字列の場合、構文ハイライトは無効化される。
pub fn syntax_theme_css(theme_name: Option<&str>) -> String {
    let ts = theme_set();
    let Some(theme) = resolve_theme(ts, theme_name) else {
        tracing::warn!("[markdown-view] テーマが見つからないため構文ハイライトCSSを生成できません");
        return highlight_disabled_notice_css();
    };

    match css_for_theme_with_class_style(theme, ClassStyle::SpacedPrefixed { prefix: "syn-" }) {
        Ok(css) => css,
        Err(e) => {
            tracing::warn!(
                "[markdown-view] 構文ハイライトCSS生成に失敗したため無効化します: {}",
                e
            );
            highlight_disabled_notice_css()
        }
    }
}

fn highlight_disabled_notice_css() -> String {
    r#"
/* 構文ハイライト無効化時のユーザー通知 */
body::before {
  content: '構文ハイライトを無効化しました（テーマ読み込み失敗）';
  position: fixed;
  top: 0;
  right: 0;
  z-index: 1100;
  background: #fff4ce;
  color: #5c4500;
  border: 1px solid #d9b84f;
  border-radius: 0 0 0 6px;
  padding: 0.35rem 0.55rem;
  font-size: 0.75rem;
  font-family: sans-serif;
}
"#
    .to_string()
}

fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options
}

/// Markdownから抽出した見出し情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingInfo {
    pub level: u8,
    pub text: String,
    pub id: String,
}

/// Markdownから見出し情報を抽出する
///
/// 画像altは見出しテキストから除外し、`render_markdown`と同じID生成ルールを適用する。
pub fn extract_headings(input: &str) -> Vec<HeadingInfo> {
    let parser = Parser::new_ext(input, markdown_options());
    let mut headings = Vec::new();
    let mut current_level: Option<u8> = None;
    let mut current_text = String::new();
    let mut in_heading_image = false;
    let mut id_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current_level = Some(level as u8);
                current_text.clear();
                in_heading_image = false;
            }
            Event::Start(Tag::Image { .. }) if current_level.is_some() => {
                in_heading_image = true;
            }
            Event::End(TagEnd::Image) if current_level.is_some() => {
                in_heading_image = false;
            }
            Event::Text(text) if current_level.is_some() => {
                if !in_heading_image {
                    current_text.push_str(&text);
                }
            }
            Event::Code(text) if current_level.is_some() => {
                if !in_heading_image {
                    current_text.push_str(&text);
                }
            }
            Event::SoftBreak if current_level.is_some() => {
                if !in_heading_image {
                    current_text.push(' ');
                }
            }
            Event::HardBreak if current_level.is_some() => {
                if !in_heading_image {
                    current_text.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = current_level {
                    let slug = slugify(&current_text);
                    let id = generate_unique_id(&slug, &mut id_counts);
                    headings.push(HeadingInfo {
                        level,
                        text: current_text.clone(),
                        id,
                    });
                }
                current_level = None;
                in_heading_image = false;
            }
            other => {
                tracing::debug!(
                    "[markdown-view] 見出し抽出で未処理イベントを無視: {:?}",
                    other
                );
            }
        }
    }

    headings
}

/// 見出しテキストをスラッグ（URL-safe ID）に変換する
pub fn slugify(text: &str) -> String {
    let slug = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        "section".to_string()
    } else {
        slug
    }
}

/// スラッグから一意なIDを生成する（重複時は連番を付与）
pub fn generate_unique_id(
    slug: &str,
    id_counts: &mut std::collections::HashMap<String, usize>,
) -> String {
    let count = id_counts.entry(slug.to_string()).or_insert(0);
    let id = if *count == 0 {
        slug.to_string()
    } else {
        format!("{}-{}", slug, count)
    };
    *count += 1;
    id
}

fn resolve_theme<'a>(
    theme_set: &'a ThemeSet,
    theme_name: Option<&str>,
) -> Option<&'a syntect::highlighting::Theme> {
    const DEFAULT_THEME: &str = "base16-ocean.dark";

    if let Some(name) = theme_name {
        if let Some(theme) = theme_set.themes.get(name) {
            return Some(theme);
        }
        let available: Vec<&str> = theme_set.themes.keys().map(|s| s.as_str()).collect();
        tracing::warn!(
            "[markdown-view] 警告: テーマ '{}' が見つかりません。デフォルトテーマを使用します。利用可能: {:?}",
            name,
            available
        );
    }

    if let Some(theme) = theme_set.themes.get(DEFAULT_THEME) {
        return Some(theme);
    }

    let fallback = theme_set.themes.iter().next();
    if let Some((name, _)) = &fallback {
        tracing::warn!(
            "[markdown-view] 警告: デフォルトテーマ '{}' が見つかりません。'{}' を使用します",
            DEFAULT_THEME,
            name
        );
    }
    fallback.map(|(_, theme)| theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::UpdateMessage;

    #[test]
    fn test_sanitized_html_serde_transparentで文字列として直列化される() {
        let html = SanitizedHtml::from_sanitized_html("<p>x</p>".to_string());
        let value = serde_json::to_value(&html).unwrap();
        assert_eq!(value, serde_json::Value::String("<p>x</p>".to_string()));
    }

    #[test]
    fn test_sanitized_htmlがupdate_message内でも文字列として直列化される() {
        let update = UpdateMessage::new(
            SanitizedHtml::from_sanitized_html("<p>content</p>".to_string()),
            SanitizedHtml::from_sanitized_html("<ul><li>toc</li></ul>".to_string()),
            None,
        );
        let value = serde_json::to_value(update).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "content": "<p>content</p>",
                "toc": "<ul><li>toc</li></ul>"
            })
        );
    }
}
