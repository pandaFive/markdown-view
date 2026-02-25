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

/// アプリケーション動作モード
#[derive(Debug, Clone)]
pub enum AppMode {
    /// 単一ファイルモード
    SingleFile(PathBuf),
    /// ディレクトリモード
    Directory(PathBuf),
}

impl AppMode {
    /// ベースディレクトリを返す（ファイルモードは親、ディレクトリモードはそのまま）
    pub fn base_dir(&self) -> &Path {
        match self {
            AppMode::SingleFile(p) => p.parent().unwrap_or(p),
            AppMode::Directory(p) => p,
        }
    }

    /// 単一ファイルモードのパスを返す（ディレクトリモードはNone）
    pub fn single_file(&self) -> Option<&Path> {
        match self {
            AppMode::SingleFile(p) => Some(p),
            AppMode::Directory(_) => None,
        }
    }
}

/// サーバー共有状態
pub struct AppState {
    pub mode: AppMode,
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

    let (file_path, file_list) = resolve_target_file(&state, query.file.as_deref())?;

    let (content, toc) = read_and_render_file(&file_path, state.theme.as_deref())
        .await
        .map_err(|e| {
            eprintln!("[markdown-view] index読み込みエラー: {}", e);
            match e {
                ReadMarkdownError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                ReadMarkdownError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            }
        })?;

    let title = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("markdown-view");

    // 現在のファイルの相対パスを計算（ディレクトリモード用）
    let current_file = match &state.mode {
        AppMode::Directory(base) => base.canonicalize().ok().and_then(|canonical_base| {
            file_path
                .strip_prefix(&canonical_base)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        }),
        AppMode::SingleFile(_) => None,
    };

    Ok(Html(render_page(
        title,
        &content,
        &toc,
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

    let (file_path, _) = resolve_target_file(&state, query.file.as_deref())?;

    let (content, toc) = read_and_render_file(&file_path, state.theme.as_deref())
        .await
        .map_err(|e| {
            eprintln!("[markdown-view] api/content読み込みエラー: {}", e);
            match e {
                ReadMarkdownError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                ReadMarkdownError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            }
        })?;

    // 現在のファイルの相対パスを計算
    let file = match &state.mode {
        AppMode::Directory(base) => base.canonicalize().ok().and_then(|canonical_base| {
            file_path
                .strip_prefix(&canonical_base)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        }),
        AppMode::SingleFile(_) => None,
    };

    Ok(Json(UpdateMessage { content, toc, file }))
}

/// GET /api/files : ディレクトリ内の.mdファイル一覧をJSON形式で返す
async fn api_files_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, StatusCode> {
    if !is_allowed_request_host(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    match &state.mode {
        AppMode::Directory(base) => {
            let files = list_markdown_files(base).map_err(|e| {
                eprintln!("[markdown-view] ファイル一覧取得エラー: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            Ok(Json(files))
        }
        AppMode::SingleFile(_) => Ok(Json(vec![])),
    }
}

/// モードとクエリパラメータからターゲットファイルを解決する
///
/// ディレクトリモード: クエリ指定があればresolve_file、なければデフォルトファイル
/// 単一ファイルモード: クエリ無視でファイルを返す
fn resolve_target_file(
    state: &AppState,
    query_file: Option<&str>,
) -> Result<(PathBuf, Option<Vec<String>>), StatusCode> {
    match &state.mode {
        AppMode::SingleFile(path) => Ok((path.clone(), None)),
        AppMode::Directory(base) => {
            let files = list_markdown_files(base).map_err(|e| {
                eprintln!("[markdown-view] ファイル一覧取得エラー: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            let file_path = if let Some(rel) = query_file {
                resolve_file(base, rel).map_err(|e| {
                    eprintln!("[markdown-view] ファイル解決エラー: {}", e);
                    match e {
                        ResolveFileError::Traversal => StatusCode::FORBIDDEN,
                        ResolveFileError::NotMarkdown => StatusCode::FORBIDDEN,
                        _ => StatusCode::NOT_FOUND,
                    }
                })?
            } else {
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

            Ok((file_path, Some(files)))
        }
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
        let (content, toc) = match read_and_render_file(file_path, state.theme.as_deref()).await {
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
        let msg = match serde_json::to_string(&UpdateMessage {
            content,
            toc,
            file: None,
        }) {
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
                        if let Err(e) = socket.send(Message::Text(msg.into())).await {
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
                            let (content, toc) = match read_and_render_file(file_path, state.theme.as_deref()).await {
                                Ok(result) => result,
                                Err(e) => {
                                    eprintln!("[markdown-view] WebSocket再送信読み込みエラー: {}", e);
                                    if let Ok(error_json) = serde_json::to_string(&serde_json::json!({
                                        "error": format!("ファイル読み込みエラー: {}", e)
                                    })) {
                                        let _ = socket.send(Message::Text(error_json.into())).await;
                                    }
                                    continue;
                                }
                            };
                            let resend = match serde_json::to_string(&UpdateMessage { content, toc, file: None }) {
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
                        // ディレクトリモードではクライアント側でリフェッチする
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

/// ファイル一覧の最大件数
const MAX_FILE_LIST: usize = 1000;

/// ディレクトリ内の.mdファイルを再帰的に列挙する
///
/// - 隠しファイル/ディレクトリ（`.`開始）を除外
/// - 最大`MAX_FILE_LIST`件まで
/// - ベースディレクトリからの相対パスで返す
pub fn list_markdown_files(base_dir: &Path) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();
    list_markdown_files_recursive(base_dir, base_dir, &mut files)?;
    files.sort();
    files.truncate(MAX_FILE_LIST);
    Ok(files)
}

fn list_markdown_files_recursive(
    base_dir: &Path,
    current_dir: &Path,
    files: &mut Vec<String>,
) -> std::io::Result<()> {
    let entries = std::fs::read_dir(current_dir)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // 隠しファイル/ディレクトリを除外
        if name_str.starts_with('.') {
            continue;
        }

        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            // 上限チェック（再帰前に打ち切り）
            if files.len() >= MAX_FILE_LIST {
                return Ok(());
            }
            list_markdown_files_recursive(base_dir, &path, files)?;
        } else if file_type.is_file() {
            if let Some(ext) = path.extension() {
                if ext.eq_ignore_ascii_case("md") {
                    if let Ok(relative) = path.strip_prefix(base_dir) {
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
                }
            }
        }
    }
    Ok(())
}

/// 相対パスを安全に解決する（ディレクトリトラバーサル防止）
///
/// - 空パス、絶対パス、NULバイト含有を拒否
/// - canonicalize + starts_with でベースディレクトリ外アクセスを防止
/// - .md拡張子のファイルのみ許可
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
    let canonical = candidate
        .canonicalize()
        .map_err(|_| ResolveFileError::NotFound)?;

    // ベースディレクトリ外へのアクセス防止
    let canonical_base = base_dir
        .canonicalize()
        .map_err(|_| ResolveFileError::NotFound)?;
    if !canonical.starts_with(&canonical_base) {
        return Err(ResolveFileError::Traversal);
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
) -> Result<(String, String), ReadMarkdownError> {
    let markdown = read_markdown_with_limit(file_path).await?;
    let content = render_markdown(&markdown, theme);
    let toc = generate_toc(&markdown);
    Ok((content, toc))
}

/// ファイル変更時にbroadcastで全クライアントに通知する
///
/// `changed_file`: 変更されたファイルの絶対パス
/// 読み込みエラー時はエラーJSONをクライアントに送信する。
/// JS側の `data.error` チェックでエラー表示される。
pub async fn notify_update(state: &AppState, changed_file: &Path) {
    // 変更ファイルの相対パスを計算（ディレクトリモード用）
    let relative_path = match &state.mode {
        AppMode::Directory(base) => base.canonicalize().ok().and_then(|canonical_base| {
            changed_file
                .strip_prefix(&canonical_base)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        }),
        AppMode::SingleFile(_) => None,
    };

    let msg = match read_and_render_file(changed_file, state.theme.as_deref()).await {
        Ok((content, toc)) => {
            match serde_json::to_string(&UpdateMessage {
                content,
                toc,
                file: relative_path,
            }) {
                Ok(json) => json,
                Err(e) => {
                    eprintln!("[markdown-view] JSONシリアライズエラー: {}", e);
                    return;
                }
            }
        }
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
    fn test_resolve_file_nulバイト拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "README\0.md");
        assert_eq!(result, Err(ResolveFileError::InvalidPath));
    }

    #[test]
    fn test_resolve_file_空パス拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "");
        assert_eq!(result, Err(ResolveFileError::EmptyPath));
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

    // --- AppMode テスト ---

    #[test]
    fn test_app_mode_single_file() {
        let mode = AppMode::SingleFile(PathBuf::from("/tmp/test.md"));
        assert_eq!(mode.base_dir(), Path::new("/tmp"));
        assert_eq!(mode.single_file(), Some(Path::new("/tmp/test.md")));
    }

    #[test]
    fn test_app_mode_directory() {
        let mode = AppMode::Directory(PathBuf::from("/tmp/docs"));
        assert_eq!(mode.base_dir(), Path::new("/tmp/docs"));
        assert!(mode.single_file().is_none());
    }
}
