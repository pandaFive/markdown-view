use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use tokio::sync::broadcast;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::renderer::syntax_theme_css;
use crate::template::{
    error_message_json, render_page, RenderPageParams, SidebarParams, UpdateMessage,
};

mod files;
mod guards;
mod websocket;

#[cfg(test)]
use self::files::{
    canonicalize_dir_for_cycle, read_bytes_with_limit, revalidate_single_file_target,
    ReadMarkdownError,
};
pub use self::files::{list_markdown_files, resolve_file, ResolveFileError};
use self::files::{
    read_rendered_update_or_error, resolve_target_file_or_error, TargetResolveContext,
};
use self::guards::{
    build_csp_header, ensure_allowed_request_host, is_allowed_request_host, is_allowed_ws_origin,
    json_error,
};
#[cfg(test)]
use self::guards::{is_trusted_host, normalize_authority};
use self::websocket::handle_socket;
#[cfg(test)]
use self::websocket::lagged_recovery_message;
pub use self::websocket::notify_update;
#[cfg(test)]
use axum::http::header::{HOST, ORIGIN};

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

impl AsRef<Path> for CanonicalPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
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
                    tracing::warn!(
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
    syntax_css: String,
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
            syntax_css: syntax_theme_css(theme.as_deref()),
            mode,
            dark_mode,
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

    /// 構文ハイライト用CSSを返す
    pub fn syntax_css(&self) -> &str {
        &self.syntax_css
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
            BroadcastMessage::Error(message) => serde_json::to_string(&error_message_json(message)),
        }
    }
}

/// ファイルサイズ上限: OOM防止
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
const FILE_SIZE_LIMIT_MB: u64 = MAX_FILE_SIZE / 1024 / 1024;

fn file_size_limit_error_message() -> String {
    format!(
        "ファイルサイズが上限（{}MB）を超えています",
        FILE_SIZE_LIMIT_MB
    )
}

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

type ApiError = (StatusCode, Json<serde_json::Value>);

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

    let current_file = state.mode.relative_path_of(&file_path);

    Ok(Html(render_page(RenderPageParams {
        title,
        content: update.content(),
        toc: update.toc(),
        dark_mode: state.dark_mode,
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
        update.with_file(state.mode.relative_path_of(&file_path)),
    ))
}

/// GET /api/files : ディレクトリ内の.mdファイル一覧をJSON形式で返す
async fn api_files_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, ApiError> {
    ensure_allowed_request_host(&headers)?;

    if let Some(base) = state.mode.directory() {
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

/// ファイル一覧の最大件数
const MAX_FILE_LIST: usize = 1000;

/// ディレクトリ走査の最大深度（スタックオーバーフロー防止）
const MAX_DIR_DEPTH: usize = 32;

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

    #[test]
    fn test_broadcast_message_refreshのjson直列化() {
        let json = BroadcastMessage::Refresh.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value, serde_json::json!({ "refresh": true }));
    }

    #[test]
    fn test_broadcast_message_errorのjson直列化() {
        let json = BroadcastMessage::Error("watcher error".to_string())
            .to_json()
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value, serde_json::json!({ "error": "watcher error" }));
    }

    // --- ReadMarkdownError::close_code テスト ---

    #[test]
    fn test_close_code_ioエラーは1011を返す() {
        let err = ReadMarkdownError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, ""));
        assert_eq!(err.close_code(), 1011);
    }

    #[test]
    fn test_close_code_too_largeは1009を返す() {
        let err = ReadMarkdownError::TooLarge;
        assert_eq!(err.close_code(), 1009);
    }

    #[test]
    fn test_close_code_not_utf8は1003を返す() {
        let err = ReadMarkdownError::NotUtf8;
        assert_eq!(err.close_code(), 1003);
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
    fn test_resolve_file_バックスラッシュ型トラバーサルを拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "..\\..\\..\\etc\\passwd");
        assert!(matches!(
            result,
            Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
        ));
    }

    #[test]
    fn test_resolve_file_urlエンコード型トラバーサルを拒否() {
        let dir = create_test_dir();
        let result = resolve_file(dir.path(), "docs/%2e%2e/%2e%2e/etc/passwd.md");
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

    #[test]
    fn test_list_markdown_files_最大1000件で打ち切る() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(MAX_FILE_LIST + 200) {
            let path = dir.path().join(format!("doc-{i:04}.md"));
            std::fs::write(path, "# x").unwrap();
        }

        let files = list_markdown_files(dir.path()).unwrap();
        assert_eq!(files.len(), MAX_FILE_LIST);
    }

    #[test]
    fn test_list_markdown_files_ベースディレクトリ正規化失敗はエラーを返す() {
        let missing = PathBuf::from("/path/that/does/not/exist");
        let result = list_markdown_files(&missing);
        assert!(result.is_err());
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

    #[test]
    fn test_list_markdown_files_深度上限を超えるパスは除外される() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("root.md"), "# root").unwrap();

        let mut current = dir.path().to_path_buf();
        for i in 0..=MAX_DIR_DEPTH {
            current = current.join(format!("d{}", i));
            std::fs::create_dir_all(&current).unwrap();
        }
        std::fs::write(current.join("deep.md"), "# deep").unwrap();

        let files = list_markdown_files(dir.path()).unwrap();
        assert!(files.contains(&"root.md".to_string()));
        assert!(!files.iter().any(|f| f.ends_with("deep.md")));
    }

    #[test]
    fn test_list_markdown_files_recursive_通常ディレクトリcanonicalize失敗時はスキップ扱い() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing-dir");
        assert!(canonicalize_dir_for_cycle(&missing, "通常ディレクトリ").is_none());
    }

    #[tokio::test]
    async fn test_read_bytes_with_limit_takeによる第2段階チェックで超過を検出する() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("large.md");
        tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
            .await
            .unwrap();

        let file = tokio::fs::File::open(&file_path).await.unwrap();
        let result = read_bytes_with_limit(file).await;
        assert!(matches!(result, Err(ReadMarkdownError::TooLarge)));
    }

    #[tokio::test]
    async fn test_notify_update_ディレクトリモードで相対パス算出失敗時は送信をスキップ() {
        let base_dir = tempfile::tempdir().unwrap();
        std::fs::write(base_dir.path().join("README.md"), "# README").unwrap();

        let outside_dir = tempfile::tempdir().unwrap();
        let outside_file = outside_dir.path().join("outside.md");
        std::fs::write(&outside_file, "# outside").unwrap();
        let outside_canonical = outside_file.canonicalize().unwrap();

        let state = create_directory_state(base_dir.path());
        let mut rx = state.tx().subscribe();

        notify_update(&state, &outside_canonical).await;
        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn test_notify_update_ディレクトリモードで読み込み失敗時はerrorを送信する() {
        let base_dir = tempfile::tempdir().unwrap();
        let target = base_dir.path().join("README.md");
        std::fs::write(&target, "# before").unwrap();

        let state = create_directory_state(base_dir.path());
        let mut rx = state.tx().subscribe();

        std::fs::remove_file(&target).unwrap();
        notify_update(&state, &target).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(message.contains("ファイル読み込みエラー"));
                assert!(
                    message.contains("README.md"),
                    "エラーメッセージにファイル名が含まれるべき: {}",
                    message
                );
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_ファイル名不明時はdisplay表示がエラーに含まれる() {
        // 単一ファイルモードでrelative_pathはNone
        // Pathが "/" の場合、file_name()もNoneを返すためdisplay()フォールバックが使われる
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("dummy.md");
        std::fs::write(&file_path, "# dummy").unwrap();

        let state = create_single_file_state(&file_path);
        let mut rx = state.tx().subscribe();

        // ルートパス "/" はfile_name()がNoneを返す
        notify_update(&state, std::path::Path::new("/")).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(
                    message.contains("/"),
                    "ファイル名不明時はdisplay()表示が含まれるべき: {}",
                    message
                );
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで読み込み失敗時はファイル名を含むエラーを送信する(
    ) {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("test.md", "# test");
        let mut rx = state.tx().subscribe();

        std::fs::remove_file(&file_path).unwrap();
        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(
                    message.contains("ファイル検証エラー"),
                    "revalidate失敗時は検証エラーを返すべき: {}",
                    message
                );
                assert!(
                    message.contains("test.md"),
                    "エラーメッセージにファイル名が含まれるべき: {}",
                    message
                );
                assert!(
                    !message.contains("No such file"),
                    "ユーザー向けメッセージにOS内部エラーが含まれるべきではない: {}",
                    message
                );
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードでサイズ超過時はtoo_largeエラーを送信する() {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("large.md", "# large");
        let mut rx = state.tx().subscribe();

        // set_lenでメタデータ上のサイズのみ変更し、metadata().len()による事前チェックで上限超過を検出させる
        let f = std::fs::File::options()
            .write(true)
            .open(&file_path)
            .unwrap();
        f.set_len(MAX_FILE_SIZE + 1).unwrap();

        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(
                    message.contains("ファイルサイズが上限"),
                    "エラーメッセージにサイズ超過メッセージが含まれるべき: {}",
                    message
                );
                assert!(
                    message.contains("large.md"),
                    "エラーメッセージにファイル名が含まれるべき: {}",
                    message
                );
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで非utf8ファイルはnot_utf8エラーを送信する() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("binary.md", "# valid");
        let mut rx = state.tx().subscribe();

        // 非UTF-8バイト列で上書き
        std::fs::write(&file_path, b"\xff\xfe\x80\x81").unwrap();
        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Error(message) => {
                assert!(
                    message.contains("UTF-8"),
                    "エラーメッセージにUTF-8が含まれるべき: {}",
                    message
                );
                assert!(
                    message.contains("binary.md"),
                    "エラーメッセージにファイル名が含まれるべき: {}",
                    message
                );
            }
            other => panic!("Errorメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで受信者ゼロ時は送信をスキップする() {
        let (_dir, file_path, state) = create_single_file_state_with_fixture("test.md", "# test");
        let _rx = state.tx().subscribe();
        // _rxをドロップして受信者ゼロにする
        drop(_rx);

        // ファイルを削除しておく（受信者ゼロで早期リターンされるため、読み込みは実行されないはず）
        std::fs::remove_file(&file_path).unwrap();
        notify_update(&state, &file_path).await;

        // 事後にsubscribeしてもメッセージは届かない
        let mut rx = state.tx().subscribe();
        assert!(
            matches!(rx.try_recv(), Err(broadcast::error::TryRecvError::Empty)),
            "受信者ゼロ時はメッセージが送信されないべき"
        );
    }

    #[tokio::test]
    async fn test_notify_update_単一ファイルモードで正常更新時はupdateを送信する() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("hello.md", "# hello world");
        let mut rx = state.tx().subscribe();

        notify_update(&state, &file_path).await;

        let received = rx.recv().await.unwrap();
        match received {
            BroadcastMessage::Update(update) => {
                assert!(
                    update.content().as_str().contains("hello world"),
                    "コンテンツに'hello world'が含まれるべき: {}",
                    update.content().as_str()
                );
                assert!(
                    update.toc().as_str().contains("hello-world"),
                    "TOCに見出しリンクが含まれるべき: {}",
                    update.toc().as_str()
                );
                assert!(
                    update.file().is_none(),
                    "単一ファイルモードではfileはNoneであるべき: {:?}",
                    update.file()
                );
            }
            other => panic!("Updateメッセージを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_lagged_recovery_message_単一ファイルモードは再読み込みしたupdateを返す() {
        let (_dir, _file_path, state) = create_single_file_state_with_fixture("test.md", "# title");

        let msg = lagged_recovery_message(&state).await;
        match msg {
            BroadcastMessage::Update(update) => {
                assert!(update.content().as_str().contains("title"));
            }
            other => panic!("Updateを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_lagged_recovery_message_ディレクトリモードはrefreshを返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# title").unwrap();
        let state = create_directory_state(dir.path());

        let msg = lagged_recovery_message(&state).await;
        assert!(matches!(msg, BroadcastMessage::Refresh));
    }

    #[tokio::test]
    async fn test_lagged_recovery_message_単一ファイル読み込み失敗時はerrorを返す() {
        let (_dir, file_path, state) =
            create_single_file_state_with_fixture("missing.md", "# title");

        std::fs::remove_file(&file_path).unwrap();
        let msg = lagged_recovery_message(&state).await;
        match msg {
            BroadcastMessage::Error(message) => {
                assert!(
                    message.contains("ファイル検証エラー"),
                    "revalidate失敗時は検証エラーを返すべき: {}",
                    message
                );
            }
            other => panic!("Errorを期待したが {:?} を受信", other),
        }
    }

    #[tokio::test]
    async fn test_read_markdown_error_into_response_too_largeのjson形式() {
        let response = ReadMarkdownError::TooLarge.into_response();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "error": format!("ファイルサイズが上限（{}MB）を超えています", FILE_SIZE_LIMIT_MB)
            })
        );
    }

    #[tokio::test]
    async fn test_read_markdown_error_into_response_ioのjson形式() {
        let io_error = std::io::Error::other("disk failure");
        let response = ReadMarkdownError::Io(io_error).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "error": "ファイルの読み込みに失敗しました"
            })
        );
    }

    #[tokio::test]
    async fn test_read_markdown_error_into_response_not_utf8のjson形式() {
        let response = ReadMarkdownError::NotUtf8.into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "error": "このファイルはUTF-8テキストではありません"
            })
        );
    }

    // --- AppMode テスト ---

    #[test]
    fn test_app_mode_new_single_file() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");

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
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");

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

    #[test]
    fn test_app_mode_new_single_file_ディレクトリ指定は拒否() {
        let dir = tempfile::tempdir().unwrap();
        let result = AppMode::new_single_file(dir.path());
        assert!(matches!(result, Err(AppModeBuildError::NotFile(_))));
    }

    #[test]
    fn test_app_mode_new_directory_ファイル指定は拒否() {
        let (_dir, file_path) = create_markdown_fixture("note.md", "# note");
        let result = AppMode::new_directory(&file_path);
        assert!(matches!(result, Err(AppModeBuildError::NotDirectory(_))));
    }

    #[test]
    fn test_revalidate_single_file_target_正常なファイルを許可する() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let canonical = file_path
            .canonicalize()
            .expect("テスト前提: canonicalizeが成功すること");
        let result = revalidate_single_file_target(&canonical);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), canonical);
    }

    #[test]
    fn test_revalidate_single_file_target_存在しないファイルはnotfoundを返す() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("nonexistent.md");
        let result = revalidate_single_file_target(&file_path);
        assert_eq!(result, Err(ResolveFileError::NotFound));
    }

    #[test]
    fn test_revalidate_single_file_target_ディレクトリはnotfoundを返す() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir
            .path()
            .canonicalize()
            .expect("テスト前提: canonicalizeが成功すること");
        let result = revalidate_single_file_target(&canonical);
        assert_eq!(result, Err(ResolveFileError::NotFound));
    }

    #[test]
    fn test_revalidate_single_file_target_非mdファイルはnotmarkdownを返す() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap();
        let canonical = file_path
            .canonicalize()
            .expect("テスト前提: canonicalizeが成功すること");
        let result = revalidate_single_file_target(&canonical);
        assert_eq!(result, Err(ResolveFileError::NotMarkdown));
    }

    #[cfg(unix)]
    #[test]
    fn test_revalidate_single_file_target_シンボリックリンクはtraversalを返す() {
        let dir = tempfile::tempdir().unwrap();
        let real_file = dir.path().join("real.md");
        std::fs::write(&real_file, "# real").unwrap();
        let link_path = dir.path().join("link.md");
        std::os::unix::fs::symlink(&real_file, &link_path).unwrap();
        let result = revalidate_single_file_target(&link_path);
        assert_eq!(result, Err(ResolveFileError::Traversal));
    }

    fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    fn create_single_file_state(file_path: &Path) -> AppState {
        let (tx, _rx) = broadcast::channel(16);
        AppState::new(
            AppMode::new_single_file(file_path).unwrap(),
            false,
            None,
            tx,
        )
    }

    fn create_directory_state(dir_path: &Path) -> AppState {
        let (tx, _rx) = broadcast::channel(16);
        AppState::new(AppMode::new_directory(dir_path).unwrap(), false, None, tx)
    }

    fn create_single_file_state_with_fixture(
        name: &str,
        content: &str,
    ) -> (tempfile::TempDir, PathBuf, AppState) {
        let (dir, file_path) = create_markdown_fixture(name, content);
        let state = create_single_file_state(&file_path);
        (dir, file_path, state)
    }
}
