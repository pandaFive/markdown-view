//! axumルーターとHTTP/WebSocketハンドラを構築する。

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use tower_http::set_header::SetResponseHeaderLayer;

use super::files::{list_markdown_files, search_directory, SearchResponse, MAX_FILE_SIZE};
use super::guards::{
    build_csp_header, ensure_allowed_request_host, is_allowed_ws_origin, json_error,
};
use super::messages::ApiError;
use super::service::{
    self, ContentRequest, MemoRequest, PageRequest, SaveMemoRequest, SidebarView,
};
use super::session::handle_socket;
use super::state::AppState;
use crate::template::{render_page, MemoResponse, RenderPageParams, SidebarParams, UpdateMessage};

#[cfg(test)]
use super::files::{resolve_route_target, ResolvedTarget, RouteTargetRequest};

// メモ本文の保存上限は save_route_memo 側の MAX_FILE_SIZE で判定する。
// ここは JSON envelope と string escape を含む HTTP body の上限。
// 通常の Markdown 本文で多い backslash や quote の escape 膨張を想定する。
// 制御文字など 2 倍を超えて膨らむ極端な JSON 入力は body limit 側で拒否され得る。
// 4096 bytes は MemoSaveRequest の現在の envelope と小さな schema 変更用の余白。
const MEMO_JSON_BODY_LIMIT: usize = (MAX_FILE_SIZE as usize * 2) + 4096;

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

fn sidebar_params(sidebar: &SidebarView) -> SidebarParams<'_> {
    match sidebar {
        SidebarView::SingleFile => SidebarParams::SingleFile,
        SidebarView::Directory {
            directory_name,
            file_list,
            current_file,
        } => SidebarParams::Directory {
            directory_name,
            file_list,
            current_file: current_file.as_deref(),
        },
    }
}

#[derive(Debug)]
#[cfg(test)]
struct RouteContext<'a> {
    _state: &'a Arc<AppState>,
    _target: ResolvedTarget,
    request: RouteTargetRequest<'a>,
}

#[cfg(test)]
impl<'a> RouteContext<'a> {
    fn resolve(
        state: &'a Arc<AppState>,
        headers: &HeaderMap,
        request: RouteTargetRequest<'a>,
    ) -> Result<Self, ApiError> {
        ensure_allowed_request_host(headers)?;
        let target = resolve_route_target(state, request)?;
        Ok(Self {
            _state: state,
            _target: target,
            request,
        })
    }

    fn memo_request(&self) -> RouteTargetRequest<'a> {
        RouteTargetRequest::api_memo(self.request.query_file())
    }
}

/// GET / : 初期HTMLページを返す
async fn index_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<FileQuery>,
) -> Result<Html<String>, ApiError> {
    ensure_allowed_request_host(&headers)?;
    let page = service::load_page(
        &state,
        PageRequest {
            file: query.file.as_deref(),
        },
    )
    .await?;

    Ok(Html(render_page(RenderPageParams {
        title: &page.title,
        content: page.update.content(),
        toc: page.update.toc(),
        memo: &page.memo,
        dark_mode: state.dark_mode(),
        syntax_css: state.syntax_css(),
        sidebar: sidebar_params(&page.sidebar),
    })))
}

/// GET /api/content : 現在のコンテンツをJSON形式で返す
async fn api_content_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<FileQuery>,
) -> Result<Json<UpdateMessage>, ApiError> {
    ensure_allowed_request_host(&headers)?;
    let update = service::load_content(
        &state,
        ContentRequest {
            file: query.file.as_deref(),
        },
    )
    .await?;

    Ok(Json(update))
}

/// GET /api/memo : 現在のメモをJSON形式で返す
async fn api_memo_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<FileQuery>,
) -> Result<Json<MemoResponse>, ApiError> {
    ensure_allowed_request_host(&headers)?;
    let memo = service::load_memo(
        &state,
        MemoRequest {
            file: query.file.as_deref(),
        },
    )
    .await?;

    Ok(Json(memo))
}

/// PUT /api/memo : メモを保存してJSON形式で返す
async fn api_memo_save_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<MemoSaveRequest>,
) -> Result<Json<MemoResponse>, ApiError> {
    ensure_allowed_request_host(&headers)?;
    let memo = service::save_memo(
        &state,
        SaveMemoRequest {
            file: payload.file.as_deref(),
            raw: payload.raw,
        },
    )
    .await?;

    Ok(Json(memo))
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

/// GET /api/search : ディレクトリ全体検索結果をJSON形式で返す
async fn api_search_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    ensure_allowed_request_host(&headers)?;

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
