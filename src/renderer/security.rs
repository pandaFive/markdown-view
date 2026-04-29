#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum UrlPolicy {
    /// link は外部遷移を許可するため remote URL を許可する。
    Link,
    /// image はローカル文書表示の範囲に閉じ、remote URL は読み込まない。
    Image,
}

impl UrlPolicy {
    fn allows_remote(self) -> bool {
        matches!(self, Self::Link)
    }
}

pub(super) fn sanitize_link_href(dest_url: &str) -> String {
    sanitize_url(dest_url, UrlPolicy::Link)
}

pub(super) fn sanitize_image_src(dest_url: &str) -> String {
    sanitize_url(dest_url, UrlPolicy::Image)
}

fn sanitize_url(dest_url: &str, policy: UrlPolicy) -> String {
    let trimmed = dest_url.trim();
    if is_safe_href(trimmed, policy) {
        trimmed.to_string()
    } else {
        "#".to_string()
    }
}

fn is_safe_href(dest_url: &str, policy: UrlPolicy) -> bool {
    if dest_url.is_empty() {
        return false;
    }

    // protocol-relative URL は現在ページの scheme で外部へ出られるため拒否する。
    if dest_url.starts_with("//") {
        return false;
    }

    if dest_url.starts_with('#')
        || dest_url.starts_with('/')
        || dest_url.starts_with("./")
        || dest_url.starts_with("../")
        || dest_url.starts_with('?')
    {
        return true;
    }

    let Some(colon_pos) = dest_url.find(':') else {
        return true;
    };

    if !policy.allows_remote() {
        return false;
    }

    let scheme = dest_url[..colon_pos].to_ascii_lowercase();
    matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel")
}

/// HTML特殊文字のエスケープ（属性値にも安全）
pub fn html_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
