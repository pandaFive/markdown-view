use crate::server::MAX_FILE_SIZE;

mod css_bundle;
mod inline_script;

const DARK_THEME_VARS: &str = r##"
  --bg: #1a1b26;
  --bg-accent: radial-gradient(circle at top, rgba(122, 162, 247, 0.10), transparent 32%), radial-gradient(circle at 80% 20%, rgba(187, 154, 247, 0.08), transparent 24%), linear-gradient(180deg, #1e2030 0%, #16161e 100%);
  --fg: #c0caf5;
  --muted: #565f89;
  --sidebar-bg: rgba(22, 22, 30, 0.88);
  --sidebar-border: rgba(61, 66, 104, 0.35);
  --panel-bg: rgba(26, 27, 38, 0.78);
  --panel-border: rgba(61, 66, 104, 0.30);
  --panel-shadow: 0 24px 80px rgba(0, 0, 0, 0.40);
  --link: #7aa2f7;
  --code-bg: #16161e;
  --blockquote-border: rgba(122, 162, 247, 0.40);
  --blockquote-fg: #9aa5ce;
  --table-border: rgba(61, 66, 104, 0.40);
  --table-alt-bg: rgba(255, 255, 255, 0.02);
  --hr-color: rgba(61, 66, 104, 0.40);
  --toc-active: #bb9af7;
  --toc-hover-bg: rgba(122, 162, 247, 0.08);
  --pill-bg: rgba(61, 66, 104, 0.20);
  --pill-strong-bg: rgba(187, 154, 247, 0.16);
  --accent: #bb9af7;
  --accent-soft: rgba(187, 154, 247, 0.14);
  --blockquote-bg: rgba(255, 255, 255, 0.04);
  --sidebar-utility-bg: rgba(61, 66, 104, 0.15);
  --search-input-bg: rgba(22, 22, 30, 0.50);
  --topbar-btn-bg: rgba(61, 66, 104, 0.20);
  --content-bg: rgba(22, 22, 30, 0.80);
  --code-copy-bg: rgba(22, 22, 30, 0.50);
"##;

pub(crate) fn css() -> &'static str {
    css_bundle::css(DARK_THEME_VARS)
}

/// ベースCSSと構文ハイライトCSSを結合する
pub fn combined_css(syntax_css: &str) -> String {
    if syntax_css.is_empty() {
        css().to_string()
    } else {
        format!("{}\n{}", css(), syntax_css)
    }
}

/// インラインCSS/JS用のCSPハッシュソースを返す
///
/// `combined_css` と `JS` のハッシュは毎回計算する（キャッシュなし）。
/// 戻り値の順序: `(script-srcハッシュ, style-srcハッシュ)`
pub fn csp_hash_sources(syntax_css: &str) -> (String, String) {
    let style_hash = sha256_base64(combined_css(syntax_css).as_bytes());
    let script_hash = sha256_base64(inline_js().as_bytes());
    (
        format!("'sha256-{}'", script_hash),
        format!("'sha256-{}'", style_hash),
    )
}

fn sha256_base64(input: &[u8]) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;

    let digest = sha2::Sha256::digest(input);
    base64::engine::general_purpose::STANDARD.encode(digest)
}

pub(crate) fn inline_js() -> String {
    inline_script::inline_js(MAX_FILE_SIZE)
}
