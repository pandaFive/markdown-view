//! axumルーターとHTTP/WebSocketハンドラを構築する。

use std::sync::Arc;

use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use tower_http::set_header::SetResponseHeaderLayer;

use super::files::{
    list_markdown_files, read_rendered_update_or_error, resolve_target_file_or_error,
    TargetResolveContext,
};
use super::guards::{
    build_csp_header, ensure_allowed_request_host, is_allowed_request_host, is_allowed_ws_origin,
    json_error,
};
use super::state::AppState;
use super::websocket::handle_socket;
use crate::template::{render_page, RenderPageParams, SidebarParams, UpdateMessage};

pub(super) type ApiError = (StatusCode, Json<serde_json::Value>);

/// axumルーターを構築する
pub fn create_router(state: Arc<AppState>) -> Router {
    let (csp_header, csp_fallback) = build_csp_header(state.syntax_css());
    let security_warning = if csp_fallback {
        HeaderValue::from_static("csp-fallback")
    } else {
        HeaderValue::from_static("none")
    };
    Router::new()
        .route("/", get(index_handler))
        .route("/ws", get(ws_handler))
        .route("/api/content", get(api_content_handler))
        .route("/api/files", get(api_files_handler))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        // CSP: scriptはハッシュベース許可を維持し、unsafe-inlineを排除する。
        // styleはsyntect class-basedハイライトを使用し、unsafe-inlineを許可しない。
        // img-srcは同一オリジンのみに制限し、Markdown経由の外部画像読込を既定拒否する。
        // renderer モジュール側でリンクと画像に個別のURLポリシーを適用する。
        // frame-ancestors 'none'でクリックジャッキングを防止。
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            csp_header,
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-markdown-view-security-warning"),
            security_warning,
        ))
        .with_state(state)
}

/// クエリパラメータ
#[derive(serde::Deserialize, Default)]
struct FileQuery {
    file: Option<String>,
}

/// GET / : 初期HTMLページを返す
async fn index_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<FileQuery>,
) -> Result<Html<String>, ApiError> {
    ensure_allowed_request_host(&headers)?;

    let (file_path, file_list) = resolve_target_file_or_error(
        &state,
        query.file.as_deref(),
        true,
        TargetResolveContext::Index,
    )?;

    let update = read_rendered_update_or_error(&file_path, TargetResolveContext::Index).await?;

    let title = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("markdown-view");

    let current_file = state.mode().relative_path_of(&file_path);

    Ok(Html(render_page(RenderPageParams {
        title,
        content: update.content(),
        toc: update.toc(),
        dark_mode: state.dark_mode(),
        syntax_css: state.syntax_css(),
        sidebar: match file_list.as_deref() {
            Some(files) => SidebarParams::Directory {
                file_list: files,
                current_file: current_file.as_deref(),
            },
            None => SidebarParams::SingleFile,
        },
    })))
}

/// GET /api/content : 現在のコンテンツをJSON形式で返す
async fn api_content_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<FileQuery>,
) -> Result<Json<UpdateMessage>, ApiError> {
    ensure_allowed_request_host(&headers)?;

    let (file_path, _) = resolve_target_file_or_error(
        &state,
        query.file.as_deref(),
        false,
        TargetResolveContext::ApiContent,
    )?;

    let update =
        read_rendered_update_or_error(&file_path, TargetResolveContext::ApiContent).await?;

    Ok(Json(
        update.with_file(state.mode().relative_path_of(&file_path)),
    ))
}

/// GET /api/files : ディレクトリ内の.mdファイル一覧をJSON形式で返す
async fn api_files_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, ApiError> {
    ensure_allowed_request_host(&headers)?;

    if let Some(base) = state.mode().directory() {
        let files = list_markdown_files(base).map_err(|e| {
            tracing::warn!("[markdown-view] ファイル一覧取得エラー: {}", e);
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ファイル一覧の取得に失敗しました",
            )
        })?;
        Ok(Json(files))
    } else {
        Ok(Json(vec![]))
    }
}

/// GET /ws : WebSocketアップグレード
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_allowed_request_host(&headers) || !is_allowed_ws_origin(&headers) {
        return json_error(StatusCode::FORBIDDEN, "WebSocket接続元が許可されていません")
            .into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}
