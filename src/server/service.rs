use axum::http::StatusCode;

use super::files::{
    list_markdown_files_from_canonical_base, load_route_memo, load_route_update,
    normalize_search_query, resolve_route_target, run_blocking_file_task, save_route_memo,
    search_directory, ResolvedTarget, RouteTargetRequest, SearchCancellation, SearchResponse,
    MAX_FILE_LIST,
};
use super::guards::json_error;
use super::messages::{ApiError, BroadcastMessage};
use super::state::{AppState, SearchConcurrencyLimitError, SearchGenerationLimitError};
use crate::template::{MemoResponse, MemoUpdateMessage, UpdateMessage};

const SIDEBAR_DIRECTORY_FALLBACK_NAME: &str = "ドキュメント";

/// indexページ表示に必要な対象ファイル指定。
pub(super) struct PageRequest<'a> {
    pub file: Option<&'a str>,
}

/// `/api/content` の対象ファイル指定。
pub(super) struct ContentRequest<'a> {
    pub file: Option<&'a str>,
}

/// `/api/memo` 読み込みの対象ファイル指定。
pub(super) struct MemoRequest<'a> {
    pub file: Option<&'a str>,
}

/// `/api/memo` 保存の対象ファイルと本文。
pub(super) struct SaveMemoRequest<'a> {
    pub file: Option<&'a str>,
    pub raw: String,
}

/// indexページ描画に渡す本文・メモ・サイドバーの集約結果。
pub(super) struct PageView {
    pub title: String,
    pub update: UpdateMessage,
    pub memo: MemoResponse,
    pub sidebar: SidebarView,
}

/// サイドバー表示モード。単一ファイルではファイル一覧を持たない。
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
        .unwrap_or(SIDEBAR_DIRECTORY_FALLBACK_NAME)
}

fn title_for_path(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("markdown-view")
        .to_string()
}

/// indexページ向けに本文とメモを読み込む。
///
/// 本文の読み込み失敗はページ表示不能として `ApiError` を返す。一方でメモ読み込み失敗は
/// 本文閲覧を継続するため `MemoResponse::empty_with_load_error` に変換し、編集を無効化する。
pub(super) async fn load_page(
    state: &AppState,
    request: PageRequest<'_>,
) -> Result<PageView, ApiError> {
    let route_request = RouteTargetRequest::page(request.file);
    let target = resolve_route_target(state, route_request).await?;
    let update = load_route_update(&target, route_request).await?;
    let memo = match load_route_memo(state, &target, RouteTargetRequest::api_memo(request.file))
        .await
    {
        Ok(memo) => memo,
        Err(error) => {
            tracing::warn!(
                "[markdown-view] index描画ではメモ読み込み失敗を空メモへフォールバック ({}): {:?}",
                target.file_label(),
                error
            );
            MemoResponse::empty_with_load_error(
                    target.relative_path().map(ToOwned::to_owned),
                    "メモを読み込めませんでした。内容を保護するため編集を無効化しています。本文の閲覧は継続できます。",
                )
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

/// `/api/content` 向けに本文更新ペイロードを読み込む。
pub(super) async fn load_content(
    state: &AppState,
    request: ContentRequest<'_>,
) -> Result<UpdateMessage, ApiError> {
    let route_request = RouteTargetRequest::api_content(request.file);
    let target = resolve_route_target(state, route_request).await?;
    load_route_update(&target, route_request).await
}

/// `/api/memo` GET 向けにメモを読み込む。
///
/// `load_page` と違い、APIでは呼び出し側に失敗を明示するためメモ読み込み失敗をそのまま
/// `ApiError` として返す。
pub(super) async fn load_memo(
    state: &AppState,
    request: MemoRequest<'_>,
) -> Result<MemoResponse, ApiError> {
    let route_request = RouteTargetRequest::api_memo(request.file);
    let target = resolve_route_target(state, route_request).await?;
    load_route_memo(state, &target, route_request).await
}

/// `/api/memo` PUT 向けにメモを保存する。
///
/// 保存成功後だけ `memo_update` をbroadcastし、他タブへ再読み込みを促す。保存失敗時は
/// broadcastしないことで、未保存または拒否された内容を他クライアントへ通知しない。
pub(super) async fn save_memo(
    state: &AppState,
    request: SaveMemoRequest<'_>,
) -> Result<MemoResponse, ApiError> {
    let route_request = RouteTargetRequest::api_memo(request.file);
    let target = resolve_route_target(state, route_request).await?;
    let memo = save_route_memo(state, &target, request.raw, route_request).await?;
    broadcast_saved_memo(state, memo_message_file(&target));
    Ok(memo)
}

/// ディレクトリモードのMarkdownファイル一覧を返す。単一ファイルモードでは空配列を返す。
pub(super) async fn list_files(state: &AppState) -> Result<Vec<String>, ApiError> {
    if let Some(base) = state.mode().directory_canonical().cloned() {
        run_blocking_file_task("ファイル一覧取得", move || {
            list_markdown_files_from_canonical_base(&base, MAX_FILE_LIST)
        })
        .await
        .map_err(|_| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ファイル一覧の取得に失敗しました",
            )
        })?
        .map_err(|error| {
            tracing::warn!("[markdown-view] ファイル一覧取得エラー: {}", error);
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ファイル一覧の取得に失敗しました",
            )
        })
    } else {
        Ok(Vec::new())
    }
}

fn map_search_error(error: std::io::Error) -> ApiError {
    if error.kind() == std::io::ErrorKind::InvalidInput {
        return json_error(StatusCode::BAD_REQUEST, "検索クエリが長すぎます");
    }

    tracing::warn!("[markdown-view] ディレクトリ検索エラー: {}", error);
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "ディレクトリ検索に失敗しました",
    )
}

fn map_search_generation_limit_error(_error: SearchGenerationLimitError) -> ApiError {
    json_error(StatusCode::TOO_MANY_REQUESTS, "検索が混み合っています")
}

fn map_search_concurrency_limit_error(_error: SearchConcurrencyLimitError) -> ApiError {
    json_error(StatusCode::TOO_MANY_REQUESTS, "検索が混み合っています")
}

/// ディレクトリモードの全文検索を実行する。単一ファイルモードでは空結果を返す。
pub(super) async fn search(
    state: &AppState,
    query: String,
    client_id: Option<&str>,
    client_sequence: Option<u64>,
) -> Result<SearchResponse, ApiError> {
    let query = normalize_search_query(&query).map_err(map_search_error)?;
    let Some(base_dir) = state.mode().directory_canonical() else {
        return Ok(SearchResponse::empty(query));
    };

    let cancellation = client_id
        .map(|client_id| {
            state
                .begin_search_generation(client_id, client_sequence)
                .map(|generation| {
                    if generation.is_stale() {
                        None
                    } else {
                        Some(SearchCancellation::new(generation))
                    }
                })
        })
        .transpose()
        .map_err(map_search_generation_limit_error)?
        .flatten();

    if query.is_empty() || cancellation.is_none() && client_id.is_some() {
        return Ok(SearchResponse::empty(query));
    }

    let permit = state
        .try_acquire_directory_search_permit()
        .map_err(map_search_concurrency_limit_error)?;

    search_directory(
        base_dir,
        &query,
        cancellation.unwrap_or_else(SearchCancellation::none),
        Some(permit),
    )
    .await
    .map_err(map_search_error)
}

fn broadcast_saved_memo(state: &AppState, file: String) {
    if state.tx().receiver_count() == 0 {
        return;
    }

    if let Err(error) = state
        .tx()
        .send(BroadcastMessage::MemoUpdate(MemoUpdateMessage::new(file)))
    {
        tracing::warn!(
            "[markdown-view] メモ保存通知の送信に失敗しました: {}",
            error
        );
    }
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
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, LazyLock,
    };
    use std::time::{Duration, Instant};

    use super::*;
    use tokio::sync::{broadcast, Mutex, MutexGuard};

    use crate::server::files::{
        set_catalog_progress_hook_for_test, set_search_progress_hook_for_test, MockMemoFs, Op,
        RouteTargetKind,
    };
    use crate::server::messages::BroadcastMessage;
    use crate::server::state::{AppMode, AppState};
    use crate::template::MemoState;

    fn create_directory_state(base_dir: &std::path::Path) -> AppState {
        let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
        AppState::new_with_tokio_memo_fs(AppMode::new_directory(base_dir).unwrap(), false, None, tx)
    }

    fn create_single_file_state(file_path: &std::path::Path) -> AppState {
        let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
        AppState::new_with_tokio_memo_fs(
            AppMode::new_single_file(file_path).unwrap(),
            false,
            None,
            tx,
        )
    }

    fn create_markdown_fixture(
        name: &str,
        content: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    static SEARCH_HOOK_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    async fn lock_search_hook_tests() -> MutexGuard<'static, ()> {
        SEARCH_HOOK_TEST_LOCK.lock().await
    }

    async fn wait_for_blocked_search_count(
        block_state: Arc<(std::sync::Mutex<(usize, bool)>, Condvar)>,
        expected: usize,
    ) {
        tokio::task::spawn_blocking(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let (lock, cvar) = &*block_state;
            let mut blocked = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            while blocked.0 < expected {
                let now = Instant::now();
                assert!(
                    now < deadline,
                    "blocked search count did not reach {expected}; actual={}",
                    blocked.0
                );
                let timeout = deadline.saturating_duration_since(now);
                let (next_blocked, result) = cvar
                    .wait_timeout(blocked, timeout)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                blocked = next_blocked;
                assert!(
                    !result.timed_out() || blocked.0 >= expected,
                    "blocked search count did not reach {expected}; actual={}",
                    blocked.0
                );
            }
        })
        .await
        .unwrap();
    }

    async fn wait_for_blocked_search(block_state: Arc<(std::sync::Mutex<(bool, bool)>, Condvar)>) {
        tokio::task::spawn_blocking(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let (lock, cvar) = &*block_state;
            let mut blocked = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            while !blocked.0 {
                let now = Instant::now();
                assert!(now < deadline, "search progress hook was not reached");
                let timeout = deadline.saturating_duration_since(now);
                let (next_blocked, result) = cvar
                    .wait_timeout(blocked, timeout)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                blocked = next_blocked;
                assert!(
                    !result.timed_out() || blocked.0,
                    "search progress hook was not reached"
                );
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_search_ディレクトリモードは検索世代を進める() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());

        let response = search(&state, "needle".to_string(), Some("client-a"), None)
            .await
            .unwrap();

        assert_eq!(state.current_search_generation("client-a"), 1);
        assert_eq!(response.results.len(), 1);
    }

    #[tokio::test]
    async fn test_search_ディレクトリモードの空queryでも検索世代を進める() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());

        let response = search(&state, "".to_string(), Some("client-a"), None)
            .await
            .unwrap();

        assert_eq!(state.current_search_generation("client-a"), 1);
        assert_eq!(response.query, "");
        assert!(response.results.is_empty());
        assert_eq!(response.searched_files, 0);
    }

    #[tokio::test]
    async fn test_search_単一ファイルモードは検索世代を進めない() {
        let (_dir, file_path) = create_markdown_fixture("note.md", "needle");
        let state = create_single_file_state(&file_path);

        let response = search(&state, "needle".to_string(), Some("client-a"), None)
            .await
            .unwrap();

        assert_eq!(state.current_search_generation("client-a"), 0);
        assert!(response.results.is_empty());
    }

    #[tokio::test]
    async fn test_search_client_idなしでは検索世代を進めない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());

        let response = search(&state, "needle".to_string(), None, None)
            .await
            .unwrap();

        assert_eq!(state.current_search_generation("client-a"), 0);
        assert_eq!(response.results.len(), 1);
    }

    #[tokio::test]
    async fn test_search_client_idなしでも全体同時実行上限到達時は429を返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());
        let _permits = (0..crate::server::state::MAX_CONCURRENT_DIRECTORY_SEARCHES)
            .map(|_| state.try_acquire_directory_search_permit().unwrap())
            .collect::<Vec<_>>();

        let error = search(&state, "needle".to_string(), None, None)
            .await
            .expect_err("client idなし検索も全体同時実行上限の対象にする");

        assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.1["error"].as_str(), Some("検索が混み合っています"));
    }

    #[tokio::test]
    async fn test_search_全体同時実行上限到達時もvalid_clientの検索世代を進める() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());
        let active = state.begin_search_generation("client-a", None).unwrap();
        let _permits = (0..crate::server::state::MAX_CONCURRENT_DIRECTORY_SEARCHES)
            .map(|_| state.try_acquire_directory_search_permit().unwrap())
            .collect::<Vec<_>>();

        let error = search(&state, "needle".to_string(), Some("client-a"), None)
            .await
            .expect_err("同時実行上限到達時は旧世代をstale化してから拒否する");

        assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.1["error"].as_str(), Some("検索が混み合っています"));
        assert!(active.is_stale());
        assert_eq!(state.current_search_generation("client-a"), 2);
    }

    #[tokio::test]
    async fn test_search_空queryは全体同時実行上限到達時もpermitなしで検索世代を進める() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());
        let active = state.begin_search_generation("client-a", Some(1)).unwrap();
        let _permits = (0..crate::server::state::MAX_CONCURRENT_DIRECTORY_SEARCHES)
            .map(|_| state.try_acquire_directory_search_permit().unwrap())
            .collect::<Vec<_>>();

        let response = search(&state, "".to_string(), Some("client-a"), Some(2))
            .await
            .unwrap();

        assert!(response.results.is_empty());
        assert!(active.is_stale());
        assert_eq!(state.current_search_generation("client-a"), 2);
    }

    #[tokio::test]
    async fn test_search_実行中searchが全体同時実行上限permitを保持する() {
        let _lock = lock_search_hook_tests().await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("permit-hold-target.md"), "needle").unwrap();
        let state = Arc::new(create_directory_state(dir.path()));
        let block_state = Arc::new((std::sync::Mutex::new((0_usize, false)), Condvar::new()));
        let _hook = set_search_progress_hook_for_test({
            let block_state = Arc::clone(&block_state);
            Arc::new(move |relative, searched_files| {
                if relative != "permit-hold-target.md" || searched_files != 1 {
                    return;
                }

                let (lock, cvar) = &*block_state;
                let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                state.0 += 1;
                cvar.notify_all();
                while !state.1 {
                    state = cvar
                        .wait(state)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            })
        });

        let handles = (0..crate::server::state::MAX_CONCURRENT_DIRECTORY_SEARCHES)
            .map(|index| {
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    search(
                        &state,
                        "needle".to_string(),
                        Some(&format!("client-{index}")),
                        None,
                    )
                    .await
                })
            })
            .collect::<Vec<_>>();

        wait_for_blocked_search_count(
            Arc::clone(&block_state),
            crate::server::state::MAX_CONCURRENT_DIRECTORY_SEARCHES,
        )
        .await;

        let error = search(&state, "needle".to_string(), Some("overflow-client"), None)
            .await
            .expect_err("実行中のsearchがpermitを保持している間は追加検索を拒否する");

        assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.1["error"].as_str(), Some("検索が混み合っています"));
        assert_eq!(state.current_search_generation("overflow-client"), 1);

        {
            let (lock, cvar) = &*block_state;
            let mut blocked = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            blocked.1 = true;
            cvar.notify_all();
        }

        for handle in handles {
            handle.await.unwrap().unwrap();
        }
    }

    #[tokio::test]
    async fn test_search_future中断後もblocking検索完了までpermitを保持する() {
        let _lock = lock_search_hook_tests().await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("abort-permit-target.md"), "needle").unwrap();
        let state = Arc::new(create_directory_state(dir.path()));
        let block_state = Arc::new((std::sync::Mutex::new((false, false)), Condvar::new()));
        let _hook = set_search_progress_hook_for_test({
            let block_state = Arc::clone(&block_state);
            Arc::new(move |relative, searched_files| {
                if relative != "abort-permit-target.md" || searched_files != 1 {
                    return;
                }

                let (lock, cvar) = &*block_state;
                let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                state.0 = true;
                cvar.notify_all();
                while !state.1 {
                    state = cvar
                        .wait(state)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            })
        });
        let handle = {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                search(&state, "needle".to_string(), Some("client-a"), None).await
            })
        };

        wait_for_blocked_search(Arc::clone(&block_state)).await;

        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());

        let _permits = (1..crate::server::state::MAX_CONCURRENT_DIRECTORY_SEARCHES)
            .map(|_| state.try_acquire_directory_search_permit().unwrap())
            .collect::<Vec<_>>();
        let overflow = state.try_acquire_directory_search_permit();
        assert!(overflow.is_err());

        let (lock, cvar) = &*block_state;
        let mut blocked = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        blocked.1 = true;
        cvar.notify_all();
    }

    #[tokio::test]
    async fn test_search_全体同時実行上限到達時の空queryで実行中の同一client検索をstale化する() {
        let _lock = lock_search_hook_tests().await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("empty-cancel-target.md"), "needle").unwrap();
        let state = Arc::new(create_directory_state(dir.path()));
        let block_state = Arc::new((std::sync::Mutex::new((false, false)), Condvar::new()));
        let _hook = set_search_progress_hook_for_test({
            let block_state = Arc::clone(&block_state);
            Arc::new(move |relative, searched_files| {
                if relative != "empty-cancel-target.md" || searched_files != 1 {
                    return;
                }

                let (lock, cvar) = &*block_state;
                let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                state.0 = true;
                cvar.notify_all();
                while !state.1 {
                    state = cvar
                        .wait(state)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            })
        });
        let handle = {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                search(&state, "needle".to_string(), Some("client-a"), Some(1)).await
            })
        };

        wait_for_blocked_search(Arc::clone(&block_state)).await;
        let _permits = (1..crate::server::state::MAX_CONCURRENT_DIRECTORY_SEARCHES)
            .map(|_| state.try_acquire_directory_search_permit().unwrap())
            .collect::<Vec<_>>();

        let cancel = search(&state, "".to_string(), Some("client-a"), Some(2))
            .await
            .unwrap();

        assert!(cancel.results.is_empty());
        assert_eq!(state.current_search_generation("client-a"), 2);

        {
            let (lock, cvar) = &*block_state;
            let mut blocked = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            blocked.1 = true;
            cvar.notify_all();
        }

        let stale = handle.await.unwrap().unwrap();
        assert!(stale.results.is_empty());
        assert_eq!(stale.searched_files, 1);
    }

    #[tokio::test]
    async fn test_search_全client_activeなら上限超過の新規clientを拒否する() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());
        let active_handles = (0..crate::server::state::MAX_SEARCH_GENERATION_CLIENTS)
            .map(|index| {
                state
                    .begin_search_generation(&format!("client-{index}"), None)
                    .unwrap()
            })
            .collect::<Vec<_>>();

        let error = search(&state, "needle".to_string(), Some("overflow-client"), None)
            .await
            .expect_err("全client activeの上限到達時は新規clientを拒否する");

        assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.1["error"].as_str(), Some("検索が混み合っています"));
        assert_eq!(state.current_search_generation("overflow-client"), 0);
        assert!(!active_handles[0].is_stale());
    }

    #[tokio::test]
    async fn test_search_同一clientの連続検索は同じ世代counterを進める() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());

        search(&state, "needle".to_string(), Some("client-a"), None)
            .await
            .unwrap();
        search(&state, "needle".to_string(), Some("client-a"), None)
            .await
            .unwrap();

        assert_eq!(state.current_search_generation("client-a"), 2);
    }

    #[tokio::test]
    async fn test_search_古いsequenceは検索世代を進めずstale応答にする() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());

        let current = search(&state, "needle".to_string(), Some("client-a"), Some(2))
            .await
            .unwrap();
        let stale = search(&state, "needle".to_string(), Some("client-a"), Some(1))
            .await
            .unwrap();

        assert_eq!(current.results.len(), 1);
        assert!(stale.results.is_empty());
        assert_eq!(state.current_search_generation("client-a"), 1);
    }

    #[tokio::test]
    async fn test_search_別clientの検索世代は独立する() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());

        search(&state, "needle".to_string(), Some("client-a"), None)
            .await
            .unwrap();
        search(&state, "needle".to_string(), Some("client-b"), None)
            .await
            .unwrap();

        assert_eq!(state.current_search_generation("client-a"), 1);
        assert_eq!(state.current_search_generation("client-b"), 1);
    }

    #[tokio::test]
    async fn test_search_同一clientの後続検索で先行検索が早期終了する() {
        let _lock = lock_search_hook_tests().await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("00-cancel-trigger.md"), "needle first").unwrap();
        std::fs::write(dir.path().join("01-next.md"), "needle second").unwrap();
        std::fs::write(dir.path().join("02-next.md"), "needle third").unwrap();
        let state = Arc::new(create_directory_state(dir.path()));
        let fired = Arc::new(AtomicBool::new(false));
        let _hook = set_search_progress_hook_for_test({
            let state = Arc::clone(&state);
            let fired = Arc::clone(&fired);
            Arc::new(move |relative, searched_files| {
                if relative == "00-cancel-trigger.md"
                    && searched_files == 1
                    && !fired.swap(true, Ordering::SeqCst)
                {
                    state.begin_search_generation("client-a", None).unwrap();
                }
            })
        });

        let response = search(&state, "needle".to_string(), Some("client-a"), None)
            .await
            .unwrap();

        assert!(fired.load(Ordering::SeqCst));
        assert_eq!(response.searched_files, 1);
        assert!(response.results.is_empty());
        assert_eq!(state.current_search_generation("client-a"), 2);
    }

    #[tokio::test]
    async fn test_search_列挙中の後続検索で先行検索がファイル処理前に終了する() {
        let _lock = lock_search_hook_tests().await;
        let dir = tempfile::tempdir().unwrap();
        for index in 0..10 {
            std::fs::write(
                dir.path().join(format!("note-{index:02}.md")),
                "needle visible",
            )
            .unwrap();
        }
        let state = Arc::new(create_directory_state(dir.path()));
        let fired = Arc::new(AtomicBool::new(false));
        let base_dir = dir.path().to_path_buf();
        let _hook = set_catalog_progress_hook_for_test({
            let state = Arc::clone(&state);
            let fired = Arc::clone(&fired);
            Arc::new(move |display_path| {
                if display_path.starts_with(&base_dir) && !fired.swap(true, Ordering::SeqCst) {
                    state.begin_search_generation("client-a", None).unwrap();
                }
            })
        });

        let response = search(&state, "needle".to_string(), Some("client-a"), None)
            .await
            .unwrap();

        assert!(fired.load(Ordering::SeqCst));
        assert_eq!(response.searched_files, 0);
        assert!(response.results.is_empty());
        assert_eq!(state.current_search_generation("client-a"), 2);
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

    #[test]
    fn test_sidebar_directory_name_fallbackは日本語名を返す() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir
            .path()
            .ancestors()
            .last()
            .expect("root directory should exist");
        let state = create_directory_state(root);

        assert_eq!(sidebar_directory_name(&state), "ドキュメント");
    }

    #[test]
    fn test_sidebar_directory_nameは通常ディレクトリ名を使う() {
        let dir = tempfile::tempdir().unwrap();
        let state = create_directory_state(dir.path());

        assert_eq!(
            sidebar_directory_name(&state),
            dir.path().file_name().unwrap().to_str().unwrap()
        );
    }

    #[test]
    fn test_route_target_request_pageからmemoリクエストを派生できる() {
        let request = RouteTargetRequest::page(Some("docs/guide.md"));

        let memo_request = RouteTargetRequest::api_memo(request.query_file());

        assert_eq!(memo_request.kind(), RouteTargetKind::ApiMemo);
        assert_eq!(memo_request.query_file(), Some("docs/guide.md"));
    }

    #[test]
    fn test_memo_message_file_ディレクトリではrelative_pathを使う() {
        let target = ResolvedTarget::for_test(
            std::path::PathBuf::from("/workspace/docs/guide.md"),
            Some(vec!["docs/guide.md".to_string()]),
            Some("docs/guide.md".to_string()),
        );

        assert_eq!(memo_message_file(&target), "docs/guide.md");
    }

    #[test]
    fn test_memo_message_file_単一ファイルではfile_nameにフォールバックする() {
        let target =
            ResolvedTarget::for_test(std::path::PathBuf::from("/workspace/note.md"), None, None);

        assert_eq!(memo_message_file(&target), "note.md");
    }

    #[test]
    fn test_memo_message_file_file_nameなしではdisplayにフォールバックする() {
        #[cfg(windows)]
        let path = std::path::PathBuf::from(r"C:\");
        #[cfg(not(windows))]
        let path = std::path::PathBuf::from("/");
        let expected = path.display().to_string();
        let target = ResolvedTarget::for_test(path, None, None);

        assert_eq!(memo_message_file(&target), expected);
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
        assert_eq!(page.memo.memo_state(), MemoState::Ready);
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
        assert_eq!(page.memo.memo_state(), MemoState::Degraded);
        assert_eq!(
            page.memo.load_error(),
            Some("メモを読み込めませんでした。内容を保護するため編集を無効化しています。本文の閲覧は継続できます。")
        );
    }

    #[tokio::test]
    async fn test_load_pageとload_memoはメモread失敗時の非対称仕様を保持する() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
        let sidecar_path = dir.path().join(".README.md.memo.md");
        std::fs::write(&sidecar_path, "memo body").unwrap();
        let memo_fs = MockMemoFs::new();
        memo_fs.fail_at(
            Op::Read,
            &sidecar_path,
            std::io::ErrorKind::PermissionDenied,
        );
        let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
        let state = AppState::new(
            AppMode::new_directory(dir.path()).unwrap(),
            false,
            None,
            tx,
            memo_fs,
        );

        let page = load_page(
            &state,
            PageRequest {
                file: Some("README.md"),
            },
        )
        .await
        .expect("index描画ではメモ読み込み失敗をフォールバックする");
        assert_eq!(page.memo.raw(), "");
        assert_eq!(page.memo.memo_state(), MemoState::Degraded);
        assert_eq!(
            page.memo.load_error(),
            Some("メモを読み込めませんでした。内容を保護するため編集を無効化しています。本文の閲覧は継続できます。")
        );

        let error = load_memo(
            &state,
            MemoRequest {
                file: Some("README.md"),
            },
        )
        .await
        .expect_err("api/memoではメモ読み込み失敗をエラーとして返す");
        assert_eq!(error.0, StatusCode::INTERNAL_SERVER_ERROR);
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
        let state = AppState::new_with_tokio_memo_fs(
            AppMode::new_directory(dir.path()).unwrap(),
            false,
            None,
            tx,
        );
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
    async fn test_save_memo_並行保存は各ファイルのmemo_updateをbroadcastする() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
        std::fs::write(dir.path().join("notes.md"), "# Notes").unwrap();
        let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
        let state = AppState::new_with_tokio_memo_fs(
            AppMode::new_directory(dir.path()).unwrap(),
            false,
            None,
            tx,
        );
        let mut rx = state.tx().subscribe();

        let (readme, notes) = tokio::join!(
            save_memo(
                &state,
                SaveMemoRequest {
                    file: Some("README.md"),
                    raw: "readme memo".to_string(),
                },
            ),
            save_memo(
                &state,
                SaveMemoRequest {
                    file: Some("notes.md"),
                    raw: "notes memo".to_string(),
                },
            )
        );

        assert_eq!(readme.unwrap().raw(), "readme memo");
        assert_eq!(notes.unwrap().raw(), "notes memo");
        let mut files = Vec::new();
        for _ in 0..2 {
            match rx
                .try_recv()
                .expect("並行保存それぞれでmemo_updateが送信されるべき")
            {
                BroadcastMessage::MemoUpdate(update) => files.push(update.file().to_string()),
                other => panic!("unexpected broadcast: {:?}", other),
            }
        }
        files.sort();
        assert_eq!(files, vec!["README.md".to_string(), "notes.md".to_string()]);
    }

    #[tokio::test]
    async fn test_save_memo_失敗時はbroadcastしない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
        let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
        let state = AppState::new_with_tokio_memo_fs(
            AppMode::new_directory(dir.path()).unwrap(),
            false,
            None,
            tx,
        );
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

    #[tokio::test]
    async fn test_list_files_ディレクトリモードではmarkdown一覧を返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
        std::fs::write(dir.path().join("note.txt"), "not markdown").unwrap();
        std::fs::create_dir_all(dir.path().join("guide")).unwrap();
        std::fs::write(dir.path().join("guide/setup.md"), "# Setup").unwrap();
        let state = create_directory_state(dir.path());

        let files = list_files(&state).await.unwrap();

        assert_eq!(
            files,
            vec!["README.md".to_string(), "guide/setup.md".to_string()]
        );
    }

    #[tokio::test]
    async fn test_list_files_単一ファイルモードでは空配列を返す() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("note.md");
        std::fs::write(&file_path, "# Note").unwrap();
        let state = create_single_file_state(&file_path);

        let files = list_files(&state).await.unwrap();

        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn test_search_ディレクトリモードでは検索結果を返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
        std::fs::write(dir.path().join("other.md"), "# Other").unwrap();
        let state = create_directory_state(dir.path());

        let response = search(&state, " needle ".to_string(), Some("client-a"), None)
            .await
            .unwrap();

        assert_eq!(response.query, "needle");
        assert_eq!(response.searched_files, 2);
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].file, "README.md");
    }

    #[tokio::test]
    async fn test_search_単一ファイルモードでは空結果を返す() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("note.md");
        std::fs::write(&file_path, "# Note\n\nneedle").unwrap();
        let state = create_single_file_state(&file_path);

        let response = search(&state, "needle".to_string(), Some("client-a"), None)
            .await
            .unwrap();

        assert_eq!(response.query, "needle");
        assert_eq!(response.searched_files, 0);
        assert_eq!(response.skipped_files, 0);
        assert!(response.results.is_empty());
        assert!(!response.truncated);
        assert!(response.truncated_reasons.is_empty());
        assert_eq!(response.limits.max_results, 100);
        assert_eq!(response.limits.max_files, 1000);
        assert_eq!(response.limits.max_bytes, 64 * 1024 * 1024);
        assert_eq!(response.searched_bytes, 0);
    }

    #[tokio::test]
    async fn test_search_単一ファイルモードでも長すぎるqueryはbad_request() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("note.md");
        std::fs::write(&file_path, "# Note\n\nneedle").unwrap();
        let state = create_single_file_state(&file_path);
        let query = "あ".repeat(crate::server::files::MAX_SEARCH_QUERY_CHARS + 1);

        let error = search(&state, query, Some("client-a"), None)
            .await
            .expect_err("長すぎる検索queryは単一ファイルモードでも拒否する");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(error.1["error"].as_str(), Some("検索クエリが長すぎます"));
    }
}
