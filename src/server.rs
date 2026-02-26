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

/// canonicalize済みの絶対パス
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalPath(PathBuf);

impl CanonicalPath {
    /// パスをcanonicalizeして`CanonicalPath`を生成する
    pub fn try_from_path(path: impl AsRef<Path>) -> Result<Self, CanonicalPathError> {
        let canonical = path
            .as_ref()
            .canonicalize()
            .map_err(CanonicalPathError::Canonicalize)?;
        Ok(Self(canonical))
    }

    /// `Path`として参照する
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// `CanonicalPath` 生成エラー
#[derive(Debug)]
pub enum CanonicalPathError {
    /// canonicalize失敗
    Canonicalize(std::io::Error),
}

impl std::fmt::Display for CanonicalPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CanonicalPathError::Canonicalize(e) => {
                write!(f, "パスの正規化に失敗しました: {}", e)
            }
        }
    }
}

impl std::error::Error for CanonicalPathError {}

/// `AppMode` 構築エラー
#[derive(Debug)]
pub enum AppModeBuildError {
    /// canonicalize済みパスの生成に失敗
    CanonicalPath(CanonicalPathError),
    /// 単一ファイルモードでファイル以外が指定された
    NotFile(PathBuf),
    /// ディレクトリモードでディレクトリ以外が指定された
    NotDirectory(PathBuf),
    /// 単一ファイルモードで.md以外が指定された
    NotMarkdown(PathBuf),
}

impl std::fmt::Display for AppModeBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppModeBuildError::CanonicalPath(e) => write!(f, "{}", e),
            AppModeBuildError::NotFile(path) => {
                write!(
                    f,
                    "単一ファイルモードにはファイルを指定してください: {}",
                    path.display()
                )
            }
            AppModeBuildError::NotDirectory(path) => {
                write!(
                    f,
                    "ディレクトリモードにはディレクトリを指定してください: {}",
                    path.display()
                )
            }
            AppModeBuildError::NotMarkdown(path) => {
                write!(f, ".mdファイルのみ指定可能です: {}", path.display())
            }
        }
    }
}

impl std::error::Error for AppModeBuildError {}

#[derive(Debug, Clone)]
enum AppModeKind {
    SingleFile(CanonicalPath),
    Directory(CanonicalPath),
}

/// アプリケーション動作モード
#[derive(Debug, Clone)]
pub struct AppMode(AppModeKind);

impl AppMode {
    /// 単一ファイルモードを生成する（canonicalize済み・.mdファイルのみ許可）
    pub fn new_single_file(path: impl AsRef<Path>) -> Result<Self, AppModeBuildError> {
        let canonical =
            CanonicalPath::try_from_path(path).map_err(AppModeBuildError::CanonicalPath)?;
        if !canonical.as_path().is_file() {
            return Err(AppModeBuildError::NotFile(
                canonical.as_path().to_path_buf(),
            ));
        }
        match canonical.as_path().extension() {
            Some(ext) if ext.eq_ignore_ascii_case("md") => {}
            _ => {
                return Err(AppModeBuildError::NotMarkdown(
                    canonical.as_path().to_path_buf(),
                ))
            }
        }
        Ok(Self(AppModeKind::SingleFile(canonical)))
    }

    /// ディレクトリモードを生成する（canonicalize済みディレクトリのみ許可）
    pub fn new_directory(path: impl AsRef<Path>) -> Result<Self, AppModeBuildError> {
        let canonical =
            CanonicalPath::try_from_path(path).map_err(AppModeBuildError::CanonicalPath)?;
        if !canonical.as_path().is_dir() {
            return Err(AppModeBuildError::NotDirectory(
                canonical.as_path().to_path_buf(),
            ));
        }
        Ok(Self(AppModeKind::Directory(canonical)))
    }

    /// ベースディレクトリを返す（ファイルモードは親、ディレクトリモードはそのまま）
    pub fn base_dir(&self) -> &Path {
        match &self.0 {
            AppModeKind::SingleFile(p) => p.as_path().parent().unwrap_or(p.as_path()),
            AppModeKind::Directory(p) => p.as_path(),
        }
    }

    /// 単一ファイルモードのパスを返す（ディレクトリモードはNone）
    pub fn single_file(&self) -> Option<&Path> {
        match &self.0 {
            AppModeKind::SingleFile(p) => Some(p.as_path()),
            AppModeKind::Directory(_) => None,
        }
    }

    /// ディレクトリモードのパスを返す（単一ファイルモードはNone）
    pub fn directory(&self) -> Option<&Path> {
        match &self.0 {
            AppModeKind::SingleFile(_) => None,
            AppModeKind::Directory(p) => Some(p.as_path()),
        }
    }

    /// ディレクトリモードかどうか
    pub fn is_directory(&self) -> bool {
        matches!(self.0, AppModeKind::Directory(_))
    }

    /// ディレクトリモード時にファイルの相対パスを計算する
    ///
    /// `file_path` はcanonicalize済みの絶対パスであること。
    /// strip_prefix失敗時はエラーログを出力してNoneを返す。
    /// 単一ファイルモードでは常にNoneを返す。
    pub fn relative_path_of(&self, file_path: &Path) -> Option<String> {
        match &self.0 {
            AppModeKind::Directory(base) => match file_path.strip_prefix(base.as_path()) {
                Ok(relative) => Some(relative.to_string_lossy().replace('\\', "/")),
                Err(_) => {
                    eprintln!(
                        "[markdown-view] 相対パス算出失敗: {} はベース {} の配下ではありません",
                        file_path.display(),
                        base.as_path().display()
                    );
                    None
                }
            },
            AppModeKind::SingleFile(_) => None,
        }
    }
}

/// サーバー共有状態
pub struct AppState {
    mode: AppMode,
    dark_mode: bool,
    theme: Option<String>,
    tx: broadcast::Sender<BroadcastMessage>,
}

impl AppState {
    /// `AppState` を生成する
    pub fn new(
        mode: AppMode,
        dark_mode: bool,
        theme: Option<String>,
        tx: broadcast::Sender<BroadcastMessage>,
    ) -> Self {
        Self {
            mode,
            dark_mode,
            theme,
            tx,
        }
    }

    /// 動作モードを返す
    pub fn mode(&self) -> &AppMode {
        &self.mode
    }

    /// ダークモード設定を返す
    pub fn dark_mode(&self) -> bool {
        self.dark_mode
    }

    /// テーマ名を返す
    pub fn theme(&self) -> Option<&str> {
        self.theme.as_deref()
    }

    /// broadcast送信チャネルを返す
    pub fn tx(&self) -> &broadcast::Sender<BroadcastMessage> {
        &self.tx
    }
}

/// WebSocket broadcastメッセージ
#[derive(Debug, Clone)]
pub enum BroadcastMessage {
    /// コンテンツ更新
    Update(UpdateMessage),
    /// クライアントに再取得を促す
    Refresh,
    /// エラー通知
    Error(String),
}

impl BroadcastMessage {
    fn to_json(&self) -> Result<String, serde_json::Error> {
        match self {
            BroadcastMessage::Update(update) => serde_json::to_string(update),
            BroadcastMessage::Refresh => serde_json::to_string(&serde_json::json!({
                "refresh": true
            })),
            BroadcastMessage::Error(message) => {
                serde_json::to_string(&UpdateMessage::error(message))
            }
        }
    }
}

/// ファイルサイズ上限（10MB）: OOM防止
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// axumルーターを構築する
pub fn create_router(state: Arc<AppState>) -> Router {
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
        // CSP: script-src/style-srcはインラインテンプレート埋め込みのため'unsafe-inline'を許可。
        // img-srcは外部画像参照のため*を許可。
        // sanitize_hrefはリンクのhref属性とimg要素のsrc属性の両方に適用される（renderer.rs参照）。
        // img srcのdata:スキームはCSP img-src側で制御する。
        // frame-ancestors 'none'でクリックジャッキングを防止。
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src *; connect-src 'self' ws: wss:; object-src 'none'; frame-ancestors 'none'",
            ),
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
) -> Result<Html<String>, StatusCode> {
    if !is_allowed_request_host(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let (file_path, file_list) = resolve_target_file(&state, query.file.as_deref(), true)?;

    let update = read_and_render_file(&file_path, state.theme.as_deref())
        .await
        .map_err(|e| {
            eprintln!("[markdown-view] index読み込みエラー: {}", e);
            e.status_code()
        })?;

    let title = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("markdown-view");

    let current_file = state.mode.relative_path_of(&file_path);

    Ok(Html(render_page(
        title,
        &update.content,
        &update.toc,
        state.dark_mode,
        file_list.as_deref(),
        current_file.as_deref(),
    )))
}

/// GET /api/content : 現在のコンテンツをJSON形式で返す
async fn api_content_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<FileQuery>,
) -> Result<Json<UpdateMessage>, StatusCode> {
    if !is_allowed_request_host(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let (file_path, _) = resolve_target_file(&state, query.file.as_deref(), false)?;

    let mut update = read_and_render_file(&file_path, state.theme.as_deref())
        .await
        .map_err(|e| {
            eprintln!("[markdown-view] api/content読み込みエラー: {}", e);
            e.status_code()
        })?;

    update.file = state.mode.relative_path_of(&file_path);
    Ok(Json(update))
}

/// GET /api/files : ディレクトリ内の.mdファイル一覧をJSON形式で返す
async fn api_files_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, StatusCode> {
    if !is_allowed_request_host(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    if let Some(base) = state.mode.directory() {
        let files = list_markdown_files(base).map_err(|e| {
            eprintln!("[markdown-view] ファイル一覧取得エラー: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        Ok(Json(files))
    } else {
        Ok(Json(vec![]))
    }
}

/// モードとクエリパラメータからターゲットファイルを解決する
///
/// ディレクトリモード: クエリ指定があればresolve_file、なければデフォルトファイル
/// 単一ファイルモード: クエリ無視でファイルを返す
fn resolve_target_file(
    state: &AppState,
    query_file: Option<&str>,
    include_file_list: bool,
) -> Result<(PathBuf, Option<Vec<String>>), StatusCode> {
    if let Some(path) = state.mode.single_file() {
        Ok((path.to_path_buf(), None))
    } else if let Some(base) = state.mode.directory() {
        let mut precomputed_files: Option<Vec<String>> = None;
        let file_path = if let Some(rel) = query_file {
            resolve_file(base, rel).map_err(|e| {
                eprintln!("[markdown-view] ファイル解決エラー: {}", e);
                e.status_code()
            })?
        } else {
            let files = list_markdown_files(base).map_err(|e| {
                eprintln!("[markdown-view] ファイル一覧取得エラー: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            precomputed_files = Some(files.clone());
            // デフォルト: README.mdがあればそれ、なければアルファベット順最初
            let default_file = files
                .iter()
                .find(|f| f.eq_ignore_ascii_case("readme.md"))
                .or_else(|| files.first());

            match default_file {
                Some(rel) => resolve_file(base, rel).map_err(|e| {
                    eprintln!("[markdown-view] デフォルトファイル解決エラー: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?,
                None => {
                    return Err(StatusCode::NOT_FOUND);
                }
            }
        };

        let file_list = if include_file_list {
            match precomputed_files {
                Some(files) => Some(files),
                None => Some(list_markdown_files(base).map_err(|e| {
                    eprintln!("[markdown-view] ファイル一覧取得エラー: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?),
            }
        } else {
            None
        };

        Ok((file_path, file_list))
    } else {
        unreachable!("AppModeは単一ファイルまたはディレクトリのいずれか")
    }
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

    // 単一ファイルモードのみ接続直後に初期コンテンツを送信
    // ディレクトリモードではクライアントが?fileパラメータで/api/contentをフェッチする
    if let Some(file_path) = state.mode.single_file() {
        let update = match read_and_render_file(file_path, state.theme.as_deref()).await {
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
        let msg = match BroadcastMessage::Update(update).to_json() {
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
                        let json = match msg.to_json() {
                            Ok(json) => json,
                            Err(e) => {
                                eprintln!("[markdown-view] WebSocketメッセージJSON化エラー: {}", e);
                                continue;
                            }
                        };
                        if let Err(e) = socket.send(Message::Text(json.into())).await {
                            eprintln!("[markdown-view] WebSocket送信エラー: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // 遅延クライアントに最新コンテンツを再送信（単一ファイルモードのみ）
                        eprintln!(
                            "[markdown-view] WebSocketクライアントが{}メッセージ遅延",
                            n
                        );
                        if let Some(file_path) = state.mode.single_file() {
                            // 単一ファイルモード: 最新コンテンツを再送信
                            let update = match read_and_render_file(file_path, state.theme.as_deref()).await {
                                Ok(result) => result,
                                Err(e) => {
                                    eprintln!("[markdown-view] WebSocket再送信読み込みエラー: {}", e);
                                    if let Ok(error_json) = BroadcastMessage::Error(format!("ファイル読み込みエラー: {}", e)).to_json() {
                                        if let Err(e) = socket.send(Message::Text(error_json.into())).await {
                                            eprintln!("[markdown-view] WebSocketエラーJSON送信失敗: {}", e);
                                            break;
                                        }
                                    } else {
                                        eprintln!(
                                            "[markdown-view] WebSocketエラーJSON生成にも失敗 (元エラー: {})",
                                            e
                                        );
                                    }
                                    continue;
                                }
                            };
                            let resend = match BroadcastMessage::Update(update).to_json() {
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
                        } else {
                            // ディレクトリモード: クライアントにリフレッシュを促す
                            match BroadcastMessage::Refresh.to_json() {
                                Ok(json) => {
                                    if let Err(e) = socket.send(Message::Text(json.into())).await {
                                        eprintln!("[markdown-view] WebSocket再送信エラー: {}", e);
                                        break;
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[markdown-view] リフレッシュJSONシリアライズエラー: {}", e);
                                }
                            }
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

impl ReadMarkdownError {
    /// HTTPステータスコードへ変換する
    fn status_code(&self) -> StatusCode {
        match self {
            ReadMarkdownError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ReadMarkdownError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        }
    }
}

impl IntoResponse for ReadMarkdownError {
    fn into_response(self) -> axum::response::Response {
        self.status_code().into_response()
    }
}

/// ファイル一覧の最大件数
const MAX_FILE_LIST: usize = 1000;

/// ディレクトリ走査の最大深度（スタックオーバーフロー防止）
const MAX_DIR_DEPTH: usize = 32;

/// ディレクトリ内の.mdファイルを再帰的に列挙する
///
/// - 隠しファイル/ディレクトリ（`.`開始）を除外
/// - シンボリックリンクのサイクルを検出してスキップ
/// - 最大`MAX_FILE_LIST`件まで
/// - ベースディレクトリからの相対パスで返す（アルファベット順ソート）
pub fn list_markdown_files(base_dir: &Path) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();
    let mut visited_dirs = std::collections::HashSet::new();
    // ベースディレクトリ自体を訪問済みに登録（サイクル検出の起点）
    match base_dir.canonicalize() {
        Ok(canonical_base) => {
            visited_dirs.insert(canonical_base);
        }
        Err(e) => {
            eprintln!(
                "[markdown-view] ベースディレクトリの正規化に失敗（サイクル検出が不完全になる可能性あり）: {} ({})",
                base_dir.display(),
                e
            );
        }
    }
    list_markdown_files_recursive(base_dir, base_dir, &mut files, &mut visited_dirs, 0)?;
    files.sort();
    files.truncate(MAX_FILE_LIST);
    Ok(files)
}

fn list_markdown_files_recursive(
    base_dir: &Path,
    current_dir: &Path,
    files: &mut Vec<String>,
    visited_dirs: &mut std::collections::HashSet<PathBuf>,
    depth: usize,
) -> std::io::Result<()> {
    if depth >= MAX_DIR_DEPTH {
        eprintln!(
            "[markdown-view] ディレクトリ深度上限に到達（スキップ）: {}",
            current_dir.display()
        );
        return Ok(());
    }
    let entries = std::fs::read_dir(current_dir)?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!(
                    "[markdown-view] ディレクトリエントリ読み取りエラー（スキップ）: {} ({})",
                    current_dir.display(),
                    e
                );
                continue;
            }
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // 隠しファイル/ディレクトリを除外
        if name_str.starts_with('.') {
            continue;
        }

        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                eprintln!(
                    "[markdown-view] ファイルタイプ取得エラー（スキップ）: {} ({})",
                    path.display(),
                    e
                );
                continue;
            }
        };

        if file_type.is_dir() || (file_type.is_symlink() && path.is_dir()) {
            // 上限チェック（再帰前に打ち切り）
            if files.len() >= MAX_FILE_LIST {
                return Ok(());
            }
            // シンボリックリンクディレクトリの場合、解決先がベースディレクトリ内か確認
            if file_type.is_symlink() {
                match path.canonicalize() {
                    Ok(resolved) => {
                        let canonical_base = match base_dir.canonicalize() {
                            Ok(cb) => cb,
                            Err(e) => {
                                eprintln!(
                                    "[markdown-view] ベースディレクトリの正規化に失敗（スキップ）: {} ({})",
                                    base_dir.display(),
                                    e
                                );
                                continue;
                            }
                        };
                        if !resolved.starts_with(&canonical_base) {
                            eprintln!(
                                "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
                                path.display(),
                                resolved.display()
                            );
                            continue;
                        }
                        // サイクル検出: 既に訪問済みのディレクトリはスキップ
                        if !visited_dirs.insert(resolved) {
                            eprintln!(
                                "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                                path.display()
                            );
                            continue;
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[markdown-view] シンボリックリンクの正規化に失敗（スキップ）: {} ({})",
                            path.display(),
                            e
                        );
                        continue;
                    }
                }
            } else {
                // 通常ディレクトリもサイクル検出対象に登録
                match path.canonicalize() {
                    Ok(canonical) => {
                        if !visited_dirs.insert(canonical) {
                            continue;
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[markdown-view] ディレクトリ正規化に失敗（サイクル検出なしで続行）: {} ({})",
                            path.display(),
                            e
                        );
                    }
                }
            }
            list_markdown_files_recursive(base_dir, &path, files, visited_dirs, depth + 1)?;
        } else if file_type.is_file() {
            if let Some(ext) = path.extension() {
                if ext.eq_ignore_ascii_case("md") {
                    match path.strip_prefix(base_dir) {
                        Ok(relative) => {
                            // パス区切り文字を/に統一
                            let relative_str = relative
                                .components()
                                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                                .collect::<Vec<_>>()
                                .join("/");
                            files.push(relative_str);
                            if files.len() >= MAX_FILE_LIST {
                                return Ok(());
                            }
                        }
                        Err(_) => {
                            eprintln!(
                                "[markdown-view] 相対パス算出不可（スキップ）: {} (ベース: {})",
                                path.display(),
                                base_dir.display()
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// 相対パスを安全に解決する（ディレクトリトラバーサル防止）
///
/// - 空パス、絶対パス、NULバイト含有を拒否
/// - 隠しファイル/ディレクトリ（`.`開始のパスコンポーネント）を拒否
/// - canonicalize + starts_with でベースディレクトリ外アクセスを防止
/// - .md拡張子のファイルのみ許可
/// - 戻り値はcanonicalize済みの絶対パス
pub fn resolve_file(base_dir: &Path, relative: &str) -> Result<PathBuf, ResolveFileError> {
    if relative.is_empty() {
        return Err(ResolveFileError::EmptyPath);
    }

    // NULバイトチェック
    if relative.contains('\0') {
        return Err(ResolveFileError::InvalidPath);
    }

    // 絶対パス拒否
    let rel_path = Path::new(relative);
    if rel_path.is_absolute() {
        return Err(ResolveFileError::InvalidPath);
    }

    let candidate = base_dir.join(rel_path);
    let canonical = candidate.canonicalize().map_err(|e| {
        eprintln!(
            "[markdown-view] ファイルパス正規化失敗: {} ({})",
            candidate.display(),
            e
        );
        ResolveFileError::NotFound
    })?;

    // ベースディレクトリ外へのアクセス防止
    let canonical_base = base_dir.canonicalize().map_err(|e| {
        eprintln!(
            "[markdown-view] ベースディレクトリ正規化失敗: {} ({})",
            base_dir.display(),
            e
        );
        ResolveFileError::NotFound
    })?;
    if !canonical.starts_with(&canonical_base) {
        return Err(ResolveFileError::Traversal);
    }

    // 隠しファイル/ディレクトリの拒否（canonicalize後の相対パスで判定）
    // /api/files や watcher から除外されるファイルへの直接アクセスを防止
    if let Ok(resolved_relative) = canonical.strip_prefix(&canonical_base) {
        if resolved_relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        {
            return Err(ResolveFileError::Hidden);
        }
    }

    // ファイル存在チェック
    if !canonical.is_file() {
        return Err(ResolveFileError::NotFound);
    }

    // .md拡張子チェック
    match canonical.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("md") => {}
        _ => return Err(ResolveFileError::NotMarkdown),
    }

    Ok(canonical)
}

/// resolve_file のエラー型
#[derive(Debug, PartialEq)]
pub enum ResolveFileError {
    /// 空パス
    EmptyPath,
    /// 無効なパス（絶対パス、NULバイト等）
    InvalidPath,
    /// ファイルが存在しない
    NotFound,
    /// ディレクトリトラバーサル検出
    Traversal,
    /// .md以外の拡張子
    NotMarkdown,
    /// 隠しファイル/ディレクトリ（`.`開始のパスコンポーネント）
    Hidden,
}

impl std::fmt::Display for ResolveFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveFileError::EmptyPath => write!(f, "ファイルパスが空です"),
            ResolveFileError::InvalidPath => write!(f, "無効なパスです"),
            ResolveFileError::NotFound => write!(f, "ファイルが見つかりません"),
            ResolveFileError::Traversal => {
                write!(f, "ディレクトリ外へのアクセスは禁止されています")
            }
            ResolveFileError::NotMarkdown => write!(f, ".mdファイルのみアクセス可能です"),
            ResolveFileError::Hidden => {
                write!(f, "隠しファイルへのアクセスは禁止されています")
            }
        }
    }
}

impl ResolveFileError {
    /// HTTPステータスコードへ変換する
    pub fn status_code(&self) -> StatusCode {
        match self {
            ResolveFileError::Traversal
            | ResolveFileError::NotMarkdown
            | ResolveFileError::Hidden => StatusCode::FORBIDDEN,
            ResolveFileError::EmptyPath
            | ResolveFileError::InvalidPath
            | ResolveFileError::NotFound => StatusCode::NOT_FOUND,
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
async fn read_and_render_file(
    file_path: &Path,
    theme: Option<&str>,
) -> Result<UpdateMessage, ReadMarkdownError> {
    let markdown = read_markdown_with_limit(file_path).await?;
    Ok(UpdateMessage::new(
        render_markdown(&markdown, theme),
        generate_toc(&markdown),
        None,
    ))
}

/// ファイル変更時にbroadcastで全クライアントに通知する
///
/// `changed_file`: 変更されたファイルの絶対パス（canonicalize済み）
/// ディレクトリモードでは相対パス算出に失敗した場合、ブロードキャストをスキップする
/// （fileフィールドなしで送信すると全クライアントのコンテンツが上書きされるため）。
/// 読み込みエラー時はエラーJSONをクライアントに送信する。
/// JS側の `data.error` チェックでコンソールにエラーログが出力される（UI表示はなし）。
pub async fn notify_update(state: &AppState, changed_file: &Path) {
    if state.tx.receiver_count() == 0 {
        return;
    }

    let relative_path = state.mode.relative_path_of(changed_file);

    // ディレクトリモードで相対パスが算出できない場合はブロードキャストをスキップ
    // fileフィールドなしで送信すると全クライアントのコンテンツが上書きされるため
    if state.mode.is_directory() && relative_path.is_none() {
        eprintln!(
            "[markdown-view] 相対パス算出失敗のためブロードキャストをスキップ: {}",
            changed_file.display()
        );
        return;
    }

    let msg = match read_and_render_file(changed_file, state.theme.as_deref()).await {
        Ok(mut update) => {
            update.file = relative_path;
            BroadcastMessage::Update(update)
        }
        Err(e) => {
            eprintln!("[markdown-view] 更新時読み込みエラー: {}", e);
            BroadcastMessage::Error(format!("ファイル読み込みエラー: {}", e))
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

    // --- resolve_file テスト ---

    fn create_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# README").unwrap();
        std::fs::write(dir.path().join("guide.md"), "# Guide").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "text file").unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("docs/api.md"), "# API").unwrap();
        std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join(".hidden/secret.md"), "# Secret").unwrap();
        std::fs::write(dir.path().join(".dotfile.md"), "# Dot").unwrap();
        dir
    }

    #[test]
    fn test_resolve_file_正常なパス() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "README.md");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("README.md"));
    }

    #[test]
    fn test_resolve_file_サブディレクトリのパス() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "docs/api.md");
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_file_トラバーサル拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "../../../etc/passwd");
        assert!(matches!(
            result,
            Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
        ));
    }

    #[test]
    fn test_resolve_file_絶対パス拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "/etc/passwd");
        assert_eq!(result, Err(ResolveFileError::InvalidPath));
    }

    #[test]
    fn test_resolve_file_存在しないファイル() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "nonexistent.md");
        assert_eq!(result, Err(ResolveFileError::NotFound));
    }

    #[test]
    fn test_resolve_file_非md拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "notes.txt");
        assert_eq!(result, Err(ResolveFileError::NotMarkdown));
    }

    #[test]
    fn test_resolve_file_隠しファイル拒否() {
        let dir = create_test_dir();
        // 隠しディレクトリ内のファイル
        let result = resolve_file(dir.path(), ".hidden/secret.md");
        assert_eq!(result, Err(ResolveFileError::Hidden));
    }

    #[test]
    fn test_resolve_file_隠しドットファイル拒否() {
        let dir = create_test_dir();
        // ドットで始まるファイル
        let result = resolve_file(dir.path(), ".dotfile.md");
        assert_eq!(result, Err(ResolveFileError::Hidden));
    }

    #[test]
    fn test_resolve_file_nulバイト拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "README\0.md");
        assert_eq!(result, Err(ResolveFileError::InvalidPath));
    }

    #[test]
    fn test_resolve_file_ディレクトリパス拒否() {
        let dir = create_test_dir();
        // docsディレクトリは存在するが、ファイルではないのでNotFound
        let result = resolve_file(dir.path(), "docs");
        assert_eq!(result, Err(ResolveFileError::NotFound));
    }

    #[test]
    fn test_resolve_file_空パス拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "");
        assert_eq!(result, Err(ResolveFileError::EmptyPath));
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_file_シンボリックリンクによるトラバーサル拒否() {
        let dir = create_test_dir();
        // ベースディレクトリ外を指すシンボリックリンクを作成
        let outside_dir = tempfile::tempdir().unwrap();
        std::fs::write(outside_dir.path().join("secret.md"), "# Secret").unwrap();

        std::os::unix::fs::symlink(
            outside_dir.path().join("secret.md"),
            dir.path().join("link.md"),
        )
        .unwrap();

        let result = resolve_file(dir.path(), "link.md");
        // canonicalizeでシンボリックリンクが解決され、ベースディレクトリ外を指すためTraversal
        assert_eq!(result, Err(ResolveFileError::Traversal));
    }

    // --- list_markdown_files テスト ---

    #[test]
    fn test_list_markdown_files_基本動作() {
        let dir = create_test_dir();
        let files = list_markdown_files(dir.path()).unwrap();
        assert!(files.contains(&"README.md".to_string()));
        assert!(files.contains(&"guide.md".to_string()));
        assert!(files.contains(&"docs/api.md".to_string()));
    }

    #[test]
    fn test_list_markdown_files_非md除外() {
        let dir = create_test_dir();
        let files = list_markdown_files(dir.path()).unwrap();
        assert!(!files.iter().any(|f| f.ends_with(".txt")));
    }

    #[test]
    fn test_list_markdown_files_隠しファイル除外() {
        let dir = create_test_dir();
        let files = list_markdown_files(dir.path()).unwrap();
        assert!(!files.iter().any(|f| f.contains(".hidden")));
        assert!(!files.iter().any(|f| f.starts_with('.')));
    }

    #[test]
    fn test_list_markdown_files_空ディレクトリ() {
        let dir = tempfile::tempdir().unwrap();
        let files = list_markdown_files(dir.path()).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_list_markdown_files_ソート済み() {
        let dir = create_test_dir();
        let files = list_markdown_files(dir.path()).unwrap();
        let mut sorted = files.clone();
        sorted.sort();
        assert_eq!(files, sorted);
    }

    #[cfg(unix)]
    #[test]
    fn test_list_markdown_files_シンボリックリンクサイクルでハングしない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# README").unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/doc.md"), "# Doc").unwrap();

        // サイクルを作成: sub/loop -> ベースディレクトリ自体
        std::os::unix::fs::symlink(dir.path(), dir.path().join("sub/loop")).unwrap();

        let files = list_markdown_files(dir.path()).unwrap();
        // 無限再帰せず正常に返ること
        assert!(files.contains(&"README.md".to_string()));
        assert!(files.contains(&"sub/doc.md".to_string()));
        // サイクル経由の重複エントリがないこと
        assert!(
            !files.iter().any(|f| f.contains("loop/")),
            "サイクル経由のエントリが含まれてはいけない: {:?}",
            files
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_list_markdown_files_自己参照シンボリックリンクでハングしない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# README").unwrap();

        // 自己参照サイクル: loop -> .
        std::os::unix::fs::symlink(".", dir.path().join("loop")).unwrap();

        let files = list_markdown_files(dir.path()).unwrap();
        assert!(files.contains(&"README.md".to_string()));
        assert!(
            !files.iter().any(|f| f.contains("loop/")),
            "サイクル経由のエントリが含まれてはいけない: {:?}",
            files
        );
    }

    // --- AppMode テスト ---

    #[test]
    fn test_app_mode_new_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.md");
        std::fs::write(&file_path, "# test").unwrap();

        let mode = AppMode::new_single_file(&file_path).unwrap();
        let canonical = file_path.canonicalize().unwrap();

        assert_eq!(mode.base_dir(), canonical.parent().unwrap());
        assert_eq!(mode.single_file(), Some(canonical.as_path()));
        assert!(mode.directory().is_none());
    }

    #[test]
    fn test_app_mode_new_directory() {
        let dir = tempfile::tempdir().unwrap();

        let mode = AppMode::new_directory(dir.path()).unwrap();
        let canonical = dir.path().canonicalize().unwrap();

        assert_eq!(mode.base_dir(), canonical.as_path());
        assert!(mode.single_file().is_none());
        assert_eq!(mode.directory(), Some(canonical.as_path()));
    }

    #[test]
    fn test_app_mode_relative_path_of_ディレクトリモード() {
        let dir = create_test_dir();
        let canonical = dir.path().canonicalize().unwrap();
        let mode = AppMode::new_directory(dir.path()).unwrap();
        let file_path = canonical.join("docs/api.md");
        assert_eq!(
            mode.relative_path_of(&file_path),
            Some("docs/api.md".to_string())
        );
    }

    #[test]
    fn test_app_mode_relative_path_of_単一ファイルモードはnone() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.md");
        std::fs::write(&file_path, "# test").unwrap();

        let mode = AppMode::new_single_file(&file_path).unwrap();
        let canonical = file_path.canonicalize().unwrap();
        assert_eq!(mode.relative_path_of(&canonical), None);
    }

    #[test]
    fn test_app_mode_new_directory_存在しないパスは拒否() {
        let result = AppMode::new_directory("/nonexistent/path/that/does/not/exist");
        assert!(matches!(result, Err(AppModeBuildError::CanonicalPath(_))));
    }

    #[test]
    fn test_app_mode_new_single_file_非mdは拒否() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "test").unwrap();

        let result = AppMode::new_single_file(&file_path);
        assert!(matches!(result, Err(AppModeBuildError::NotMarkdown(_))));
    }
}
