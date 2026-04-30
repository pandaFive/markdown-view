use super::files::{
    load_route_memo, load_route_update, resolve_route_target, save_route_memo, ResolvedTarget,
    RouteTargetRequest,
};
use super::messages::{ApiError, BroadcastMessage};
use super::state::AppState;
use crate::template::{MemoResponse, MemoUpdateMessage, UpdateMessage};

pub(super) struct PageRequest<'a> {
    pub file: Option<&'a str>,
}

pub(super) struct ContentRequest<'a> {
    pub file: Option<&'a str>,
}

pub(super) struct MemoRequest<'a> {
    pub file: Option<&'a str>,
}

pub(super) struct SaveMemoRequest<'a> {
    pub file: Option<&'a str>,
    pub raw: String,
}

pub(super) struct PageView {
    pub title: String,
    pub update: UpdateMessage,
    pub memo: MemoResponse,
    pub sidebar: SidebarView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SidebarView {
    SingleFile,
    Directory {
        directory_name: String,
        file_list: Vec<String>,
        current_file: Option<String>,
    },
}

impl SidebarView {
    fn single_file() -> Self {
        Self::SingleFile
    }

    fn directory(
        directory_name: impl Into<String>,
        file_list: Vec<String>,
        current_file: Option<&str>,
    ) -> Self {
        Self::Directory {
            directory_name: directory_name.into(),
            file_list,
            current_file: current_file.map(ToOwned::to_owned),
        }
    }
}

fn sidebar_directory_name(state: &AppState) -> &str {
    state
        .mode()
        .directory()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Documents")
}

fn title_for_path(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("markdown-view")
        .to_string()
}

pub(super) async fn load_page(
    state: &AppState,
    request: PageRequest<'_>,
) -> Result<PageView, ApiError> {
    let route_request = RouteTargetRequest::page(request.file);
    let target = resolve_route_target(state, route_request)?;
    let update = load_route_update(&target, route_request).await?;
    let memo =
        match load_route_memo(state, &target, RouteTargetRequest::api_memo(request.file)).await {
            Ok(memo) => memo,
            Err(error) => {
                tracing::warn!(
                "[markdown-view] index描画ではメモ読み込み失敗を空メモへフォールバック ({}): {:?}",
                target.file_label(),
                error
            );
                MemoResponse::empty(target.relative_path().map(ToOwned::to_owned))
            }
        };
    let sidebar = match target.file_list() {
        Some(files) => SidebarView::directory(
            sidebar_directory_name(state),
            files.to_vec(),
            target.relative_path(),
        ),
        None => SidebarView::single_file(),
    };

    Ok(PageView {
        title: title_for_path(target.file_path()),
        update,
        memo,
        sidebar,
    })
}

pub(super) async fn load_content(
    state: &AppState,
    request: ContentRequest<'_>,
) -> Result<UpdateMessage, ApiError> {
    let route_request = RouteTargetRequest::api_content(request.file);
    let target = resolve_route_target(state, route_request)?;
    load_route_update(&target, route_request).await
}

pub(super) async fn load_memo(
    state: &AppState,
    request: MemoRequest<'_>,
) -> Result<MemoResponse, ApiError> {
    let route_request = RouteTargetRequest::api_memo(request.file);
    let target = resolve_route_target(state, route_request)?;
    load_route_memo(state, &target, route_request).await
}

pub(super) async fn save_memo(
    state: &AppState,
    request: SaveMemoRequest<'_>,
) -> Result<MemoResponse, ApiError> {
    let route_request = RouteTargetRequest::api_memo(request.file);
    let target = resolve_route_target(state, route_request)?;
    let memo = save_route_memo(state, &target, request.raw, route_request).await?;
    broadcast_saved_memo(state, memo_message_file(&target));
    Ok(memo)
}

fn broadcast_saved_memo(state: &AppState, file: String) {
    if state.tx().receiver_count() == 0 {
        return;
    }

    let _ = state
        .tx()
        .send(BroadcastMessage::MemoUpdate(MemoUpdateMessage::new(file)));
}

fn memo_message_file(target: &ResolvedTarget) -> String {
    target
        .relative_path()
        .map(ToOwned::to_owned)
        .or_else(|| {
            target
                .file_path()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| target.file_path().display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    use crate::server::messages::BroadcastMessage;
    use crate::server::state::{AppMode, AppState};

    fn create_directory_state(base_dir: &std::path::Path) -> AppState {
        let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
        AppState::new(AppMode::new_directory(base_dir).unwrap(), false, None, tx)
    }

    fn create_single_file_state(file_path: &std::path::Path) -> AppState {
        let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
        AppState::new(
            AppMode::new_single_file(file_path).unwrap(),
            false,
            None,
            tx,
        )
    }

    #[test]
    fn test_sidebar_view_directoryは所有データを保持する() {
        let sidebar = SidebarView::directory(
            "docs",
            vec!["README.md".to_string(), "guide/setup.md".to_string()],
            Some("guide/setup.md"),
        );

        assert_eq!(
            sidebar,
            SidebarView::Directory {
                directory_name: "docs".to_string(),
                file_list: vec!["README.md".to_string(), "guide/setup.md".to_string()],
                current_file: Some("guide/setup.md".to_string()),
            }
        );
    }

    #[test]
    fn test_sidebar_view_single_fileを作れる() {
        assert_eq!(SidebarView::single_file(), SidebarView::SingleFile);
    }

    #[tokio::test]
    async fn test_load_page_ディレクトリ表示情報を組み立てる() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("guide")).unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
        std::fs::write(dir.path().join("guide/setup.md"), "# Setup").unwrap();
        let state = create_directory_state(dir.path());

        let page = load_page(
            &state,
            PageRequest {
                file: Some("guide/setup.md"),
            },
        )
        .await
        .unwrap();

        assert_eq!(page.title, "setup.md");
        assert!(page.update.content().as_str().contains("Setup"));
        assert_eq!(page.memo.file(), Some("guide/setup.md"));
        assert_eq!(
            page.sidebar,
            SidebarView::Directory {
                directory_name: dir
                    .path()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                file_list: vec!["README.md".to_string(), "guide/setup.md".to_string()],
                current_file: Some("guide/setup.md".to_string()),
            }
        );
    }

    #[tokio::test]
    async fn test_load_page_単一ファイル表示情報を組み立てる() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("note.md");
        std::fs::write(&file_path, "# Note").unwrap();
        let state = create_single_file_state(&file_path);

        let page = load_page(&state, PageRequest { file: None }).await.unwrap();

        assert_eq!(page.title, "note.md");
        assert!(page.update.content().as_str().contains("Note"));
        assert_eq!(page.memo.file(), None);
        assert_eq!(page.sidebar, SidebarView::SingleFile);
    }

    #[tokio::test]
    async fn test_load_page_壊れたメモは空メモへフォールバックする() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
        std::fs::write(dir.path().join(".README.md.memo.md"), [0xff, 0xfe, 0xfd]).unwrap();
        let state = create_directory_state(dir.path());

        let page = load_page(&state, PageRequest { file: None }).await.unwrap();

        assert_eq!(page.memo.raw(), "");
        assert_eq!(page.memo.file(), Some("README.md"));
    }

    #[tokio::test]
    async fn test_load_content_指定ファイルのupdateを返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
        std::fs::write(dir.path().join("target.md"), "# Target").unwrap();
        let state = create_directory_state(dir.path());

        let update = load_content(
            &state,
            ContentRequest {
                file: Some("target.md"),
            },
        )
        .await
        .unwrap();

        assert!(update.content().as_str().contains("Target"));
        assert_eq!(update.file(), Some("target.md"));
    }

    #[tokio::test]
    async fn test_load_memo_指定ファイルのmemoを返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
        std::fs::write(dir.path().join("target.md"), "# Target").unwrap();
        std::fs::write(dir.path().join(".target.md.memo.md"), "memo body").unwrap();
        let state = create_directory_state(dir.path());

        let memo = load_memo(
            &state,
            MemoRequest {
                file: Some("target.md"),
            },
        )
        .await
        .unwrap();

        assert_eq!(memo.raw(), "memo body");
        assert_eq!(memo.file(), Some("target.md"));
    }

    #[tokio::test]
    async fn test_save_memo_成功時だけmemo_updateをbroadcastする() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
        let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
        let state = AppState::new(AppMode::new_directory(dir.path()).unwrap(), false, None, tx);
        let mut rx = state.tx().subscribe();

        let memo = save_memo(
            &state,
            SaveMemoRequest {
                file: Some("README.md"),
                raw: "saved memo".to_string(),
            },
        )
        .await
        .unwrap();

        assert_eq!(memo.raw(), "saved memo");
        let message = rx
            .try_recv()
            .expect("保存成功時はmemo_updateが送信されるべき");
        match message {
            BroadcastMessage::MemoUpdate(update) => assert_eq!(update.file(), "README.md"),
            other => panic!("unexpected broadcast: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_save_memo_失敗時はbroadcastしない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
        let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
        let state = AppState::new(AppMode::new_directory(dir.path()).unwrap(), false, None, tx);
        let mut rx = state.tx().subscribe();

        let error = save_memo(
            &state,
            SaveMemoRequest {
                file: Some("../README.md"),
                raw: "blocked".to_string(),
            },
        )
        .await
        .expect_err("不正パスは保存失敗になるべき");

        assert_eq!(error.0, axum::http::StatusCode::NOT_FOUND);
        assert!(rx.try_recv().is_err());
    }
}
