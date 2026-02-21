use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::header::{HOST, ORIGIN};
use axum::http::uri::Authority;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
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
        // img-srcは外部画像参照のため*を許可（data:スキームはsanitize_hrefで除外済み）。
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
    let (content, toc) = read_and_render(&state).await;
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
    let (content, toc) = read_and_render(&state).await;
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
    let (content, toc) = read_and_render(&state).await;
    let msg = match serde_json::to_string(&UpdateMessage { content, toc }) {
        Ok(json) => json,
        Err(e) => {
            eprintln!("[markdown-view] JSONシリアライズエラー: {}", e);
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
                        let (content, toc) = read_and_render(&state).await;
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

/// ファイルを読み込んでレンダリングする
async fn read_and_render(state: &AppState) -> (String, String) {
    // ファイルサイズチェック（OOM防止）
    let metadata = match tokio::fs::metadata(&state.file_path).await {
        Ok(m) => m,
        Err(e) => return (format!("ファイルアクセスエラー: {}", e), String::new()),
    };

    if metadata.len() > MAX_FILE_SIZE {
        return (
            "ファイルサイズが上限（10MB）を超えています".to_string(),
            String::new(),
        );
    }

    let markdown = match tokio::fs::read_to_string(&state.file_path).await {
        Ok(content) => content,
        Err(e) => return (format!("ファイル読み込みエラー: {}", e), String::new()),
    };

    let content = render_markdown(&markdown, state.theme.as_deref());
    let toc = generate_toc(&markdown);
    (content, toc)
}

/// ファイル変更時にbroadcastで全クライアントに通知する
pub async fn notify_update(state: &AppState) {
    let (content, toc) = read_and_render(state).await;
    let msg = match serde_json::to_string(&UpdateMessage { content, toc }) {
        Ok(json) => json,
        Err(e) => {
            eprintln!("[markdown-view] JSONシリアライズエラー: {}", e);
            return;
        }
    };
    // 送信失敗は無視（受信者がいない場合）
    let _ = state.tx.send(msg);
}
