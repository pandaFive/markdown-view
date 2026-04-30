# RouteContext Service Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `RouteContext` responsibilities so `src/server/routes.rs` becomes an HTTP adapter and application flow moves into `src/server/service.rs`.

**Architecture:** Add a server-level service module that owns request DTOs, page view assembly, target resolution orchestration, memo broadcast, file listing, and search orchestration. Keep Host/Origin checks in HTTP routes for this phase, preserve all external API/HTML/WebSocket behavior, and remove `RouteContext` after every handler calls service functions.

**Tech Stack:** Rust, axum, tokio broadcast, existing `server::files` APIs, existing `template::{RenderPageParams, SidebarParams}`.

---

## File Structure

- Create `src/server/service.rs`: application service layer. Owns request DTOs, `PageView`, `SidebarView`, service functions, and service unit tests.
- Modify `src/server.rs`: add `mod service;`.
- Modify `src/server/routes.rs`: remove `RouteContext`, keep HTTP extraction and Host/WebSocket checks, call `service` functions, convert `SidebarView` to `SidebarParams`.
- Modify `docs/todo/TODO.md`: mark the `RouteContext` split item complete after implementation and verification.

Do not move `Host` validation into middleware in this implementation. Do not change `search_directory`, `list_markdown_files`, memo save semantics, body limits, CSP, or WebSocket Origin validation.

## Task 1: Add Service Module Skeleton And Page View Types

**Files:**
- Create: `src/server/service.rs`
- Modify: `src/server.rs`
- Test: `src/server/service.rs`

- [ ] **Step 1: Write the failing service skeleton tests**

Create `src/server/service.rs` with only the request/view types and tests below. The test intentionally calls methods that do not exist yet.

```rust
use crate::template::UpdateMessage;
use crate::template::MemoResponse;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
```

- [ ] **Step 2: Register the module and verify the test fails**

Modify `src/server.rs`:

```rust
mod broadcast;
mod files;
mod guards;
pub(crate) mod log_path;
mod messages;
mod routes;
mod service;
mod session;
mod state;
mod watch;
```

Run:

```bash
cargo test service::tests::test_sidebar_view --all-targets --all-features
```

Expected: FAIL with missing associated functions `SidebarView::directory` and `SidebarView::single_file`.

- [ ] **Step 3: Implement the minimal constructors**

Add this impl to `src/server/service.rs` after the `SidebarView` enum:

```rust
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
```

- [ ] **Step 4: Run the focused tests**

Run:

```bash
cargo test service::tests::test_sidebar_view --all-targets --all-features
```

Expected: PASS, two service tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/server.rs src/server/service.rs
git commit -m "refactor: service層の型を追加"
```

## Task 2: Move Page Assembly Into Service

**Files:**
- Modify: `src/server/service.rs`
- Modify: `src/server/routes.rs`
- Test: `src/server/service.rs`

- [ ] **Step 1: Add failing page service tests**

Append these tests inside `#[cfg(test)] mod tests` in `src/server/service.rs`. Keep the existing `SidebarView` tests.

```rust
use tokio::sync::broadcast;

use crate::server::messages::BroadcastMessage;
use crate::server::state::{AppMode, AppState};

fn create_directory_state(base_dir: &std::path::Path) -> AppState {
    let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
    AppState::new(AppMode::new_directory(base_dir).unwrap(), false, None, tx)
}

fn create_single_file_state(file_path: &std::path::Path) -> AppState {
    let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
    AppState::new(AppMode::new_single_file(file_path).unwrap(), false, None, tx)
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
            directory_name: dir.path().file_name().unwrap().to_string_lossy().into_owned(),
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test service::tests::test_load_page --all-targets --all-features
```

Expected: FAIL with `cannot find function load_page`.

- [ ] **Step 3: Implement page service functions**

Replace the top of `src/server/service.rs` with these imports and keep the existing public structs:

```rust
use axum::http::StatusCode;

use super::files::{
    list_markdown_files, load_route_memo, load_route_update, resolve_route_target,
    RouteTargetRequest,
};
use super::guards::json_error;
use super::messages::ApiError;
use super::state::AppState;
use crate::template::{MemoResponse, UpdateMessage};
```

Add these helpers and function after `impl SidebarView`:

```rust
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
    let memo = match load_route_memo(state, &target, RouteTargetRequest::api_memo(request.file)).await {
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
```

- [ ] **Step 4: Run page service tests**

Run:

```bash
cargo test service::tests::test_load_page --all-targets --all-features
```

Expected: PASS, three `load_page` tests pass.

- [ ] **Step 5: Adapt `index_handler` to use service**

Keep the existing `super::files` imports for now. `RouteContext` still needs them until content, memo, files, and search handlers are moved in later tasks.

Add:

```rust
use super::service::{self, PageRequest, SidebarView};
```

Remove the local `sidebar_directory_name` function from `routes.rs`.

Add this helper near the query structs:

```rust
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
```

Replace `index_handler` body with:

```rust
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
```

Keep `RouteContext` temporarily for other handlers.

- [ ] **Step 6: Run route and integration checks**

Run:

```bash
cargo test server::routes::tests::test_route_context --all-targets --all-features
cargo test test_indexページ取得 --all-targets --all-features
```

Expected: PASS. The route context tests still pass because `RouteContext` remains for content/memo.

- [ ] **Step 7: Commit**

```bash
git add src/server/service.rs src/server/routes.rs
git commit -m "refactor: ページ組み立てをservice層へ移動"
```

## Task 3: Move Content And Memo Flows Into Service

**Files:**
- Modify: `src/server/service.rs`
- Modify: `src/server/routes.rs`
- Test: `src/server/service.rs`

- [ ] **Step 1: Add failing content and memo service tests**

Append these tests inside `src/server/service.rs` test module.

```rust
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
    let message = rx.try_recv().expect("保存成功時はmemo_updateが送信されるべき");
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

    assert_eq!(error.0, axum::http::StatusCode::FORBIDDEN);
    assert!(rx.try_recv().is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test service::tests::test_load --all-targets --all-features
cargo test service::tests::test_save_memo --all-targets --all-features
```

Expected: FAIL with missing functions `load_content`, `load_memo`, and `save_memo`.

- [ ] **Step 3: Implement content and memo service functions**

Update imports in `src/server/service.rs`:

```rust
use super::files::{
    list_markdown_files, load_route_memo, load_route_update, resolve_route_target,
    save_route_memo, RouteTargetRequest,
};
use super::messages::{ApiError, BroadcastMessage};
use crate::template::{MemoResponse, MemoUpdateMessage, UpdateMessage};
```

Add these functions after `load_page`:

```rust
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

fn memo_message_file(target: &super::files::ResolvedTarget) -> String {
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
```

If `ResolvedTarget` is no longer imported after cleanup, add it to the `super::files` import list and change the helper signature to `fn memo_message_file(target: &ResolvedTarget) -> String`.

- [ ] **Step 4: Run focused service tests**

Run:

```bash
cargo test service::tests::test_load --all-targets --all-features
cargo test service::tests::test_save_memo --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 5: Adapt content and memo handlers**

In `src/server/routes.rs`, extend service imports:

```rust
use super::service::{self, ContentRequest, MemoRequest, PageRequest, SaveMemoRequest, SidebarView};
```

Replace `api_content_handler` body with:

```rust
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
```

Replace `api_memo_handler` body with:

```rust
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
```

Replace `api_memo_save_handler` body with:

```rust
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
```

- [ ] **Step 6: Remove now-unused RouteContext methods**

In `src/server/routes.rs`, remove these methods from `impl RouteContext<'a>` if they are unused:

```rust
async fn load_update(&self) -> Result<UpdateMessage, ApiError> { ... }
async fn load_memo(&self) -> Result<MemoResponse, ApiError> { ... }
async fn save_memo(&self, raw: String) -> Result<MemoResponse, ApiError> { ... }
fn broadcast_saved_memo(&self) { ... }
fn memo_message_file(&self) -> String { ... }
fn title(&self) -> &str { ... }
fn sidebar(&self) -> SidebarParams<'_> { ... }
fn target(&self) -> &ResolvedTarget { ... }
```

Keep `ensure_allowed`, `resolve`, and `memo_request` temporarily if route tests still use them.

- [ ] **Step 7: Run handler and integration tests**

Run:

```bash
cargo test service::tests --all-targets --all-features
cargo test test_apiコンテンツ取得 --all-targets --all-features
cargo test test_apiメモ_保存成功時にmemo_updateをbroadcastする --all-targets --all-features
cargo test test_apiメモ_保存失敗時はmemo_updateをbroadcastしない --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/server/service.rs src/server/routes.rs
git commit -m "refactor: contentとmemo処理をservice層へ移動"
```

## Task 4: Move Files And Search Flows Into Service, Remove RouteContext

**Files:**
- Modify: `src/server/service.rs`
- Modify: `src/server/routes.rs`
- Test: `src/server/service.rs`

- [ ] **Step 1: Add failing files/search service tests**

Append these tests inside `src/server/service.rs` test module.

```rust
#[test]
fn test_list_files_ディレクトリモードではmarkdown一覧を返す() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# Home").unwrap();
    std::fs::write(dir.path().join("note.txt"), "not markdown").unwrap();
    std::fs::create_dir_all(dir.path().join("guide")).unwrap();
    std::fs::write(dir.path().join("guide/setup.md"), "# Setup").unwrap();
    let state = create_directory_state(dir.path());

    let files = list_files(&state).unwrap();

    assert_eq!(files, vec!["README.md".to_string(), "guide/setup.md".to_string()]);
}

#[test]
fn test_list_files_単一ファイルモードでは空配列を返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("note.md");
    std::fs::write(&file_path, "# Note").unwrap();
    let state = create_single_file_state(&file_path);

    let files = list_files(&state).unwrap();

    assert!(files.is_empty());
}

#[tokio::test]
async fn test_search_ディレクトリモードでは検索結果を返す() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
    std::fs::write(dir.path().join("other.md"), "# Other").unwrap();
    let state = create_directory_state(dir.path());

    let response = search(&state, " needle ".to_string()).await.unwrap();

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

    let response = search(&state, "needle".to_string()).await.unwrap();

    assert_eq!(response.query, "needle");
    assert_eq!(response.searched_files, 0);
    assert_eq!(response.skipped_files, 0);
    assert!(response.results.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test service::tests::test_list_files --all-targets --all-features
cargo test service::tests::test_search --all-targets --all-features
```

Expected: FAIL with missing functions `list_files` and `search`, or inaccessible search response fields. If fields are inaccessible, update `SearchResponse` and `SearchResultItem` field visibility from `pub(in crate::server)` fields to accessor methods instead of widening public API. Add only these accessors:

```rust
impl SearchResponse {
    pub(in crate::server) fn query(&self) -> &str { &self.query }
    pub(in crate::server) fn results(&self) -> &[SearchResultItem] { &self.results }
    pub(in crate::server) fn searched_files(&self) -> usize { self.searched_files }
    pub(in crate::server) fn skipped_files(&self) -> usize { self.skipped_files }
}

impl SearchResultItem {
    pub(in crate::server) fn file(&self) -> &str { &self.file }
}
```

Then adjust the tests to call accessors:

```rust
assert_eq!(response.query(), "needle");
assert_eq!(response.searched_files(), 2);
assert_eq!(response.results().len(), 1);
assert_eq!(response.results()[0].file(), "README.md");
```

- [ ] **Step 3: Implement files/search service functions**

Update imports in `src/server/service.rs`:

```rust
use super::files::{
    list_markdown_files, load_route_memo, load_route_update, resolve_route_target,
    save_route_memo, search_directory, ResolvedTarget, RouteTargetRequest, SearchResponse,
};
```

Add these functions after `save_memo`:

```rust
pub(super) fn list_files(state: &AppState) -> Result<Vec<String>, ApiError> {
    if let Some(base) = state.mode().directory() {
        list_markdown_files(base).map_err(|error| {
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

pub(super) async fn search(
    state: &AppState,
    query: String,
) -> Result<SearchResponse, ApiError> {
    let Some(base_dir) = state.mode().directory() else {
        return Ok(SearchResponse {
            query: query.trim().to_string(),
            results: Vec::new(),
            searched_files: 0,
            skipped_files: 0,
        });
    };

    search_directory(base_dir, &query).await.map_err(|error| {
        tracing::warn!("[markdown-view] ディレクトリ検索エラー: {}", error);
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ディレクトリ検索に失敗しました",
        )
    })
}
```

- [ ] **Step 4: Run focused service tests**

Run:

```bash
cargo test service::tests::test_list_files --all-targets --all-features
cargo test service::tests::test_search --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 5: Adapt files and search handlers**

Replace `api_files_handler` body in `src/server/routes.rs`:

```rust
async fn api_files_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, ApiError> {
    ensure_allowed_request_host(&headers)?;
    Ok(Json(service::list_files(&state)?))
}
```

Replace `api_search_handler` body:

```rust
async fn api_search_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    ensure_allowed_request_host(&headers)?;
    let response = service::search(&state, query.q.unwrap_or_default()).await?;
    Ok(Json(response))
}
```

- [ ] **Step 6: Remove `RouteContext` and stale imports**

Delete the entire `RouteContext<'a>` struct and its `impl` from `src/server/routes.rs`.

Delete the `#[cfg(test)] mod tests` at the bottom of `src/server/routes.rs`, because both tests target `RouteContext`. Its surviving behavior is now covered by service tests and integration tests:

- memo request derivation is covered by `test_load_memo_指定ファイルのmemoを返す`.
- Host rejection is covered by `tests/integration_test.rs` tests such as `test_httpは許可されないhostを拒否する`, `test_ディレクトリモード_api_filesは不正hostを拒否する`, and `test_ディレクトリモード_api_searchは不正hostを拒否する`.

Clean imports in `src/server/routes.rs` so they include only used items. The top of the file should be close to:

```rust
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use tower_http::set_header::SetResponseHeaderLayer;

use super::files::{SearchResponse, MAX_FILE_SIZE};
use super::guards::{
    build_csp_header, ensure_allowed_request_host, is_allowed_ws_origin, json_error,
};
use super::messages::ApiError;
use super::service::{self, ContentRequest, MemoRequest, PageRequest, SaveMemoRequest, SidebarView};
use super::session::handle_socket;
use super::state::AppState;
use crate::template::{
    render_page, MemoResponse, RenderPageParams, SidebarParams, UpdateMessage,
};
```

- [ ] **Step 7: Run route, service, and integration checks**

Run:

```bash
cargo test service::tests --all-targets --all-features
cargo test test_ディレクトリモード_ファイル一覧api --all-targets --all-features
cargo test test_ディレクトリモード_検索apiは複数ファイルから結果を返す --all-targets --all-features
cargo test test_ディレクトリモード_api_filesは不正hostを拒否する --all-targets --all-features
cargo test test_ディレクトリモード_api_searchは不正hostを拒否する --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/server/service.rs src/server/routes.rs src/server/files/search.rs
git commit -m "refactor: filesとsearch処理をservice層へ移動"
```

If `src/server/files/search.rs` was not changed, omit it from `git add`.

## Task 5: Mark Work Complete And Run Full Verification

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Mark the TODO item complete**

In `docs/todo/TODO.md`, change only this line:

```markdown
- [ ] `RouteContext` を HTTP adapter と application service に分割する
```

to:

```markdown
- [x] `RouteContext` を HTTP adapter と application service に分割する
```

Do not edit the item body unless the implementation intentionally changed its scope.

- [ ] **Step 2: Run formatting**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS with no output or only rustfmt check success.

- [ ] **Step 3: Run lint**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS with no warnings.

- [ ] **Step 4: Run all tests**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 5: Run repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS, ending with `==> 検証が正常に完了しました。`

- [ ] **Step 6: Review the diff for scope control**

Run:

```bash
git diff --stat
git diff -- src/server/routes.rs src/server/service.rs src/server.rs docs/todo/TODO.md
```

Expected:

- `src/server/service.rs` contains the moved application orchestration.
- `src/server/routes.rs` has no `RouteContext`.
- Host checks remain in each HTTP handler.
- WebSocket still calls `ensure_allowed_request_host` and `is_allowed_ws_origin`.
- No search load-control, watcher, memo persistence, CSP, or body-limit behavior was changed.

- [ ] **Step 7: Commit**

```bash
git add docs/todo/TODO.md
git commit -m "docs: RouteContext分割TODOを完了"
```

## Security Notes

- Every HTTP handler must continue to call `ensure_allowed_request_host(&headers)?` before invoking service functions.
- `ws_handler` must continue to reject when `ensure_allowed_request_host(&headers).is_err()` or `!is_allowed_ws_origin(&headers)`.
- Service functions must call `resolve_route_target` before any content or memo file I/O.
- Do not pass `FileQuery.file`, `MemoSaveRequest.file`, or `SearchQuery.q` directly to filesystem APIs.
- Keep HTML sanitization and CSP untouched. `service.rs` only moves orchestration and must not introduce raw HTML rendering.

## Rollback Path

Each task is independently revertible:

- Revert Task 2 to move page assembly back to `RouteContext`.
- Revert Task 3 to move content/memo orchestration back to `RouteContext`.
- Revert Task 4 to restore files/search handler bodies and route tests.
- Revert Task 5 to restore the unchecked entry in `docs/todo/TODO.md`.

If a late verification failure appears after several commits, first revert the newest task commit and rerun the focused tests listed in that task.
