use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::header::{HOST, ORIGIN};
use axum::http::uri::Authority;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use tokio::io::AsyncReadExt;
use tokio::sync::broadcast;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::renderer::render_markdown;
use crate::template::{render_page, UpdateMessage};
use crate::toc::generate_toc;

/// サーバー共有状態
pub struct AppState {
    pub file_path: PathBuf,
    pub dark_mode: bool,
    pub theme: Option<String>,
    pub tx: broadcast::Sender<String>,
}

/// ファイルサイズ上限（10MB）: OOM防止
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// axumルーターを構築する
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/ws", get(ws_handler))
        .route("/api/content", get(api_content_handler))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        // CSP: script-src/style-srcはインラインテンプレート埋め込みのため'unsafe-inline'を許可。
        // img-srcは外部画像参照のため*を許可。
        // sanitize_hrefはリンクのhref属性を対象とし、img srcのdata:スキームはCSP img-src側で制御する。
        // frame-ancestors 'none'でクリックジャッキングを防止。
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src *; connect-src 'self' ws: wss:; object-src 'none'; frame-ancestors 'none'",
            ),
        ))
        .with_state(state)
}

/// GET / : 初期HTMLページを返す
async fn index_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Html<String>, StatusCode> {
    if !is_allowed_request_host(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let (content, toc) = read_and_render(&state).await.map_err(|e| {
        eprintln!("[markdown-view] index読み込みエラー: {}", e);
        match e {
            ReadMarkdownError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ReadMarkdownError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    })?;
    let title = state
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("markdown-view");
    Ok(Html(render_page(title, &content, &toc, state.dark_mode)))
}

/// GET /api/content : 現在のコンテンツをJSON形式で返す
async fn api_content_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<UpdateMessage>, StatusCode> {
    if !is_allowed_request_host(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let (content, toc) = read_and_render(&state).await.map_err(|e| {
        eprintln!("[markdown-view] api/content読み込みエラー: {}", e);
        match e {
            ReadMarkdownError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ReadMarkdownError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    })?;
    Ok(Json(UpdateMessage { content, toc }))
}

/// GET /ws : WebSocketアップグレード
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_allowed_request_host(&headers) || !is_allowed_ws_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

fn is_allowed_request_host(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    is_trusted_authority(host)
}

fn is_allowed_ws_origin(headers: &HeaderMap) -> bool {
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

fn is_trusted_authority(authority: &str) -> bool {
    let Ok(authority) = authority.parse::<Authority>() else {
        return false;
    };
    is_trusted_host(authority.host())
}

fn is_trusted_host(host: &str) -> bool {
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

fn normalize_authority(authority: &str) -> String {
    authority.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// WebSocket接続を処理する
/// broadcastチャネルからメッセージを受信してクライアントに転送
async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.tx.subscribe();

    // 接続直後に現在のコンテンツを送信
    let (content, toc) = match read_and_render(&state).await {
        Ok(result) => result,
        Err(e) => {
            eprintln!("[markdown-view] WebSocket初期読み込みエラー: {}", e);
            if let Err(e) = socket
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 1011,
                    reason: "ファイル読み込みエラー".into(),
                })))
                .await
            {
                eprintln!("[markdown-view] WebSocket closeフレーム送信エラー: {}", e);
            }
            return;
        }
    };
    let msg = match serde_json::to_string(&UpdateMessage { content, toc }) {
        Ok(json) => json,
        Err(e) => {
            eprintln!("[markdown-view] JSONシリアライズエラー: {}", e);
            if let Err(e) = socket
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 1011,
                    reason: "内部エラー".into(),
                })))
                .await
            {
                eprintln!("[markdown-view] WebSocket closeフレーム送信エラー: {}", e);
            }
            return;
        }
    };
    if let Err(e) = socket.send(Message::Text(msg.into())).await {
        eprintln!("[markdown-view] WebSocket初期送信エラー: {}", e);
        return;
    }

    // broadcastチャネルからの更新を転送
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) => {
                        break;
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if let Err(e) = socket.send(Message::Pong(payload)).await {
                            eprintln!("[markdown-view] WebSocket pong送信エラー: {}", e);
                            break;
                        }
                    }
                    Some(Ok(_)) => {
                        // クライアントからのメッセージは現状未使用（読み捨て）。
                        // サーバーは配信専用のため、クライアント入力を処理しない設計。
                    }
                    Some(Err(e)) => {
                        eprintln!("[markdown-view] WebSocket受信エラー: {}", e);
                        break;
                    }
                    None => {
                        break;
                    }
                }
            }
            recv = rx.recv() => {
                match recv {
                    Ok(msg) => {
                        if let Err(e) = socket.send(Message::Text(msg.into())).await {
                            eprintln!("[markdown-view] WebSocket送信エラー: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // 遅延クライアントに最新コンテンツを再送信
                        eprintln!(
                            "[markdown-view] WebSocketクライアントが{}メッセージ遅延、最新コンテンツを再送信",
                            n
                        );
                        let (content, toc) = match read_and_render(&state).await {
                            Ok(result) => result,
                            Err(e) => {
                                eprintln!("[markdown-view] WebSocket再送信読み込みエラー: {}", e);
                                // クライアントにエラーを通知
                                if let Ok(error_json) = serde_json::to_string(&serde_json::json!({
                                    "error": format!("ファイル読み込みエラー: {}", e)
                                })) {
                                    let _ = socket.send(Message::Text(error_json.into())).await;
                                }
                                continue;
                            }
                        };
                        let resend = match serde_json::to_string(&UpdateMessage { content, toc }) {
                            Ok(json) => json,
                            Err(e) => {
                                eprintln!("[markdown-view] 再送信JSONシリアライズエラー: {}", e);
                                continue;
                            }
                        };
                        if let Err(e) = socket.send(Message::Text(resend.into())).await {
                            eprintln!("[markdown-view] WebSocket再送信エラー: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
enum ReadMarkdownError {
    Io(std::io::Error),
    TooLarge,
}

impl std::fmt::Display for ReadMarkdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadMarkdownError::Io(e) => write!(f, "I/Oエラー: {}", e),
            ReadMarkdownError::TooLarge => {
                write!(f, "ファイルサイズが上限（10MB）を超えています")
            }
        }
    }
}

/// ファイルサイズ上限付きでMarkdownファイルを読み込む
///
/// TOCTOU対策として二段階のサイズチェックを行う:
/// 1. metadata().len() による事前チェック（明らかな超過を早期拒否）
/// 2. take() + read_to_end による読み込み時の実サイズ制限
async fn read_markdown_with_limit(file_path: &Path) -> Result<String, ReadMarkdownError> {
    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(ReadMarkdownError::Io)?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err(ReadMarkdownError::TooLarge);
    }

    let file = tokio::fs::File::open(file_path)
        .await
        .map_err(ReadMarkdownError::Io)?;
    let mut limited_reader = file.take(MAX_FILE_SIZE + 1);
    let mut buffer = Vec::new();
    limited_reader
        .read_to_end(&mut buffer)
        .await
        .map_err(ReadMarkdownError::Io)?;
    if buffer.len() as u64 > MAX_FILE_SIZE {
        return Err(ReadMarkdownError::TooLarge);
    }

    String::from_utf8(buffer)
        .map_err(|e| ReadMarkdownError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
}

/// ファイルを読み込んでレンダリングする
async fn read_and_render(state: &AppState) -> Result<(String, String), ReadMarkdownError> {
    let markdown = read_markdown_with_limit(&state.file_path).await?;
    let content = render_markdown(&markdown, state.theme.as_deref());
    let toc = generate_toc(&markdown);
    Ok((content, toc))
}

/// ファイル変更時にbroadcastで全クライアントに通知する
///
/// 読み込みエラー時はエラーJSONをクライアントに送信する。
/// JS側の `data.error` チェックでエラー表示される。
pub async fn notify_update(state: &AppState) {
    let msg = match read_and_render(state).await {
        Ok((content, toc)) => match serde_json::to_string(&UpdateMessage { content, toc }) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("[markdown-view] JSONシリアライズエラー: {}", e);
                return;
            }
        },
        Err(e) => {
            eprintln!("[markdown-view] 更新時読み込みエラー: {}", e);
            // クライアントにエラーを通知（JS側のdata.errorチェックで処理される）
            match serde_json::to_string(&serde_json::json!({
                "error": format!("ファイル読み込みエラー: {}", e)
            })) {
                Ok(json) => json,
                Err(_) => return,
            }
        }
    };
    // 受信者がいない場合は正常（クライアント接続時に最新をフェッチするため）
    let _ = state.tx.send(msg);
}

#[cfg(test)]
mod tests {
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
        // 127.0.0.0/8は全てループバック
        assert!(is_trusted_host("127.0.0.2"));
        assert!(!is_trusted_host("0.0.0.0"));
    }

    #[test]
    fn test_trusted_host_loopback_ipv6() {
        assert!(is_trusted_host("[::1]"));
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
}
