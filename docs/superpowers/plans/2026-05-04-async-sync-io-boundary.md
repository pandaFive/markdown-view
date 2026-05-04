# Async Sync I/O Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** async HTTP/API 経路から同期 I/O を外し、catalog と watcher が起動時 canonical base を再利用する構造へ整理する。

**Status:** Completed. This file is retained as the implementation record for the async/sync I/O boundary cleanup.

**Architecture:** `AppMode` が保持する `CanonicalPath` を catalog/search/resolve/watcher の境界へ渡す。同期ディレクトリ走査は `spawn_blocking` に閉じ、メモ symlink 検査は `tokio::fs::symlink_metadata` へ移す。watcher のディレクトリイベント判定は削除済みパスを扱える lexical helper を主経路にする。

**Tech Stack:** Rust, Tokio, axum, notify, tempfile, cargo test, `./verify.sh`

---

## File Structure

- Modify: `src/server/files/catalog.rs`
  - canonical base API、再帰引数、symlink directory 判定、visited set の責務を持つ。
- Modify: `src/server/files/resolve.rs`
  - route target 解決を async 化し、ディレクトリ一覧取得を blocking helper 経由にする。
- Modify: `src/server/service.rs`
  - async 化された target 解決に追従し、一覧取得 helper の重複を減らす。
- Modify: `src/server/files/search.rs`
  - search blocking task 内で canonical base API を使い、base の再 canonicalize を避ける。
- Modify: `src/server/files/memo.rs`
  - メモ保存先 symlink component 検査を async metadata へ変更する。
- Modify: `src/watcher/strategy.rs`
  - directory mode の base 配下判定と相対化を lexical helper に寄せる。
- Modify: `src/server/files/tests.rs`
  - catalog、resolve、memo の境界テストを追加・更新する。
- Modify: `tests/integration_test.rs`
  - API 経由の既存契約が変わらないことを既存テスト実行で確認する。

---

### Task 1: catalog に canonical base API を追加する

**Files:**
- Modify: `src/server/files/catalog.rs`
- Modify: `src/server/files/tests.rs`

- [x] **Step 1: failing test を追加する**

`src/server/files/tests.rs` の catalog tests 付近に、canonical base API を直接使うテストを追加する。

```rust
#[test]
fn test_list_markdown_files_from_canonical_base_基本動作() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("b.md"), "# b").unwrap();
    std::fs::write(dir.path().join("a.md"), "# a").unwrap();
    std::fs::write(dir.path().join("skip.txt"), "skip").unwrap();

    let canonical = CanonicalPath::try_from_path(dir.path()).unwrap();
    let files = list_markdown_files_from_canonical_base(&canonical).unwrap();

    assert_eq!(files, vec!["a.md".to_string(), "b.md".to_string()]);
}

#[test]
#[cfg(unix)]
fn test_list_markdown_files_from_canonical_base_ベース外symlinkディレクトリは除外() {
    use std::os::unix::fs::symlink;

    let base = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.md"), "# secret").unwrap();
    symlink(outside.path(), base.path().join("linked")).unwrap();

    let canonical = CanonicalPath::try_from_path(base.path()).unwrap();
    let files = list_markdown_files_from_canonical_base(&canonical).unwrap();

    assert!(files.is_empty());
}
```

テストモジュールの import に以下を追加する。

```rust
use crate::server::CanonicalPath;
use super::catalog::list_markdown_files_from_canonical_base;
```

- [x] **Step 2: test が失敗することを確認する**

Run:

```bash
cargo test --all-targets --all-features test_list_markdown_files_from_canonical_base -- --nocapture
```

Expected: `list_markdown_files_from_canonical_base` が未定義で FAIL。

- [x] **Step 3: catalog の最小実装を追加する**

`src/server/files/catalog.rs` を次の形へ整理する。既存関数は残し、canonical base API に委譲する。

```rust
use crate::server::CanonicalPath;

pub(super) fn list_markdown_files_from_canonical_base(
    base_dir: &CanonicalPath,
) -> std::io::Result<Vec<String>> {
    list_markdown_files_with_limit_from_canonical_base(base_dir, MAX_FILE_LIST)
}

pub(super) fn list_markdown_files_with_limit_from_canonical_base(
    base_dir: &CanonicalPath,
    max_files: usize,
) -> std::io::Result<Vec<String>> {
    let base_path = base_dir.as_path();
    let mut files = Vec::new();
    let mut visited_dirs = HashSet::new();
    visited_dirs.insert(base_path.to_path_buf());
    list_markdown_files_recursive(
        base_path,
        base_path,
        base_path,
        &mut files,
        &mut visited_dirs,
        0,
        max_files,
    )?;
    files.sort();
    files.truncate(max_files);
    Ok(files)
}

pub(super) fn list_markdown_files_with_limit(
    base_dir: &Path,
    max_files: usize,
) -> std::io::Result<Vec<String>> {
    let canonical = CanonicalPath::try_from_path(base_dir)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::NotFound, error))?;
    list_markdown_files_with_limit_from_canonical_base(&canonical, max_files)
}
```

`list_markdown_files_recursive` の signature を次に変更する。

```rust
fn list_markdown_files_recursive(
    log_base_dir: &Path,
    canonical_base_dir: &Path,
    current_dir: &Path,
    files: &mut Vec<String>,
    visited_dirs: &mut HashSet<PathBuf>,
    depth: usize,
    max_files: usize,
) -> std::io::Result<()>
```

関数内の `base_dir` 参照は、ログと `strip_prefix` では `log_base_dir`、base 外 symlink 判定では `canonical_base_dir` を使う。symlink 分岐の `base_dir.canonicalize()` ブロックは削除し、次に置き換える。

```rust
if !resolved.starts_with(canonical_base_dir) {
    tracing::warn!(
        "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
        sanitize_path_for_logging(&path, log_base_dir),
        sanitize_path_for_logging(&resolved, log_base_dir)
    );
    continue;
}
```

再帰呼び出しは次にする。

```rust
list_markdown_files_recursive(
    log_base_dir,
    canonical_base_dir,
    &path,
    files,
    visited_dirs,
    depth + 1,
    max_files,
)?;
```

- [x] **Step 4: targeted test を通す**

Run:

```bash
cargo test --all-targets --all-features test_list_markdown_files_from_canonical_base -- --nocapture
```

Expected: PASS。

- [x] **Step 5: catalog 既存テストを通す**

Run:

```bash
cargo test --all-targets --all-features list_markdown_files -- --nocapture
```

Expected: PASS。既存の hidden、上限、cycle、canonicalize 失敗テストが維持される。

- [x] **Step 6: commit**

```bash
git add src/server/files/catalog.rs src/server/files/tests.rs
git commit -m "refactor: catalogでcanonical baseを再利用する"
```

---

### Task 2: resolve/service のディレクトリ一覧取得を blocking 境界へ移す

**Files:**
- Modify: `src/server/files/resolve.rs`
- Modify: `src/server/service.rs`
- Modify: `src/server/files/tests.rs`
- Modify: `tests/integration_test.rs`

- [x] **Step 1: async 化の呼び出し元を確認する**

Run:

```bash
rg -n "resolve_route_target\\(" src tests
```

Expected: `src/server/service.rs` と `src/server/files` 内の呼び出しが表示される。

- [x] **Step 2: failing compile を作るテスト変更を入れる**

`src/server/files/tests.rs` にある `resolve_route_target` 呼び出しテストを async test に変更する。既存の同名テストがあればその body を以下の呼び方へ更新する。

```rust
#[tokio::test]
async fn test_resolve_route_target_ディレクトリ既定ファイルを返す() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# readme").unwrap();
    let state = test_directory_state(dir.path());

    let target = resolve_route_target(&state, RouteTargetRequest::page(None))
        .await
        .unwrap();

    assert_eq!(target.relative_path(), Some("README.md"));
    assert_eq!(target.file_list(), Some(["README.md".to_string()].as_slice()));
}
```

`test_directory_state` が既にない場合は、既存の state test helper に合わせて次の形で追加する。

```rust
fn test_directory_state(path: &Path) -> AppState {
    let (tx, _) = tokio::sync::broadcast::channel(16);
    AppState::new(
        AppMode::new_directory(path).unwrap(),
        false,
        None,
        tx,
    )
}
```

- [x] **Step 3: test が失敗することを確認する**

Run:

```bash
cargo test --all-targets --all-features test_resolve_route_target_ディレクトリ既定ファイルを返す -- --nocapture
```

Expected: `Result<ResolvedTarget, ApiError> is not a future` または async 化前提の compile error。

- [x] **Step 4: resolve の blocking helper を追加する**

`src/server/files/resolve.rs` の import を更新する。

```rust
use super::catalog::list_markdown_files_from_canonical_base;
use crate::server::state::{AppState, CanonicalPath};
```

同ファイルに helper を追加する。

```rust
async fn list_markdown_files_blocking(
    base_dir: &CanonicalPath,
) -> Result<Vec<String>, StatusCode> {
    let base_dir = base_dir.clone();
    tokio::task::spawn_blocking(move || list_markdown_files_from_canonical_base(&base_dir))
        .await
        .map_err(|error| {
            if error.is_panic() {
                tracing::error!(
                    "[markdown-view] ファイル一覧取得タスクがpanicしました: {}",
                    error
                );
            } else {
                tracing::warn!(
                    "[markdown-view] ファイル一覧取得タスクのjoinエラー: {}",
                    error
                );
            }
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map_err(|error| {
            tracing::warn!("[markdown-view] ファイル一覧取得エラー: {}", error);
            StatusCode::INTERNAL_SERVER_ERROR
        })
}
```

- [x] **Step 5: `resolve_route_target` と下位関数を async 化する**

signature を次に変更する。

```rust
pub(in crate::server) async fn resolve_route_target(
    state: &AppState,
    request: RouteTargetRequest<'_>,
) -> Result<ResolvedTarget, ApiError>
```

内部呼び出しを次に変更する。

```rust
let (file_path, file_list) = resolve_request_target(state, request).await.map_err(|status| {
```

`resolve_request_target` も async 化する。

```rust
async fn resolve_request_target(
    state: &AppState,
    request: RouteTargetRequest<'_>,
) -> Result<(PathBuf, Option<Vec<String>>), StatusCode>
```

directory branch は canonical base を使う。

```rust
let Some(base_dir) = state.mode().directory_canonical() else {
    tracing::error!("[markdown-view] 未知のAppModeです");
    return Err(StatusCode::INTERNAL_SERVER_ERROR);
};
let base_path = base_dir.as_path();
```

`resolve_file(base_dir, relative)` は `resolve_file(base_path, relative)` に置き換える。一覧取得は次に置き換える。

```rust
let files = list_markdown_files_blocking(base_dir).await?;
```

file list 同梱時の追加取得も次にする。

```rust
None => Some(list_markdown_files_blocking(base_dir).await?),
```

- [x] **Step 6: service の呼び出しに `.await` を追加する**

`src/server/service.rs` の `resolve_route_target` 呼び出しを全て以下の形にする。

```rust
let target = resolve_route_target(state, route_request).await?;
```

対象は `load_page`、`load_content`、`load_memo`、`save_memo`。

- [x] **Step 7: `service::list_files` を canonical base helper に寄せる**

`src/server/service.rs` の import を `list_markdown_files_from_canonical_base` へ寄せる。

```rust
use super::files::{
    list_markdown_files_from_canonical_base, load_route_memo, load_route_update,
    resolve_route_target, save_route_memo, SearchResponse,
};
```

`list_files` の base 取得を次に変更する。

```rust
if let Some(base) = state.mode().directory_canonical().cloned() {
    tokio::task::spawn_blocking(move || list_markdown_files_from_canonical_base(&base))
```

- [x] **Step 8: targeted tests を通す**

Run:

```bash
cargo test --all-targets --all-features test_resolve_route_target_ディレクトリ既定ファイルを返す -- --nocapture
```

Expected: PASS。

Run:

```bash
cargo test --all-targets --all-features api_files -- --nocapture
```

Expected: PASS または matching test がない場合は `0 passed; 0 failed`。

- [x] **Step 9: commit**

```bash
git add src/server/files/resolve.rs src/server/service.rs src/server/files/tests.rs tests/integration_test.rs
git commit -m "refactor: route target解決の一覧取得をblocking化する"
```

---

### Task 3: search を canonical base API へ追従させる

**Files:**
- Modify: `src/server/files/search.rs`
- Modify: `src/server/service.rs`
- Modify: `src/server/files/tests.rs`
- Modify: `tests/integration_test.rs`

- [x] **Step 1: failing test を追加する**

`src/server/files/tests.rs` の search tests 付近に、`CanonicalPath` を直接受け取って検索できることを固定するテストを追加する。`Path` 入口で再 `canonicalize` する互換 API ではなく、呼び出し側が保持している起動時 canonical base を渡す契約をテスト名で明示する。

```rust
#[tokio::test]
async fn test_search_directory_canonical_base_再canonicalizeなしで検索する() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("guide.md"), "hello search target").unwrap();
    let canonical = CanonicalPath::try_from_path(dir.path()).unwrap();

    let response = search_directory(&canonical, "target").await.unwrap();

    assert_eq!(response.query, "target");
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].file, "guide.md");
}
```

Expected: 既存 API では `search_directory` が `&Path` を受けるため型不一致で FAIL。

- [x] **Step 2: search の import を canonical base API 前提へ変更する**

`src/server/files/search.rs` の import を次へ変更する。

```rust
use super::catalog::list_markdown_files_with_limit_from_canonical_base;
```

Run:

```bash
cargo test --all-targets --all-features test_search_directory_canonical_base_再canonicalizeなしで検索する -- --nocapture
```

Expected: `search_directory` の signature 変更前なので型不一致で FAIL。

- [x] **Step 3: search blocking core へ canonical base を渡す**

`src/server/files/search.rs` の imports を次にする。`Path` は `read_markdown_with_limit_blocking` などのファイル読込 helper で使うため残す。

```rust
use std::path::Path;
use super::catalog::list_markdown_files_with_limit_from_canonical_base;
use crate::server::CanonicalPath;
```

`search_directory` は canonical base を直接受ける。

```rust
pub(in crate::server) async fn search_directory(
    base_dir: &CanonicalPath,
    raw_query: &str,
) -> std::io::Result<SearchResponse> {
    let base_dir = base_dir.clone();
    let raw_query = raw_query.to_owned();

    tokio::task::spawn_blocking(move || search_directory_blocking(&base_dir, &raw_query))
        .await
        .map_err(map_search_join_error)?
}
```

blocking core signature を変更する。

```rust
fn search_directory_blocking(
    base_dir: &CanonicalPath,
    raw_query: &str,
) -> std::io::Result<SearchResponse> {
    search_directory_with_limits_blocking(base_dir, raw_query, SearchLimits::default())
}

fn search_directory_with_limits_blocking(
    base_dir: &CanonicalPath,
    raw_query: &str,
    limits: SearchLimits,
) -> std::io::Result<SearchResponse>
```

候補取得と resolve を次に変更する。

```rust
let files = list_markdown_files_with_limit_from_canonical_base(
    base_dir,
    limits.max_files.saturating_add(1),
)?;
let base_path = base_dir.as_path();
```

```rust
let file_path = match resolve_file(base_path, &relative) {
```

- [x] **Step 4: service の search 呼び出しを canonical path へ寄せる**

`src/server/service.rs` の search 関数を次に変更する。

```rust
let Some(base_dir) = state.mode().directory_canonical() else {
    return Ok(SearchResponse::empty(query.trim().to_string()));
};

search_directory(base_dir, &query).await.map_err(|error| {
```

- [x] **Step 5: targeted tests を通す**

Run:

```bash
cargo test --all-targets --all-features test_search_directory_canonical_base_再canonicalizeなしで検索する -- --nocapture
```

Expected: PASS。

Run:

```bash
cargo test --all-targets --all-features search -- --nocapture
```

Expected: PASS。

- [x] **Step 6: commit**

```bash
git add src/server/files/search.rs src/server/service.rs src/server/files/tests.rs tests/integration_test.rs
git commit -m "refactor: 検索候補列挙でcanonical baseを使う"
```

---

### Task 4: memo の symlink component 検査を async metadata にする

**Files:**
- Modify: `src/server/files/memo.rs`
- Modify: `src/server/files/tests.rs`

- [x] **Step 1: 既存 symlink 拒否テストを targeted 実行する**

Run:

```bash
cargo test --all-targets --all-features memo symlink -- --nocapture
```

Expected: 既存の symlink 関連テストが PASS。該当テストが名前一致しない場合は `0 passed; 0 failed` ではなく、`rg -n "symlink|シンボリック" src/server/files/tests.rs` で対象名を探して実行する。

- [x] **Step 2: async 化で compile failure を作る**

`src/server/files/memo.rs` の `first_unsafe_memo_path_component` signature を先に変更する。

```rust
async fn first_unsafe_memo_path_component(
    base_dir: &Path,
    target: &Path,
) -> Option<UnsafeMemoPathComponent>
```

Run:

```bash
cargo test --all-targets --all-features memo -- --nocapture
```

Expected: 呼び出し元が `.await` していない compile error。

- [x] **Step 3: unsafe path 検査の呼び出し元を async 化する**

`ensure_safe_memo_path` を変更する。

```rust
async fn ensure_safe_memo_path(
    memo_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), ApiError> {
    let base_dir = state.mode().base_dir();
    if let Some(unsafe_component) = first_unsafe_memo_path_component(base_dir, memo_path).await {
        log_unsafe_memo_path(&unsafe_component, memo_path, base_dir, target, request);
        return Err(json_error(
            StatusCode::FORBIDDEN,
            unsafe_component.user_message(),
        ));
    }
    Ok(())
}
```

`ensure_safe_memo_rename_paths` と `ensure_safe_memo_rename_path` も async 化する。

```rust
async fn ensure_safe_memo_rename_paths(
    final_path: &Path,
    tmp_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), MemoBeforeRenameError>
```

```rust
async fn ensure_safe_memo_rename_path(
    memo_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), MemoBeforeRenameError>
```

内部呼び出しは次にする。

```rust
if let Some(unsafe_component) = first_unsafe_memo_path_component(base_dir, memo_path).await {
```

上位の呼び出しは `.await` を追加する。

```rust
ensure_safe_memo_path(path, state, target, request).await
```

```rust
ensure_safe_memo_rename_paths(&final_path, &tmp_path, state, target, request).await?;
```

- [x] **Step 4: `tokio::fs::symlink_metadata` へ置換する**

`first_unsafe_memo_path_component` 内を次に変更する。

```rust
match tokio::fs::symlink_metadata(&current).await {
    Ok(metadata) if metadata.file_type().is_symlink() => {
        return Some(UnsafeMemoPathComponent::Symlink(current));
    }
    Ok(_) => {}
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
    Err(error) => {
        tracing::warn!(
            "[markdown-view] メモパス要素のsymlink検査に失敗したため安全側で拒否します ({}): {}",
            sanitize_path_for_logging(&current, base_dir),
            error
        );
        return Some(UnsafeMemoPathComponent::InspectionError(current));
    }
}
```

- [x] **Step 5: targeted tests を通す**

Run:

```bash
cargo test --all-targets --all-features memo -- --nocapture
```

Expected: PASS。

- [x] **Step 6: commit**

```bash
git add src/server/files/memo.rs src/server/files/tests.rs
git commit -m "refactor: メモパス安全確認をasync metadataへ寄せる"
```

---

### Task 5: watcher directory 判定を canonical base 前提の lexical helper にする

**Files:**
- Modify: `src/watcher/strategy.rs`

- [x] **Step 1: failing tests を追加する**

`src/watcher/strategy.rs` の tests に追加する。

```rust
#[test]
fn test_collect_directory_changes_削除済みbase配下markdownを通知する() {
    let dir = tempfile::tempdir().unwrap();
    let canonical_base = CanonicalPath::try_from_path(dir.path()).unwrap();
    let deleted = canonical_base.as_path().join("docs").join("deleted.md");
    let events = vec![debounced_event(deleted.clone(), DebouncedEventKind::Any)];

    let changes = WatchStrategy::Directory {
        base_dir: canonical_base,
    }
    .collect_changed_paths(&events);

    assert_eq!(changes, vec![normalize_lexical_path(&deleted)]);
}

#[test]
fn test_collect_directory_changes_base外markdownを除外する() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let canonical_base = CanonicalPath::try_from_path(dir.path()).unwrap();
    let outside_file = outside.path().join("outside.md");
    let events = vec![debounced_event(outside_file, DebouncedEventKind::Any)];

    let changes = WatchStrategy::Directory {
        base_dir: canonical_base,
    }
    .collect_changed_paths(&events);

    assert!(changes.is_empty());
}
```

- [x] **Step 2: tests を実行して現状を確認する**

Run:

```bash
cargo test --all-targets --all-features collect_directory_changes -- --nocapture
```

Expected: 追加テストが PASS する場合がある。その場合も次 step で canonicalize fallback を削除する refactor を行い、既存テストを更新する。

- [x] **Step 3: directory helper を lexical base 前提に変更する**

`collect_directory_changes` の signature を変更する。

```rust
fn collect_directory_changes(base_dir: &CanonicalPath, events: &[DebouncedEvent]) -> Vec<PathBuf>
```

呼び出し元を変更する。

```rust
Self::Directory { base_dir } => collect_directory_changes(base_dir, events),
```

helper を追加する。

```rust
fn try_strip_canonical_base_lexical(path: &Path, canonical_base: &Path) -> Option<PathBuf> {
    let normalized_path = normalize_lexical_path(path);
    let normalized_base = normalize_lexical_path(canonical_base);
    normalized_path
        .strip_prefix(&normalized_base)
        .ok()
        .map(Path::to_path_buf)
}

fn is_within_canonical_base_lexical(path: &Path, canonical_base: &Path) -> bool {
    try_strip_canonical_base_lexical(path, canonical_base).is_some()
}

fn is_hidden_relative_to_canonical_base(path: &Path, canonical_base: &Path) -> bool {
    match try_strip_canonical_base_lexical(path, canonical_base) {
        Some(relative) => relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.')),
        None => {
            tracing::warn!(
                "[markdown-view] 隠しファイル判定: 相対パス算出不可（安全側で除外）: {}",
                sanitize_path_for_logging(path, canonical_base)
            );
            true
        }
    }
}
```

`collect_directory_changes` 内を次に変更する。

```rust
let base_path = base_dir.as_path();
if !is_within_canonical_base_lexical(&event.path, base_path) {
    tracing::warn!(
        "[markdown-view] ベースディレクトリ外のパスを検出（スキップ）: {}",
        sanitize_path_for_logging(&event.path, base_path)
    );
    continue;
}
if is_hidden_relative_to_canonical_base(&event.path, base_path) {
    continue;
}
```

既存 `try_strip_base` と `is_within_base_dir` は、単体テストが不要になれば削除する。削除する場合は tests import からも外す。

- [x] **Step 4: watcher tests を通す**

Run:

```bash
cargo test --all-targets --all-features watcher::strategy -- --nocapture
```

Expected: PASS。

Run:

```bash
cargo test --all-targets --all-features collect_directory_changes -- --nocapture
```

Expected: PASS。

- [x] **Step 5: commit**

```bash
git add src/watcher/strategy.rs
git commit -m "refactor: watcherのdirectory判定をlexical化する"
```

---

### Task 6: 全体検証と仕上げ

**Files:**
- Modify: `docs/todo/TODO.md`

- [x] **Step 1: formatting を確認する**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS。FAIL した場合は次を実行する。

```bash
cargo fmt --all
```

その後 `cargo fmt --all -- --check` を再実行して PASS を確認する。

- [x] **Step 2: clippy を通す**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS。

- [x] **Step 3: full test を通す**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS。

- [x] **Step 4: full verification を通す**

Run:

```bash
./verify.sh
```

Expected: PASS。

- [x] **Step 5: TODO を完了更新する**

`docs/todo/TODO.md` の該当項目を `[x]` に変更し、完了根拠を1行追加する。

```markdown
- [x] async ハンドラ内の同期 I/O を `spawn_blocking` ないし起動時固定化で解消する
  - 完了根拠: catalog は canonical base API で再帰中の base 再 canonicalize を廃止し、resolve/search のディレクトリ走査は blocking 境界へ隔離。memo symlink 検査は async metadata 化し、watcher directory 判定は削除イベント対応の lexical helper へ整理した
```

既存の詳細説明は削らず、完了根拠を `対応:` または `完了根拠:` として追記する。

- [x] **Step 6: final status を確認する**

Run:

```bash
git status --short
```

Expected: 変更対象が今回の files のみ。

- [x] **Step 7: final commit**

```bash
git add docs/todo/TODO.md
git commit -m "docs: async同期IO境界整理の完了を記録"
```

---

## Self-Review

Spec coverage:

- HTTP/API の同期走査隔離: Task 2, Task 3, Task 6。
- 起動時 canonical base の再利用: Task 1, Task 2, Task 3。
- watcher callback の正規化削減: Task 5。
- memo symlink metadata の async 化: Task 4。
- security constraints 維持: Task 1, Task 2, Task 3, Task 4, Task 5 のテスト。
- verification: Task 6。

Placeholder scan:

- 未確定事項を示す placeholder marker は含めない。
- 各 code step は対象 signature または追加コードを明示した。
- 各 task に targeted command と expected result を含めた。

Type consistency:

- catalog canonical API は `list_markdown_files_from_canonical_base` と `list_markdown_files_with_limit_from_canonical_base` に統一。
- canonical base 型は `crate::server::CanonicalPath` に統一。
- `search_directory` は `&CanonicalPath` を直接受け、入口で `Path` から再 `canonicalize` しない。
- watcher lexical helper は `try_strip_canonical_base_lexical`、`is_within_canonical_base_lexical`、`is_hidden_relative_to_canonical_base` に統一。
