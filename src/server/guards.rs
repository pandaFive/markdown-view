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
    let Ok(parsed) = authority.parse::<Authority>() else {
        return false;
    };
    // userinfo 付き authority (user@host 形式) は拒否する。
    // 現実の Host / Origin ヘッダーには userinfo は含まれず、
    // 攻撃者が任意 host 文字列を埋め込むバイパス経路になりうるため。
    // （例: "user@[::1]:3000" は http クレートのパーサを通過するが、
    //   host() が "[::1]" を返すため loopback 認定されてしまう）
    if parsed.as_str().contains('@') {
        return false;
    }
    // http クレート (1.x) の Authority パーサは非数値port（例: "[::1]:abc"）も受け入れ、
    // この場合 port() / port_u16() はいずれも None を返す（=無port扱い）。
    // DNS Rebinding境界として信頼するには数値portを必須とするため、
    // 元文字列を直接検査してport接尾辞の有無を判定する。
    if has_port_suffix(parsed.as_str()) && parsed.port_u16().is_none() {
        return false;
    }
    is_trusted_host(parsed.host())
}

/// authority 文字列に `:port` 接尾辞が存在するかを判定する。
///
/// is_trusted_authority が http クレートの非数値port受理を補正するために使用する
/// **DNS Rebinding対策の一部**。「Authority::port_u16() が None」だけでは
/// 「port未指定」と「非数値port」を区別できないため、元文字列を直接検査する。
///
/// IPv6 (bracketed) の場合は `]` の直後に `:` が続くかで判断し、
/// 内部のコロン（`::`）を port 区切りと誤認しないようにする。
fn has_port_suffix(authority: &str) -> bool {
    match authority.find(']') {
        Some(i) => authority[i + 1..].starts_with(':'),
        None => authority.contains(':'),
    }
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
    fn test_trusted_host_ipv6_非loopbackを拒否する() {
        // link-local：loopback ではない
        assert!(!is_trusted_host("[fe80::1]"));
        // unspecified（::）：0.0.0.0 相当。loopback と紛らわしいため明示
        assert!(!is_trusted_host("[::]"));
        // public IPv6（RFC 3849 ドキュメント用アドレス）
        assert!(!is_trusted_host("[2001:db8::1]"));
        // IPv4-mapped IPv6：Ipv6Addr::is_loopback は ::1 のみ true を返す仕様
        // （IPv4-mapped を loopback 扱いする将来の書き換えを防ぐ固定テスト）
        assert!(!is_trusted_host("[::ffff:127.0.0.1]"));
    }

    #[test]
    fn test_trusted_authority_ipv6_port付きを検証する() {
        // 正常系：port 付き IPv6 loopback authority
        assert!(is_trusted_authority("[::1]:3000"));

        // 非数値 port：is_trusted_authority 内の has_port_suffix チェックで明示拒否
        // （httpクレートのAuthorityパーサ自体は非数値portを受け入れてしまうため、
        //  このガードが無いとloopback認定されて通過してしまう）
        assert!(!is_trusted_authority("[::1]:abc"));

        // 非 loopback IPv6 + port：is_trusted_host 側で拒否
        assert!(!is_trusted_authority("[fe80::1]:3000"));

        // 空 port 接尾辞：":"はあるが port_u16 が None → has_port_suffix チェックで拒否
        assert!(!is_trusted_authority("[::1]:"));

        // u16 範囲外の port：u16 overflow → port_u16 が None → 拒否
        assert!(!is_trusted_authority("[::1]:65536"));
        assert!(!is_trusted_authority("[::1]:99999"));

        // port 0：RFC 上は予約だが port_u16 が Some(0) のため現状の実装では許可される。
        // 実害のない挙動を固定化することで、将来「0 を予約として拒否」する選択を
        // 意識的に行えるようにする
        assert!(is_trusted_authority("[::1]:0"));

        // IPv6 zone ID：http クレートは解析を許すが、is_trusted_host の IpAddr::parse
        // が zone suffix 付き文字列を受け付けないため最終的に拒否される
        assert!(!is_trusted_authority("[fe80::1%25eth0]"));
        assert!(!is_trusted_authority("[fe80::1%25eth0]:3000"));
    }

    #[test]
    fn test_has_port_suffix_直接検証() {
        // IPv6 bracketed
        assert!(!has_port_suffix("[::1]"));
        assert!(has_port_suffix("[::1]:3000"));
        assert!(has_port_suffix("[::1]:abc"));
        assert!(has_port_suffix("[::1]:"));
        // 非 bracketed（hostname / IPv4）
        assert!(!has_port_suffix("localhost"));
        assert!(has_port_suffix("localhost:3000"));
        assert!(has_port_suffix("127.0.0.1:3000"));
        // 非 bracketed IPv6 ("::1") は内部コロンを port 区切りと誤認する既知仕様。
        // Authority 正規形式では IPv6 は bracketed が必須のため、この経路の authority は
        // そもそも parse<Authority>() 前段でほぼ到達しない。現状挙動を固定。
        assert!(has_port_suffix("::1"));
        // 空文字
        assert!(!has_port_suffix(""));
    }

    #[test]
    fn test_trusted_authority_userinfo_を拒否する() {
        // userinfo 経由のバイパス防御。http クレートの Authority パーサは
        // user@host 形式を受理し、host() は userinfo を除いた host を返すため、
        // 明示的に弾かないと攻撃者が任意の userinfo を埋め込んで loopback 認定させうる
        assert!(!is_trusted_authority("user@localhost:3000"));
        assert!(!is_trusted_authority("user@[::1]:3000"));
        assert!(!is_trusted_authority("user:pass@localhost:3000"));
        assert!(!is_trusted_authority("user:pass@[::1]:3000"));
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
    fn test_normalize_authority_ipv6_等価性() {
        // is_allowed_ws_origin 内部で実行される比較を直接再現：
        // HOST ヘッダー文字列と Origin URI から取得した authority 文字列が
        // 同じ正規化結果になることを保証する
        let host_normalized = normalize_authority("[::1]:3000");
        let origin_uri: Uri = "http://[::1]:3000".parse().expect("有効な URI");
        let origin_authority = origin_uri
            .authority()
            .expect("authority が存在する")
            .as_str();
        assert_eq!(host_normalized, normalize_authority(origin_authority));

        // 非空かつ IPv6 情報と port が含まれていることを確認する。
        // 具体的な文字列形式（brackets 有無など）は http クレートのバージョン差で
        // 変化しうるため、内容ベースで検証する。
        // 参考：http 1.x の Authority::host() は IPv6 の brackets を保持して返すが、
        // 将来それが変わっても「::1 と 3000 を含むこと」という意味的要件は不変。
        assert!(!host_normalized.is_empty());
        assert!(host_normalized.contains("::1"));
        assert!(host_normalized.contains("3000"));
    }

    #[test]
    fn test_allowed_ws_origin_trailing_dotとmixed_caseを許可する() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "LOCALHOST.:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert!(is_allowed_ws_origin(&headers));
    }

    #[test]
    fn test_allowed_ws_origin_ipv6_loopback許可と境界() {
        // 成功ケース：HOST と Origin が同一 IPv6 loopback authority
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "[::1]:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://[::1]:3000".parse().unwrap());
        assert!(is_allowed_ws_origin(&headers));

        // 失敗ケース：port 不一致
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "[::1]:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://[::1]:4000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));

        // 失敗ケース：非 loopback IPv6（link-local）は
        // HOST/Origin が一致していても拒否される
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "[fe80::1]:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://[fe80::1]:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
    }
}
