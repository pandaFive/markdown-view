//! Host/Origin 検証とHTTPエラー応答を管理する。

use std::net::IpAddr;

use axum::extract::Request;
use axum::http::header::{HOST, ORIGIN};
use axum::http::uri::Authority;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::messages::ApiError;
use crate::template::{csp_hash_sources, error_message_json};

pub(super) fn build_csp_header(syntax_css: &str) -> HeaderValue {
    let (script_src, style_src) = csp_hash_sources(syntax_css);
    let csp = format!(
        "default-src 'self'; script-src {}; style-src {}; img-src 'self'; connect-src 'self' ws: wss:; object-src 'none'; frame-ancestors 'none'",
        script_src, style_src
    );
    // csp_hash_sources は base64 sha256 のみを返す契約のため、
    // visible-ASCII 違反による HeaderValue::from_str 失敗は構造上到達不能。
    // 到達した場合は契約破り（バグ）であり、permissive な fallback CSP で
    // silent に degradation するより startup panic で表面化させる。
    HeaderValue::from_str(&csp).unwrap_or_else(|e| {
        panic!(
            "CSP ヘッダー生成に失敗（csp_hash_sources の出力契約破り）: {} (CSP: {})",
            e, csp
        )
    })
}

pub(super) fn json_error(status: StatusCode, message: impl AsRef<str>) -> ApiError {
    (status, Json(error_message_json(message)))
}

/// ヘッダー値を audit log 用に安全に取り出す。
///
/// - ヘッダー不在 → `"<absent>"`
/// - ヘッダー存在するが `to_str()` 失敗（非 ASCII バイト含む） → `"<non-ascii>"`
///
/// 2 つの失敗モードを sentinel で区別することで、正常な欠落と攻撃者制御の
/// malformed header probe を audit log 上で分離する。security triage の
/// 観点で重要。
fn log_value_for_header<'a>(headers: &'a HeaderMap, name: &axum::http::HeaderName) -> &'a str {
    match headers.get(name) {
        None => "<absent>",
        Some(v) => v.to_str().unwrap_or("<non-ascii>"),
    }
}

fn log_value_for_ws_origin(headers: &HeaderMap) -> String {
    let Some(value) = headers.get(ORIGIN) else {
        return "<absent>".to_string();
    };
    let Ok(origin) = value.to_str() else {
        return "<non-ascii>".to_string();
    };
    let Ok(uri) = origin.parse::<Uri>() else {
        return "<invalid-origin-uri>".to_string();
    };
    let (Some(scheme), Some(authority)) = (uri.scheme_str(), uri.authority()) else {
        return "<invalid-origin-uri>".to_string();
    };
    if authority.as_str().contains('@') {
        return "<origin-authority-with-userinfo>".to_string();
    }
    format!("{scheme}://{authority}")
}

fn log_value_for_ws_host(headers: &HeaderMap) -> String {
    let Some(value) = headers.get(HOST) else {
        return "<absent>".to_string();
    };
    let Ok(host) = value.to_str() else {
        return "<non-ascii>".to_string();
    };
    let Ok(authority) = host.parse::<Authority>() else {
        return "<invalid-host-authority>".to_string();
    };
    if authority.as_str().contains('@') {
        return "<host-authority-with-userinfo>".to_string();
    }
    if has_port_suffix(authority.as_str()) && authority.port_u16().is_none() {
        return "<invalid-host-authority>".to_string();
    }
    authority.as_str().to_string()
}

/// 許可されたHostヘッダーのみ受け付け、拒否時は監査向けwarnログを残す。
#[cfg(test)]
pub(super) fn ensure_allowed_request_host(headers: &HeaderMap) -> Result<(), ApiError> {
    ensure_allowed_request_host_with_path(headers, None)
}

fn ensure_allowed_request_host_with_path(
    headers: &HeaderMap,
    request_path: Option<&str>,
) -> Result<(), ApiError> {
    if is_allowed_request_host(headers) {
        Ok(())
    } else {
        let host = log_value_for_header(headers, &HOST);
        match request_path {
            Some(path) => {
                tracing::warn!(
                    host = ?host,
                    request_path = path,
                    "[markdown-view] 許可されていないHostヘッダーを拒否: host={:?} path={:?}",
                    host,
                    path
                );
            }
            None => {
                tracing::warn!(
                    "[markdown-view] 許可されていないHostヘッダーを拒否: {:?}",
                    host
                );
            }
        }
        Err(json_error(
            StatusCode::FORBIDDEN,
            "許可されていないHostヘッダーです",
        ))
    }
}

/// Host 検証を通過したリクエストだけを後続 route へ渡す axum middleware。
///
/// 拒否時の warn 監査ログと `403` JSON 応答は
/// path 付きの Host guard helper に委譲する。許可時だけ `next.run` を呼び、
/// handler 側で Host 検証を重複実装しないための共通境界として使う。
///
/// 適用範囲は呼び出し側の `Router::layer` 配置で決まるため、route 追加時は
/// `create_router` 側の Host middleware 配下に入る構造を維持すること。
pub(super) async fn require_allowed_request_host(request: Request, next: Next) -> Response {
    if let Err(error) =
        ensure_allowed_request_host_with_path(request.headers(), Some(request.uri().path()))
    {
        return error.into_response();
    }

    next.run(request).await
}

pub(super) fn is_allowed_request_host(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    is_trusted_authority(host, "host")
}

/// WebSocket Origin 検証の拒否理由
///
/// `is_allowed_ws_origin` の silent return を観測可能にするため、
/// 各拒否分岐を variant として表現する。
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub(super) enum WsOriginRejection {
    MissingOrigin,
    MissingHost,
    /// Origin ヘッダーは存在するが `to_str()` に失敗（非 ASCII バイト含む）
    ///
    /// 通常のブラウザは ASCII のみで構成された Origin を送る。非 ASCII
    /// バイトを含む Origin は malformed header probe の兆候として warn
    /// レベルで記録する（MissingOrigin の info より強い信号）。
    OriginMalformed,
    /// Host ヘッダーは存在するが `to_str()` に失敗（非 ASCII バイト含む）
    ///
    /// 通常のブラウザ／プロキシは ASCII のみで構成された Host を送る。
    /// Host middleware 後段で非 ASCII バイトを含む Host に到達した場合は、
    /// middleware bypass、または Host 検証通過後の malformed probe として
    /// error レベルで記録する。通常運用では到達しない。
    HostMalformed,
    UntrustedHost,
    OriginParseError,
    UnsupportedScheme,
    /// scheme は http/https だが authority が欠落した Origin の拒否
    ///
    /// 現在の axum (http 1.x) では実測で以下のように振る舞い、本 variant に
    /// 到達する入力は確認できていない:
    /// - `"http:"` / `"http:path-only"` → `scheme_str() == None` → `UnsupportedScheme`
    /// - `"http:/"` / `"http:?query"` / `"http:///"` → `Uri::parse` 失敗 → `OriginParseError`
    ///
    /// 将来の http クレート挙動変更や、axum 以外のパスから到達した場合の
    /// 防御的フォールバックとして残し、DNS Rebinding 防御の核となる
    /// validation 経路から panic を排除する。
    #[allow(dead_code)]
    OriginMissingAuthority,
    UntrustedOriginAuthority,
    AuthorityMismatch,
}

fn is_host_middleware_bypass_indicator(rejection: WsOriginRejection) -> bool {
    match rejection {
        WsOriginRejection::MissingHost
        | WsOriginRejection::HostMalformed
        | WsOriginRejection::UntrustedHost => true,
        WsOriginRejection::MissingOrigin
        | WsOriginRejection::OriginMalformed
        | WsOriginRejection::OriginParseError
        | WsOriginRejection::UnsupportedScheme
        | WsOriginRejection::OriginMissingAuthority
        | WsOriginRejection::UntrustedOriginAuthority
        | WsOriginRejection::AuthorityMismatch => false,
    }
}

fn ws_rejection_log_level(rejection: WsOriginRejection) -> tracing::Level {
    if is_host_middleware_bypass_indicator(rejection) {
        tracing::Level::ERROR
    } else {
        match rejection {
            WsOriginRejection::MissingOrigin => tracing::Level::INFO,
            WsOriginRejection::OriginMalformed
            | WsOriginRejection::OriginParseError
            | WsOriginRejection::UnsupportedScheme
            | WsOriginRejection::OriginMissingAuthority
            | WsOriginRejection::UntrustedOriginAuthority
            | WsOriginRejection::AuthorityMismatch => tracing::Level::WARN,
            WsOriginRejection::MissingHost
            | WsOriginRejection::HostMalformed
            | WsOriginRejection::UntrustedHost => tracing::Level::ERROR,
        }
    }
}

fn ws_rejection_log_message(rejection: WsOriginRejection) -> &'static str {
    match rejection {
        WsOriginRejection::MissingHost
        | WsOriginRejection::HostMalformed
        | WsOriginRejection::UntrustedHost => "WS Host 検証異常",
        WsOriginRejection::MissingOrigin
        | WsOriginRejection::OriginMalformed
        | WsOriginRejection::OriginParseError
        | WsOriginRejection::UnsupportedScheme
        | WsOriginRejection::OriginMissingAuthority
        | WsOriginRejection::UntrustedOriginAuthority
        | WsOriginRejection::AuthorityMismatch => "WS Origin 拒否",
    }
}

fn emit_ws_rejection_log(
    level: tracing::Level,
    message: &'static str,
    host_recheck_anomaly: bool,
    rejection: WsOriginRejection,
    host: &str,
    origin: &str,
) {
    macro_rules! emit_ws_rejection_event {
        ($macro_name:ident) => {
            if host_recheck_anomaly {
                tracing::$macro_name!(
                    rejection = ?rejection,
                    host = host,
                    origin = origin,
                    ws_rejection_class = message,
                    host_recheck_anomaly = host_recheck_anomaly,
                    "[markdown-view] {} ({:?}): host={:?} origin={:?}; Host 系拒否は middleware bypass、または Host 検証通過後の malformed/untrusted probe。通常運用では到達しない",
                    message,
                    rejection,
                    host,
                    origin
                );
            } else {
                tracing::$macro_name!(
                    rejection = ?rejection,
                    host = host,
                    origin = origin,
                    ws_rejection_class = message,
                    host_recheck_anomaly = host_recheck_anomaly,
                    "[markdown-view] {} ({:?}): host={:?} origin={:?}",
                    message,
                    rejection,
                    host,
                    origin
                );
            }
        };
    }

    match level {
        tracing::Level::ERROR => {
            emit_ws_rejection_event!(error);
        }
        tracing::Level::WARN => {
            emit_ws_rejection_event!(warn);
        }
        tracing::Level::INFO => {
            emit_ws_rejection_event!(info);
        }
        tracing::Level::DEBUG => {
            emit_ws_rejection_event!(debug);
        }
        tracing::Level::TRACE => {
            emit_ws_rejection_event!(trace);
        }
    }
}

/// WebSocket Origin 検証を行い、許可時は `Ok(())`、拒否時は理由を返す
///
/// ヘッダー不在（`Missing*`）と非 ASCII 等で `to_str()` に失敗するケース
/// （`*Malformed`）を別 variant で区別し、呼び出し元でログレベルを
/// 段階化できるようにする。
pub(super) fn check_ws_origin(headers: &HeaderMap) -> Result<(), WsOriginRejection> {
    let host = match headers.get(HOST) {
        None => return Err(WsOriginRejection::MissingHost),
        Some(v) => match v.to_str() {
            Ok(s) => s,
            Err(_) => return Err(WsOriginRejection::HostMalformed),
        },
    };
    if !is_trusted_authority(host, "host") {
        return Err(WsOriginRejection::UntrustedHost);
    }
    let origin = match headers.get(ORIGIN) {
        None => return Err(WsOriginRejection::MissingOrigin),
        Some(v) => match v.to_str() {
            Ok(s) => s,
            Err(_) => return Err(WsOriginRejection::OriginMalformed),
        },
    };
    let Ok(origin_uri) = origin.parse::<Uri>() else {
        return Err(WsOriginRejection::OriginParseError);
    };
    match origin_uri.scheme_str() {
        Some("http") | Some("https") => {}
        _ => return Err(WsOriginRejection::UnsupportedScheme),
    }
    // 到達不能だが将来の http クレート挙動変更と axum 外経路からの防御的 fallback。
    // 背景は `WsOriginRejection::OriginMissingAuthority` の doc を参照。
    let Some(origin_authority) = origin_uri.authority() else {
        return Err(WsOriginRejection::OriginMissingAuthority);
    };
    if !is_trusted_authority(origin_authority.as_str(), "origin_authority") {
        return Err(WsOriginRejection::UntrustedOriginAuthority);
    }
    if normalize_authority(origin_authority.as_str()) != normalize_authority(host) {
        return Err(WsOriginRejection::AuthorityMismatch);
    }
    Ok(())
}

/// WebSocket接続時のOriginヘッダーを検証する
///
/// DNS Rebinding対策として、Host検証に加えてOriginのauthority一致も要求する。
/// Originスキームは`http`/`https`のみ許可する。
/// 拒否時は `check_ws_origin` の返す `WsOriginRejection` を使って
/// info / warn / error の監査ログを出力する。
pub(super) fn is_allowed_ws_origin(headers: &HeaderMap) -> bool {
    match check_ws_origin(headers) {
        Ok(()) => true,
        Err(rejection) => {
            let host = log_value_for_ws_host(headers);
            let origin = log_value_for_ws_origin(headers);
            let level = ws_rejection_log_level(rejection);
            let message = ws_rejection_log_message(rejection);
            let host_recheck_anomaly = is_host_middleware_bypass_indicator(rejection);
            emit_ws_rejection_log(
                level,
                message,
                host_recheck_anomaly,
                rejection,
                &host,
                &origin,
            );
            false
        }
    }
}

pub(super) fn is_trusted_authority(authority: &str, context: &'static str) -> bool {
    let Ok(parsed) = authority.parse::<Authority>() else {
        tracing::warn!(
            "[markdown-view] authority の parse に失敗し拒否 (context={})",
            context
        );
        return false;
    };
    // userinfo 付き authority (user@host 形式) は拒否する。
    // 現実の Host / Origin ヘッダーには userinfo は含まれず、
    // 攻撃者が任意 host 文字列を埋め込むバイパス経路になりうるため。
    // （例: "user@[::1]:3000" は http クレートのパーサを通過するが、
    //   host() が "[::1]" を返すため loopback 認定されてしまう）
    if parsed.as_str().contains('@') {
        tracing::warn!(
            "[markdown-view] authority に userinfo を検出し拒否 (context={})",
            context
        );
        return false;
    }
    // http クレート (1.x) の Authority パーサは非数値port（例: "[::1]:abc"）も受け入れ、
    // この場合 port() / port_u16() はいずれも None を返す（=無port扱い）。
    // DNS Rebinding境界として信頼するには数値portを必須とするため、
    // 元文字列を直接検査してport接尾辞の有無を判定する。
    if has_port_suffix(parsed.as_str()) && parsed.port_u16().is_none() {
        tracing::warn!(
            "[markdown-view] authority に非数値 port を検出し拒否 (context={})",
            context
        );
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
///
/// # 呼び出し側契約
///
/// 呼び出し元は本関数に渡す authority を事前に [`is_trusted_authority`] で
/// 検証すること。parse 失敗フォールバックは防御的保険であり、permissive な
/// lowercase/trim 比較によって DNS Rebinding 境界が弱まらないよう、
/// 呼び出し側で pre-validation を保証する必要がある。現行 `check_ws_origin`
/// はこの契約を満たしており、fallback 分岐は構造上到達不能。
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
    use std::collections::BTreeMap;
    use std::fmt;
    use std::sync::{Arc, Mutex};

    use axum::http::header::{HOST, ORIGIN};
    use axum::http::{HeaderMap, StatusCode};
    use axum::{middleware, routing::get, Router};
    use tracing::field::{Field, Visit};
    use tracing::{Event, Level, Subscriber};
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::{Layer, Registry};
    use tracing_test::traced_test;

    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct CapturedEvent {
        level: Level,
        fields: BTreeMap<String, String>,
    }

    #[derive(Default)]
    struct CapturedFields {
        values: BTreeMap<String, String>,
    }

    impl Visit for CapturedFields {
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.values
                .insert(field.name().to_string(), format!("{value:?}"));
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.values
                .insert(field.name().to_string(), value.to_string());
        }

        fn record_bool(&mut self, field: &Field, value: bool) {
            self.values
                .insert(field.name().to_string(), value.to_string());
        }
    }

    #[derive(Clone, Default)]
    struct EventCapture(Arc<Mutex<Vec<CapturedEvent>>>);

    impl<S> Layer<S> for EventCapture
    where
        S: Subscriber,
    {
        fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
            let mut fields = CapturedFields::default();
            event.record(&mut fields);
            self.0
                .lock()
                .expect("event capture lock")
                .push(CapturedEvent {
                    level: *event.metadata().level(),
                    fields: fields.values,
                });
        }
    }

    fn capture_ws_rejection_events(headers: &HeaderMap) -> Vec<CapturedEvent> {
        let capture = EventCapture::default();
        let events = Arc::clone(&capture.0);
        let subscriber = Registry::default().with(capture);

        tracing::subscriber::with_default(subscriber, || {
            assert!(!is_allowed_ws_origin(headers));
        });

        let captured = events.lock().expect("event capture lock").clone();
        captured
    }

    fn assert_captured_field_eq(event: &CapturedEvent, field: &str, expected: &str, context: &str) {
        assert!(
            event
                .fields
                .get(field)
                .is_some_and(|actual| actual == expected),
            "{context} の {field} field が不正"
        );
    }

    fn assert_captured_field_contains(
        event: &CapturedEvent,
        field: &str,
        expected_fragment: &str,
        context: &str,
    ) {
        assert!(
            event
                .fields
                .get(field)
                .is_some_and(|actual| actual.contains(expected_fragment)),
            "{context} の {field} field が不正"
        );
    }

    fn assert_captured_events_do_not_contain(
        events: &[CapturedEvent],
        forbidden_fragments: &[&str],
        context: &str,
    ) {
        assert!(
            events.iter().all(
                |event| event.fields.values().all(|value| forbidden_fragments
                    .iter()
                    .all(|fragment| !value.contains(fragment)))
            ),
            "{context} の監査ログに非公開値が混入している"
        );
    }

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
        assert!(is_trusted_authority("[::1]:3000", "host"));

        // 非数値 port：is_trusted_authority 内の has_port_suffix チェックで明示拒否
        // （httpクレートのAuthorityパーサ自体は非数値portを受け入れてしまうため、
        //  このガードが無いとloopback認定されて通過してしまう）
        assert!(!is_trusted_authority("[::1]:abc", "host"));

        // 非 loopback IPv6 + port：is_trusted_host 側で拒否
        assert!(!is_trusted_authority("[fe80::1]:3000", "host"));

        // 空 port 接尾辞：":"はあるが port_u16 が None → has_port_suffix チェックで拒否
        assert!(!is_trusted_authority("[::1]:", "host"));

        // u16 範囲外の port：u16 overflow → port_u16 が None → 拒否
        assert!(!is_trusted_authority("[::1]:65536", "host"));
        assert!(!is_trusted_authority("[::1]:99999", "host"));

        // port 0：RFC 上は予約だが port_u16 が Some(0) のため現状の実装では許可される。
        // 実害のない挙動を固定化することで、将来「0 を予約として拒否」する選択を
        // 意識的に行えるようにする
        assert!(is_trusted_authority("[::1]:0", "host"));

        // IPv6 zone ID：http クレートは解析を許すが、is_trusted_host の IpAddr::parse
        // が zone suffix 付き文字列を受け付けないため最終的に拒否される
        assert!(!is_trusted_authority("[fe80::1%25eth0]", "host"));
        assert!(!is_trusted_authority("[fe80::1%25eth0]:3000", "host"));
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
        assert!(!is_trusted_authority("user@localhost:3000", "host"));
        assert!(!is_trusted_authority("user@[::1]:3000", "host"));
        assert!(!is_trusted_authority("user:pass@localhost:3000", "host"));
        assert!(!is_trusted_authority("user:pass@[::1]:3000", "host"));
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
    fn test_request_host_guardは欠落hostをforbidden_jsonで拒否する() {
        let headers = HeaderMap::new();

        assert_request_host_guard_forbidden(&headers);
    }

    #[test]
    fn test_request_host_guardは非ascii_hostをforbidden_jsonで拒否する() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HOST,
            axum::http::HeaderValue::from_bytes(b"\xff non-ascii host").unwrap(),
        );

        assert_request_host_guard_forbidden(&headers);
    }

    fn assert_request_host_guard_forbidden(headers: &HeaderMap) {
        let error = ensure_allowed_request_host(headers).unwrap_err();
        assert_eq!(error.0, StatusCode::FORBIDDEN);
        assert_eq!(error.1["error"], "許可されていないHostヘッダーです");
    }

    #[tokio::test]
    #[traced_test]
    async fn test_request_host_guard拒否ログはpathを含みqueryを含めない() {
        let app = Router::new()
            .route("/api/search", get(|| async { "ok" }))
            .layer(middleware::from_fn(require_allowed_request_host));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let response = reqwest::Client::new()
            .get(format!("http://{}/api/search?q=secret", addr))
            .header("Host", format!("evil.example:{}", addr.port()))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(logs_contain("許可されていないHostヘッダーを拒否"));
        assert!(logs_contain("path=\"/api/search\""));
        assert!(!logs_contain("q=secret"));
    }

    #[tokio::test]
    async fn test_host_middlewareは不正hostを拒否して許可hostを通す() {
        let app = Router::new()
            .route("/probe", get(|| async { "ok" }))
            .layer(middleware::from_fn(require_allowed_request_host));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let attack_host = format!("evil.example:{}", addr.port());
        let rejected = client
            .get(format!("http://{}/probe", addr))
            .header("Host", &attack_host)
            .send()
            .await
            .unwrap();

        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
        let json: serde_json::Value = rejected.json().await.unwrap();
        assert!(json["error"].as_str().is_some());

        let allowed = client
            .get(format!("http://{}/probe", addr))
            .send()
            .await
            .unwrap();

        assert_eq!(allowed.status(), StatusCode::OK);
        assert_eq!(allowed.text().await.unwrap(), "ok");
    }

    #[test]
    fn test_allowed_ws_origin_valid() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert!(is_allowed_ws_origin(&headers));
        assert_eq!(check_ws_origin(&headers), Ok(()));
    }

    #[test]
    fn test_allowed_ws_origin_rejects_different_port() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:4000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
        // 両 authority が trusted かつ normalize 結果が異なるため AuthorityMismatch
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::AuthorityMismatch)
        );
    }

    #[test]
    fn test_allowed_ws_origin_rejects_ftp_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "ftp://localhost:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::UnsupportedScheme)
        );
    }

    #[test]
    fn test_allowed_ws_origin_rejects_different_host() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://evil.example:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
        // HOST は trusted だが Origin authority が trusted でないため
        // AuthorityMismatch ではなく UntrustedOriginAuthority に到達する
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::UntrustedOriginAuthority)
        );
    }

    #[test]
    fn test_allowed_ws_origin_missing_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::MissingOrigin)
        );
    }

    #[test]
    fn test_allowed_ws_origin_missing_host() {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::MissingHost)
        );
    }

    #[test]
    fn test_ws_origin拒否ログ分類は全variantを明示する() {
        let cases = [
            (
                WsOriginRejection::MissingHost,
                true,
                tracing::Level::ERROR,
                "WS Host 検証異常",
            ),
            (
                WsOriginRejection::HostMalformed,
                true,
                tracing::Level::ERROR,
                "WS Host 検証異常",
            ),
            (
                WsOriginRejection::UntrustedHost,
                true,
                tracing::Level::ERROR,
                "WS Host 検証異常",
            ),
            (
                WsOriginRejection::MissingOrigin,
                false,
                tracing::Level::INFO,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::OriginMalformed,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::OriginParseError,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::UnsupportedScheme,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::OriginMissingAuthority,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::UntrustedOriginAuthority,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
            (
                WsOriginRejection::AuthorityMismatch,
                false,
                tracing::Level::WARN,
                "WS Origin 拒否",
            ),
        ];

        for (rejection, is_bypass_indicator, level, message) in cases {
            assert_eq!(
                is_host_middleware_bypass_indicator(rejection),
                is_bypass_indicator,
                "{rejection:?} の Host bypass 分類が不正"
            );
            assert_eq!(
                ws_rejection_log_level(rejection),
                level,
                "{rejection:?} のログレベル分類が不正"
            );
            assert_eq!(
                ws_rejection_log_message(rejection),
                message,
                "{rejection:?} のログメッセージ分類が不正"
            );
        }
    }

    #[test]
    fn test_ws_host_bypass兆候は構造化errorログ契約として固定する() {
        let missing_host_and_origin = HeaderMap::new();

        let mut missing_host = HeaderMap::new();
        missing_host.insert(
            ORIGIN,
            "http://localhost:3000/private?token=secret"
                .parse()
                .unwrap(),
        );

        let mut host_malformed = HeaderMap::new();
        host_malformed.insert(
            ORIGIN,
            "http://localhost:3000/private?token=secret"
                .parse()
                .unwrap(),
        );
        host_malformed.insert(
            HOST,
            axum::http::HeaderValue::from_bytes(b"\xff non-ascii host").unwrap(),
        );

        let mut untrusted_host = HeaderMap::new();
        untrusted_host.insert(HOST, "evil.example:3000".parse().unwrap());
        untrusted_host.insert(
            ORIGIN,
            "http://localhost:3000/private?token=secret"
                .parse()
                .unwrap(),
        );

        let cases = [
            (
                missing_host_and_origin,
                "MissingHost",
                "<absent>",
                "<absent>",
            ),
            (
                missing_host,
                "MissingHost",
                "<absent>",
                "http://localhost:3000",
            ),
            (
                host_malformed,
                "HostMalformed",
                "<non-ascii>",
                "http://localhost:3000",
            ),
            (
                untrusted_host,
                "UntrustedHost",
                "evil.example:3000",
                "http://localhost:3000",
            ),
        ];

        for (headers, expected_rejection, expected_host, expected_origin) in cases {
            let events = capture_ws_rejection_events(&headers);
            assert_eq!(events.len(), 1, "{expected_rejection} の拒否ログ件数が不正");

            let event = &events[0];
            assert_eq!(
                event.level,
                Level::ERROR,
                "{expected_rejection} は Host middleware bypass 兆候として ERROR で記録する"
            );
            assert_captured_field_contains(
                event,
                "rejection",
                expected_rejection,
                expected_rejection,
            );
            assert_captured_field_eq(
                event,
                "ws_rejection_class",
                "WS Host 検証異常",
                expected_rejection,
            );
            assert_captured_field_eq(event, "host_recheck_anomaly", "true", expected_rejection);
            assert_captured_field_eq(event, "host", expected_host, expected_rejection);
            assert_captured_field_eq(event, "origin", expected_origin, expected_rejection);
            assert_captured_events_do_not_contain(
                std::slice::from_ref(event),
                &["private", "token", "secret"],
                expected_rejection,
            );
        }
    }

    #[test]
    fn test_ws_origin拒否はhost_bypass兆候として扱わない() {
        let mut missing_origin = HeaderMap::new();
        missing_origin.insert(HOST, "localhost:3000".parse().unwrap());

        let mut origin_malformed = HeaderMap::new();
        origin_malformed.insert(HOST, "localhost:3000".parse().unwrap());
        origin_malformed.insert(
            ORIGIN,
            axum::http::HeaderValue::from_bytes(b"\xff non-ascii origin").unwrap(),
        );

        let mut origin_parse_error = HeaderMap::new();
        origin_parse_error.insert(HOST, "localhost:3000".parse().unwrap());
        origin_parse_error.insert(ORIGIN, "not a uri".parse().unwrap());

        let mut unsupported_scheme = HeaderMap::new();
        unsupported_scheme.insert(HOST, "localhost:3000".parse().unwrap());
        unsupported_scheme.insert(
            ORIGIN,
            "ftp://localhost:3000/private?token=secret".parse().unwrap(),
        );

        let mut untrusted_origin_authority = HeaderMap::new();
        untrusted_origin_authority.insert(HOST, "localhost:3000".parse().unwrap());
        untrusted_origin_authority.insert(
            ORIGIN,
            "http://evil.example:3000/private?token=secret"
                .parse()
                .unwrap(),
        );

        let mut authority_mismatch = HeaderMap::new();
        authority_mismatch.insert(HOST, "localhost:3000".parse().unwrap());
        authority_mismatch.insert(
            ORIGIN,
            "http://127.0.0.1:3000/private?token=secret"
                .parse()
                .unwrap(),
        );

        let cases = [
            (
                missing_origin,
                Level::INFO,
                "MissingOrigin",
                "localhost:3000",
                "<absent>",
            ),
            (
                origin_malformed,
                Level::WARN,
                "OriginMalformed",
                "localhost:3000",
                "<non-ascii>",
            ),
            (
                origin_parse_error,
                Level::WARN,
                "OriginParseError",
                "localhost:3000",
                "<invalid-origin-uri>",
            ),
            (
                unsupported_scheme,
                Level::WARN,
                "UnsupportedScheme",
                "localhost:3000",
                "ftp://localhost:3000",
            ),
            (
                untrusted_origin_authority,
                Level::WARN,
                "UntrustedOriginAuthority",
                "localhost:3000",
                "http://evil.example:3000",
            ),
            (
                authority_mismatch,
                Level::WARN,
                "AuthorityMismatch",
                "localhost:3000",
                "http://127.0.0.1:3000",
            ),
        ];

        for (headers, expected_level, expected_rejection, expected_host, expected_origin) in cases {
            let events = capture_ws_rejection_events(&headers);
            assert_eq!(events.len(), 1, "{expected_rejection} の拒否ログ件数が不正");

            let event = &events[0];
            assert_eq!(
                event.level, expected_level,
                "{expected_rejection} の実ログ level が不正"
            );
            assert_captured_field_contains(
                event,
                "rejection",
                expected_rejection,
                expected_rejection,
            );
            assert_captured_field_eq(
                event,
                "ws_rejection_class",
                "WS Origin 拒否",
                expected_rejection,
            );
            assert_captured_field_eq(event, "host_recheck_anomaly", "false", expected_rejection);
            assert_captured_field_eq(event, "host", expected_host, expected_rejection);
            assert_captured_field_eq(event, "origin", expected_origin, expected_rejection);
            assert_captured_events_do_not_contain(
                std::slice::from_ref(event),
                &["private", "token", "secret", "user", "pass"],
                expected_rejection,
            );
        }
    }

    #[test]
    fn test_ws_origin_userinfoは監査ログに実値を残さない() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(
            ORIGIN,
            "http://alice:hunter2@localhost:3000/private?token=secret"
                .parse()
                .unwrap(),
        );

        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::UntrustedOriginAuthority)
        );

        let events = capture_ws_rejection_events(&headers);
        assert_captured_events_do_not_contain(
            &events,
            &["alice", "hunter2", "private", "token", "secret"],
            "userinfo 付き Origin",
        );

        let ws_event = events
            .iter()
            .find(|event| event.fields.contains_key("ws_rejection_class"))
            .expect("WS rejection event should be captured");
        assert_eq!(ws_event.level, Level::WARN);
        assert_captured_field_contains(
            ws_event,
            "rejection",
            "UntrustedOriginAuthority",
            "userinfo 付き Origin",
        );
        assert_captured_field_eq(
            ws_event,
            "origin",
            "<origin-authority-with-userinfo>",
            "userinfo 付き Origin",
        );
    }

    #[test]
    fn test_ws_host_untrusted入力は監査ログに実値を残さない() {
        let cases = [
            (
                "host_userinfo",
                "alice:hunter2@localhost:3000",
                "<host-authority-with-userinfo>",
            ),
            (
                "host_path_query",
                "localhost:3000/private?token=secret",
                "<invalid-host-authority>",
            ),
            (
                "host_invalid_port",
                "localhost:abc",
                "<invalid-host-authority>",
            ),
        ];

        for (case_label, host, expected_host_field) in cases {
            let mut headers = HeaderMap::new();
            headers.insert(HOST, host.parse().unwrap());
            headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());

            assert_eq!(
                check_ws_origin(&headers),
                Err(WsOriginRejection::UntrustedHost),
                "{case_label} は Host 検証異常として拒否する"
            );

            let events = capture_ws_rejection_events(&headers);
            assert_captured_events_do_not_contain(
                &events,
                &["alice", "hunter2", "private", "token", "secret"],
                case_label,
            );

            let ws_event = events
                .iter()
                .find(|event| event.fields.contains_key("ws_rejection_class"))
                .expect("WS rejection event should be captured");
            assert_eq!(ws_event.level, Level::ERROR);
            assert_captured_field_contains(ws_event, "rejection", "UntrustedHost", case_label);
            assert_captured_field_eq(ws_event, "host", expected_host_field, case_label);
        }
    }

    #[test]
    #[traced_test]
    fn test_ws_missing_hostはhost検証異常ログに記録する() {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());

        assert!(!is_allowed_ws_origin(&headers));
        assert!(logs_contain("WS Host 検証異常"));
        assert!(logs_contain(
            "middleware bypass、または Host 検証通過後の malformed/untrusted probe"
        ));
        assert!(logs_contain("MissingHost"));
        assert!(!logs_contain("WS Origin 拒否"));
    }

    #[test]
    #[traced_test]
    fn test_ws_host_malformedはhost検証異常ログに記録する() {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        headers.insert(
            HOST,
            axum::http::HeaderValue::from_bytes(b"\xff non-ascii host").unwrap(),
        );

        assert!(!is_allowed_ws_origin(&headers));
        assert!(logs_contain("WS Host 検証異常"));
        assert!(logs_contain(
            "middleware bypass、または Host 検証通過後の malformed/untrusted probe"
        ));
        assert!(logs_contain("HostMalformed"));
        assert!(logs_contain("<non-ascii>"));
        assert!(!logs_contain("WS Origin 拒否"));
    }

    #[test]
    #[traced_test]
    fn test_ws_untrusted_hostはhost検証異常ログに記録する() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "evil.example:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());

        assert!(!is_allowed_ws_origin(&headers));
        assert!(logs_contain("WS Host 検証異常"));
        assert!(logs_contain(
            "middleware bypass、または Host 検証通過後の malformed/untrusted probe"
        ));
        assert!(logs_contain("UntrustedHost"));
        assert!(logs_contain("evil.example:3000"));
        assert!(!logs_contain("WS Origin 拒否"));
    }

    #[test]
    #[traced_test]
    fn test_ws_missing_originは通常origin拒否ログに記録する() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());

        assert!(!is_allowed_ws_origin(&headers));
        assert!(logs_contain("WS Origin 拒否"));
        assert!(logs_contain("MissingOrigin"));
        assert!(!logs_contain(
            "middleware bypass、または Host 検証通過後の malformed/untrusted probe"
        ));
        assert!(!logs_contain("WS Host 検証異常"));
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
        assert_eq!(check_ws_origin(&headers), Ok(()));
    }

    #[test]
    fn test_allowed_ws_origin_ipv6_loopback許可と境界() {
        // 成功ケース：HOST と Origin が同一 IPv6 loopback authority
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "[::1]:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://[::1]:3000".parse().unwrap());
        assert!(is_allowed_ws_origin(&headers));
        assert_eq!(check_ws_origin(&headers), Ok(()));

        // 失敗ケース：port 不一致
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "[::1]:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://[::1]:4000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::AuthorityMismatch)
        );

        // 失敗ケース：非 loopback IPv6（link-local）は
        // HOST/Origin が一致していても拒否される
        // (HOST が trusted でない時点で UntrustedHost に到達)
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "[fe80::1]:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://[fe80::1]:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::UntrustedHost)
        );
    }

    #[test]
    fn test_check_ws_origin_variants_網羅() {
        // MissingHost: Host と Origin がどちらもない場合も Host bypass 兆候を優先する
        let headers = HeaderMap::new();
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::MissingHost)
        );

        // MissingHost: HOST ヘッダー不在
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::MissingHost)
        );

        // MissingOrigin: Origin ヘッダー不在
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::MissingOrigin)
        );

        // UntrustedHost: HOST が trusted でない
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "evil.example:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::UntrustedHost)
        );

        // OriginParseError: Origin が URI として parse 不可
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "not a uri".parse().unwrap());
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::OriginParseError)
        );

        // UnsupportedScheme: http/https 以外
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "ftp://localhost:3000".parse().unwrap());
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::UnsupportedScheme)
        );

        // UntrustedOriginAuthority: HOST は trusted、Origin authority が trusted でない
        // (AuthorityMismatch ではなく UntrustedOriginAuthority に到達する：
        //  is_trusted_authority("evil.example:3000") が false を返すため、
        //  AuthorityMismatch チェックより前に早期 return される)
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://evil.example:3000".parse().unwrap());
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::UntrustedOriginAuthority)
        );

        // AuthorityMismatch: 両 authority が trusted だが正規化結果が異なる
        // (localhost と 127.0.0.1 はどちらも is_trusted_host で true だが、
        //  normalize_authority の出力文字列が異なるため AuthorityMismatch 発火)
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://127.0.0.1:3000".parse().unwrap());
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::AuthorityMismatch)
        );

        // 評価順序の固定: UntrustedHost が OriginParseError より先に評価される。
        // HOST untrusted + Origin parse 不可の入力では、先に評価される HOST 側の
        // UntrustedHost が返ることを保証する（将来 check_ws_origin の早期 return
        // 順を入れ替えた場合に検出される）
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "evil.example:3000".parse().unwrap());
        headers.insert(ORIGIN, "not a uri".parse().unwrap());
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::UntrustedHost)
        );

        // OriginMalformed: Origin ヘッダーは存在するが to_str() 失敗（非 ASCII）
        // (HeaderValue は obs-text 範囲 0x80-0xFF を許容するが to_str() は
        //  visible ASCII のみ受理するため、\xff を含む入力で失敗する)
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "localhost:3000".parse().unwrap());
        headers.insert(
            ORIGIN,
            axum::http::HeaderValue::from_bytes(b"\xff non-ascii origin").unwrap(),
        );
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::OriginMalformed)
        );

        // HostMalformed: Host ヘッダーは存在するが to_str() 失敗（非 ASCII）
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        headers.insert(
            HOST,
            axum::http::HeaderValue::from_bytes(b"\xff non-ascii host").unwrap(),
        );
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::HostMalformed)
        );

        // HostMalformed は OriginMalformed より Host bypass 兆候として優先する
        let mut headers = HeaderMap::new();
        headers.insert(
            ORIGIN,
            axum::http::HeaderValue::from_bytes(b"\xff non-ascii origin").unwrap(),
        );
        headers.insert(
            HOST,
            axum::http::HeaderValue::from_bytes(b"\xff non-ascii host").unwrap(),
        );
        assert_eq!(
            check_ws_origin(&headers),
            Err(WsOriginRejection::HostMalformed)
        );

        // 注: OriginMissingAuthority variant は現行 axum (http 1.x) では
        // 到達可能な入力が実測確認できない（"http:", "http:/", "http:?q",
        // "http:path-only", "http:///" はいずれも scheme 欠落 or parse エラーに
        // 流れる）。ただし validation 経路の panic を排除する防御的 fallback として
        // variant と let-else 分岐を残しているため、本テストでの assertion は省略する。
    }

    #[test]
    fn test_is_trusted_authority_context_引数を受け取る() {
        // userinfo 経由バイパスは "host" コンテキストで拒否される
        assert!(!is_trusted_authority("user@localhost:3000", "host"));

        // 非数値 port は "origin_authority" コンテキストで拒否される
        // (warn ログには context=origin_authority が記録される)
        assert!(!is_trusted_authority("[::1]:abc", "origin_authority"));

        // 正常系: context 値に関わらず判定結果は不変
        assert!(is_trusted_authority("[::1]:3000", "host"));
        assert!(is_trusted_authority("localhost:3000", "origin_authority"));
    }
}
