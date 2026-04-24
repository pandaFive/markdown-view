//! axumルーターとHTTP/WebSocketハンドラを構築する。

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use tower_http::set_header::SetResponseHeaderLayer;

use super::files::{
    list_markdown_files, load_route_memo, load_route_update, resolve_route_target, save_route_memo,
    search_directory, ResolvedTarget, RouteTargetRequest, SearchResponse, MAX_FILE_SIZE,
};
use super::guards::{
    build_csp_header, ensure_allowed_request_host, is_allowed_ws_origin, json_error,
};
use super::messages::{ApiError, BroadcastMessage};
use super::session::handle_socket;
use super::state::AppState;
use crate::template::{
    render_page, MemoResponse, MemoUpdateMessage, RenderPageParams, SidebarParams, UpdateMessage,
};

const MEMO_JSON_BODY_LIMIT: usize = (MAX_FILE_SIZE as usize * 2) + 4096;

fn sidebar_directory_name(state: &AppState) -> &str {
    state
        .mode()
        .directory()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Documents")
}

/// axumルーターを構築する
pub fn create_router(state: Arc<AppState>) -> Router {
    let csp_header = build_csp_header(state.syntax_css());
    Router::new()
        .route("/", get(index_handler))
        .route("/ws", get(ws_handler))
        .route("/api/content", get(api_content_handler))
        .route("/api/search", get(api_search_handler))
        .route(
            "/api/memo",
            get(api_memo_handler)
                .put(api_memo_save_handler)
                .layer(DefaultBodyLimit::max(MEMO_JSON_BODY_LIMIT)),
        )
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
        .with_state(state)
}

/// クエリパラメータ
#[derive(serde::Deserialize, Default)]
struct FileQuery {
    file: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct SearchQuery {
    q: Option<String>,
}

#[derive(serde::Deserialize)]
struct MemoSaveRequest {
    file: Option<String>,
    raw: String,
}

#[derive(Debug)]
struct RouteContext<'a> {
    state: &'a Arc<AppState>,
    target: ResolvedTarget,
    request: RouteTargetRequest<'a>,
}

impl<'a> RouteContext<'a> {
    fn ensure_allowed(headers: &HeaderMap) -> Result<(), ApiError> {
        ensure_allowed_request_host(headers)
    }

    fn resolve(
        state: &'a Arc<AppState>,
        headers: &HeaderMap,
        request: RouteTargetRequest<'a>,
    ) -> Result<Self, ApiError> {
        Self::ensure_allowed(headers)?;
        let target = resolve_route_target(state, request)?;
        Ok(Self {
            state,
            target,
            request,
        })
    }

    async fn load_update(&self) -> Result<UpdateMessage, ApiError> {
        load_route_update(&self.target, self.request).await
    }

    async fn load_memo(&self) -> Result<MemoResponse, ApiError> {
        load_route_memo(self.state, &self.target, self.memo_request()).await
    }

    async fn save_memo(&self, raw: String) -> Result<MemoResponse, ApiError> {
        save_route_memo(self.state, &self.target, raw, self.memo_request()).await
    }

    fn broadcast_saved_memo(&self) {
        if self.state.tx().receiver_count() == 0 {
            return;
        }

        let _ = self
            .state
            .tx()
            .send(BroadcastMessage::MemoUpdate(MemoUpdateMessage::new(
                self.memo_message_file(),
            )));
    }

    fn memo_request(&self) -> RouteTargetRequest<'a> {
        RouteTargetRequest::api_memo(self.request.query_file())
    }

    fn memo_message_file(&self) -> String {
        self.target
            .relative_path()
            .map(ToOwned::to_owned)
            .or_else(|| {
                self.target
                    .file_path()
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| self.target.file_path().display().to_string())
    }

    fn title(&self) -> &str {
        self.target
            .file_path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("markdown-view")
    }

    fn sidebar(&self) -> SidebarParams<'_> {
        match self.target.file_list() {
            Some(files) => SidebarParams::Directory {
                directory_name: sidebar_directory_name(self.state),
                file_list: files,
                current_file: self.target.relative_path(),
            },
            None => SidebarParams::SingleFile,
        }
    }

    fn target(&self) -> &ResolvedTarget {
        &self.target
    }
}

/// GET / : 初期HTMLページを返す
async fn index_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<FileQuery>,
) -> Result<Html<String>, ApiError> {
    let context = RouteContext::resolve(
        &state,
        &headers,
        RouteTargetRequest::page(query.file.as_deref()),
    )?;
    let update = context.load_update().await?;
    let memo = match context.load_memo().await {
        Ok(memo) => memo,
        Err(error) => {
            tracing::warn!(
                "[markdown-view] index描画ではメモ読み込み失敗を空メモへフォールバック ({}): {:?}",
                context.target().file_label(),
                error
            );
            MemoResponse::empty(context.target().relative_path().map(ToOwned::to_owned))
        }
    };

    Ok(Html(render_page(RenderPageParams {
        title: context.title(),
        content: update.content(),
        toc: update.toc(),
        memo: &memo,
        dark_mode: state.dark_mode(),
        syntax_css: state.syntax_css(),
        sidebar: context.sidebar(),
    })))
}

/// GET /api/content : 現在のコンテンツをJSON形式で返す
async fn api_content_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<FileQuery>,
) -> Result<Json<UpdateMessage>, ApiError> {
    let context = RouteContext::resolve(
        &state,
        &headers,
        RouteTargetRequest::api_content(query.file.as_deref()),
    )?;
    let update = context.load_update().await?;

    Ok(Json(update))
}

/// GET /api/memo : 現在のメモをJSON形式で返す
async fn api_memo_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<FileQuery>,
) -> Result<Json<MemoResponse>, ApiError> {
    let context = RouteContext::resolve(
        &state,
        &headers,
        RouteTargetRequest::api_memo(query.file.as_deref()),
    )?;
    let memo = context.load_memo().await?;

    Ok(Json(memo))
}

/// PUT /api/memo : メモを保存してJSON形式で返す
async fn api_memo_save_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<MemoSaveRequest>,
) -> Result<Json<MemoResponse>, ApiError> {
    let context = RouteContext::resolve(
        &state,
        &headers,
        RouteTargetRequest::api_memo(payload.file.as_deref()),
    )?;
    let memo = context.save_memo(payload.raw).await?;
    context.broadcast_saved_memo();

    Ok(Json(memo))
}

/// GET /api/files : ディレクトリ内の.mdファイル一覧をJSON形式で返す
async fn api_files_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, ApiError> {
    RouteContext::ensure_allowed(&headers)?;

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

/// GET /api/search : ディレクトリ全体検索結果をJSON形式で返す
async fn api_search_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    RouteContext::ensure_allowed(&headers)?;

    let query = query.q.unwrap_or_default();
    let Some(base_dir) = state.mode().directory() else {
        return Ok(Json(SearchResponse {
            query: query.trim().to_string(),
            results: Vec::new(),
            searched_files: 0,
            skipped_files: 0,
        }));
    };

    let response = search_directory(base_dir, &query).await.map_err(|error| {
        tracing::warn!("[markdown-view] ディレクトリ検索エラー: {}", error);
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ディレクトリ検索に失敗しました",
        )
    })?;

    Ok(Json(response))
}

/// GET /ws : WebSocketアップグレード
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // HOST 経路の拒否を監査ログに残すため ensure_allowed_request_host を使う。
    // `||` の短絡評価により、HOST 拒否時は is_allowed_ws_origin (内部で HOST を
    // 再チェックする) が走らず、重複ログを防ぐ。
    if ensure_allowed_request_host(&headers).is_err() || !is_allowed_ws_origin(&headers) {
        return json_error(StatusCode::FORBIDDEN, "WebSocket接続元が許可されていません")
            .into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    use super::super::files::RouteTargetKind;

    fn create_directory_state(base_dir: &std::path::Path) -> Arc<AppState> {
        let (tx, _rx) = broadcast::channel(16);
        Arc::new(AppState::new(
            super::super::state::AppMode::new_directory(base_dir).unwrap(),
            false,
            None,
            tx,
        ))
    }

    #[test]
    fn test_route_context_pageからmemoリクエストを派生できる() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("README.md"), "# README").unwrap();
        std::fs::write(dir.path().join("docs/guide.md"), "# Guide").unwrap();
        let state = create_directory_state(dir.path());
        let mut headers = HeaderMap::new();
        headers.insert("Host", "127.0.0.1:3000".parse().unwrap());

        let context = RouteContext::resolve(
            &state,
            &headers,
            RouteTargetRequest::page(Some("docs/guide.md")),
        )
        .unwrap();

        let memo_request = context.memo_request();
        assert_eq!(memo_request.kind(), RouteTargetKind::ApiMemo);
        assert_eq!(memo_request.query_file(), Some("docs/guide.md"));
    }

    #[test]
    fn test_route_context_不正hostを拒否する() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# README").unwrap();
        let state = create_directory_state(dir.path());
        let mut headers = HeaderMap::new();
        headers.insert("Host", "evil.example:3000".parse().unwrap());

        let error = RouteContext::resolve(&state, &headers, RouteTargetRequest::page(None))
            .expect_err("不正なHostヘッダは拒否されるべき");

        assert_eq!(error.0, StatusCode::FORBIDDEN);
    }
}
