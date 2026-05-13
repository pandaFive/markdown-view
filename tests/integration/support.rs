use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt};

use futures_util::{stream::SplitStream, StreamExt};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use markdown_view::server::{AppMode, AppState};

pub(super) type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
pub(super) type WsReadHalf = SplitStream<WsStream>;
pub(super) fn build_single_file_state(file_path: &Path) -> Arc<AppState> {
    let (tx, _rx) = broadcast::channel(16);
    Arc::new(AppState::new_with_tokio_memo_fs(
        AppMode::new_single_file(file_path).unwrap(),
        false,
        None,
        tx,
    ))
}
pub(super) fn build_dir_state(base_dir: &Path) -> Arc<AppState> {
    let (tx, _rx) = broadcast::channel(16);
    Arc::new(AppState::new_with_tokio_memo_fs(
        AppMode::new_directory(base_dir).unwrap(),
        false,
        None,
        tx,
    ))
}
pub(super) async fn spawn_test_server(state: Arc<AppState>) -> std::net::SocketAddr {
    let router = markdown_view::server::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    addr
}
pub(super) async fn setup_single_file_server_from_path(
    file_path: &Path,
) -> (Arc<AppState>, std::net::SocketAddr) {
    let state = build_single_file_state(file_path);
    let addr = spawn_test_server(state.clone()).await;
    (state, addr)
}
pub(super) fn atomic_save_markdown_file(path: &Path, new_content: &str) {
    let swp_path = path.with_extension("md.swp");
    let backup_path = path.with_extension("md~");

    std::fs::write(&swp_path, new_content).unwrap();
    std::fs::rename(path, &backup_path).unwrap();
    std::fs::rename(&swp_path, path).unwrap();
    std::fs::remove_file(&backup_path).unwrap();
}
pub(super) async fn setup_single_file_server_with_bytes(
    file_name: &str,
    content: &[u8],
) -> (
    Arc<AppState>,
    std::net::SocketAddr,
    tempfile::TempDir,
    std::path::PathBuf,
) {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join(file_name);
    tokio::fs::write(&file_path, content).await.unwrap();
    let (state, addr) = setup_single_file_server_from_path(&file_path).await;
    (state, addr, tmp_dir, file_path)
}
pub(super) async fn assert_json_error_for_paths(
    addr: std::net::SocketAddr,
    paths: &[&str],
    expected_status: reqwest::StatusCode,
    expected_message: Option<&str>,
) {
    for path in paths {
        let resp = reqwest::get(format!("http://{}{}", addr, path))
            .await
            .unwrap();
        assert_eq!(resp.status(), expected_status);
        let json: serde_json::Value = resp.json().await.unwrap();
        match expected_message {
            Some(message) => assert_eq!(json["error"].as_str().unwrap(), message),
            None => assert!(json["error"].as_str().is_some()),
        }
    }
}
#[cfg(unix)]
pub(super) struct FilePermissionGuard {
    path: std::path::PathBuf,
    original_mode: u32,
}
#[cfg(unix)]
impl Drop for FilePermissionGuard {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.original_mode));
    }
}
#[cfg(unix)]
pub(super) fn make_file_unreadable(path: &Path) -> Option<FilePermissionGuard> {
    let original_mode = fs::metadata(path).unwrap().permissions().mode();
    let guard = FilePermissionGuard {
        path: path.to_path_buf(),
        original_mode,
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();

    if fs::File::open(path).is_ok() {
        eprintln!("chmod 0o000 後も対象ファイルを読めるため、IOエラー統合テストをskipします");
        drop(guard);
        None
    } else {
        Some(guard)
    }
}
pub(super) async fn setup_single_file_server(
    markdown_content: &str,
) -> (Arc<AppState>, std::net::SocketAddr, tempfile::TempDir) {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file_path = tmp_dir.path().join("test.md");
    tokio::fs::write(&file_path, markdown_content)
        .await
        .unwrap();

    let (state, addr) = setup_single_file_server_from_path(&file_path).await;

    (state, addr, tmp_dir)
}
pub(super) async fn setup_dir_server() -> (Arc<AppState>, std::net::SocketAddr, tempfile::TempDir) {
    let tmp_dir = tempfile::tempdir().unwrap();

    // ファイル構造を作成
    tokio::fs::write(
        tmp_dir.path().join("README.md"),
        "# README\n\nThis is readme.",
    )
    .await
    .unwrap();
    tokio::fs::write(tmp_dir.path().join("notes.txt"), "not markdown")
        .await
        .unwrap();
    tokio::fs::create_dir_all(tmp_dir.path().join("docs"))
        .await
        .unwrap();
    tokio::fs::write(
        tmp_dir.path().join("docs/guide.md"),
        "# Guide\n\nThis is guide.",
    )
    .await
    .unwrap();
    tokio::fs::create_dir_all(tmp_dir.path().join(".hidden"))
        .await
        .unwrap();
    tokio::fs::write(tmp_dir.path().join(".hidden/secret.md"), "# Secret")
        .await
        .unwrap();

    let state = build_dir_state(tmp_dir.path());
    let addr = spawn_test_server(state.clone()).await;

    (state, addr, tmp_dir)
}
pub(super) async fn connect_ws(
    url: &str,
    origin: &str,
) -> Result<
    (
        WsStream,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    tokio_tungstenite::tungstenite::Error,
> {
    connect_ws_with_host(url, origin, None).await
}
pub(super) async fn connect_ws_with_host(
    url: &str,
    origin: &str,
    host: Option<&str>,
) -> Result<
    (
        WsStream,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    tokio_tungstenite::tungstenite::Error,
> {
    let mut request = url.into_client_request()?;
    request
        .headers_mut()
        .insert("Origin", origin.parse().expect("Originヘッダは妥当な値"));
    if let Some(host) = host {
        request
            .headers_mut()
            .insert("Host", host.parse().expect("Hostヘッダは妥当な値"));
    }
    tokio_tungstenite::connect_async(request).await
}
pub(super) async fn next_ws_message(
    read: &mut WsReadHalf,
) -> tokio_tungstenite::tungstenite::Message {
    tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .expect("WebSocketメッセージ受信がタイムアウト")
        .expect("WebSocketストリームが予期せず終了")
        .expect("WebSocketメッセージの読み取りに失敗")
}
pub(super) async fn assert_close_frame_message(
    read: &mut WsReadHalf,
    expected_code: u16,
    expected_reason: &str,
) {
    let msg = next_ws_message(read).await;
    match msg {
        tokio_tungstenite::tungstenite::Message::Close(Some(frame)) => {
            assert_eq!(u16::from(frame.code), expected_code);
            let reason: &str = frame.reason.as_ref();
            assert_eq!(reason, expected_reason);
        }
        other => panic!("Close frameを期待したが {:?} を受信", other),
    }
}
