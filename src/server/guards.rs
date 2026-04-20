//! Host/Origin 検証とHTTPエラー応答を管理する。

use std::net::IpAddr;

use axum::http::header::{HOST, ORIGIN};
use axum::http::uri::Authority;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::Json;

use super::messages::ApiError;
use crate::template::{csp_hash_sources, error_message_json};

pub(super) fn build_csp_header(syntax_css: &str) -> (HeaderValue, bool) {
    let (script_src, style_src) = csp_hash_sources(syntax_css);
    let csp = format!(
        "default-src 'self'; script-src {}; style-src {}; img-src 'self'; connect-src 'self' ws: wss:; object-src 'none'; frame-ancestors 'none'",
        script_src, style_src
    );
    match HeaderValue::from_str(&csp) {
        Ok(header) => (header, false),
        Err(e) => {
            tracing::error!(
                "[markdown-view] CSPヘッダーの生成に失敗（フォールバックCSPを使用）: {} (CSP: {})",
                e,
                csp
            );
            tracing::warn!(
                "[markdown-view] セキュリティ警告: フォールバックCSPのためscript/styleのsha256制約が無効です"
            );
            (
                HeaderValue::from_static(
                    "default-src 'self'; object-src 'none'; frame-ancestors 'none'",
                ),
                true,
            )
        }
    }
}

pub(super) fn json_error(status: StatusCode, message: impl AsRef<str>) -> ApiError {
    (status, Json(error_message_json(message)))
}

/// 許可されたHostヘッダーのみ受け付け、拒否時は監査向けwarnログを残す。
pub(super) fn ensure_allowed_request_host(headers: &HeaderMap) -> Result<(), ApiError> {
    if is_allowed_request_host(headers) {
        Ok(())
    } else {
        let host = headers
            .get(HOST)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("<missing-or-invalid>");
        tracing::warn!(
            "[markdown-view] 許可されていないHostヘッダーを拒否: {}",
            host
        );
        Err(json_error(
            StatusCode::FORBIDDEN,
            "許可されていないHostヘッダーです",
        ))
    }
}

pub(super) fn is_allowed_request_host(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    is_trusted_authority(host)
}

/// WebSocket接続時のOriginヘッダーを検証する
///
/// DNS Rebinding対策として、Host検証に加えてOriginのauthority一致も要求する。
/// Originスキームは`http`/`https`のみ許可する。
pub(super) fn is_allowed_ws_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(host) = headers.get(HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    if !is_trusted_authority(host) {
        return false;
    }
    let Ok(origin_uri) = origin.parse::<Uri>() else {
        return false;
    };

    match origin_uri.scheme_str() {
        Some("http") | Some("https") => {}
        _ => return false,
    }

    let Some(origin_authority) = origin_uri.authority() else {
        return false;
    };
    if !is_trusted_authority(origin_authority.as_str()) {
        return false;
    }

    normalize_authority(origin_authority.as_str()) == normalize_authority(host)
}

pub(super) fn is_trusted_authority(authority: &str) -> bool {
    let Ok(authority) = authority.parse::<Authority>() else {
        return false;
    };
    is_trusted_host(authority.host())
}

pub(super) fn is_trusted_host(host: &str) -> bool {
    let normalized = host
        .trim()
        .trim_end_matches('.')
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();

    if normalized == "localhost" {
        return true;
    }

    match normalized.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// authority文字列（`host[:port]`）を比較用に正規化する
///
/// 末尾ドットを除去し、大小文字差を吸収する。
pub(super) fn normalize_authority(authority: &str) -> String {
    if let Ok(parsed) = authority.parse::<Authority>() {
        let host = parsed.host().trim_end_matches('.').to_ascii_lowercase();
        if let Some(port) = parsed.port_u16() {
            format!("{}:{}", host, port)
        } else {
            host
        }
    } else {
        tracing::warn!(
            "[markdown-view] authority解析に失敗（簡易正規化にフォールバック）: {:?}",
            authority
        );
        authority.trim().trim_end_matches('.').to_ascii_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use axum::http::header::{HOST, ORIGIN};
    use axum::http::HeaderMap;

    use super::*;

    #[test]
    fn test_trusted_host_localhost() {
        assert!(is_trusted_host("localhost"));
        assert!(is_trusted_host("LOCALHOST"));
        assert!(is_trusted_host("localhost."));
    }

    #[test]
    fn test_trusted_host_loopback_ipv4() {
        assert!(is_trusted_host("127.0.0.1"));
        assert!(is_trusted_host("127.0.0.2"));
        assert!(!is_trusted_host("0.0.0.0"));
    }

    #[test]
    fn test_trusted_host_loopback_ipv6() {
        // bracketed（HTTP authority の正規形式）
        assert!(is_trusted_host("[::1]"));
        // 非 bracketed（is_trusted_host の防御的実装が自前で bracket を trim するケース）
        assert!(is_trusted_host("::1"));
    }

    #[test]
    fn test_trusted_host_rejects_external() {
        assert!(!is_trusted_host("evil.example"));
        assert!(!is_trusted_host("example.com"));
        assert!(!is_trusted_host("192.168.1.1"));
    }

    #[test]
    fn test_allowed_request_host_missing_header() {
        let headers = HeaderMap::new();
        assert!(!is_allowed_request_host(&headers));
    }

    #[test]
    fn test_allowed_request_host_valid() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        assert!(is_allowed_request_host(&headers));
    }

    #[test]
    fn test_allowed_request_host_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "evil.example:3000".parse().unwrap());
        assert!(!is_allowed_request_host(&headers));
    }

    #[test]
    fn test_allowed_ws_origin_valid() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert!(is_allowed_ws_origin(&headers));
    }

    #[test]
    fn test_allowed_ws_origin_rejects_different_port() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:4000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
    }

    #[test]
    fn test_allowed_ws_origin_rejects_ftp_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "ftp://localhost:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
    }

    #[test]
    fn test_allowed_ws_origin_rejects_different_host() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://evil.example:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
    }

    #[test]
    fn test_allowed_ws_origin_missing_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
    }

    #[test]
    fn test_allowed_ws_origin_missing_host() {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
    }

    #[test]
    fn test_normalize_authority_末尾ドットと大文字小文字を正規化する() {
        assert_eq!(
            normalize_authority("LOCALHOST.:3000"),
            normalize_authority("localhost:3000")
        );
        assert_eq!(
            normalize_authority("Example.COM."),
            normalize_authority("example.com")
        );
    }

    #[test]
    fn test_allowed_ws_origin_trailing_dotとmixed_caseを許可する() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "LOCALHOST.:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert!(is_allowed_ws_origin(&headers));
    }
}
