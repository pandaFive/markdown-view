# Directory Search Cancellation Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ディレクトリ検索で新しい検索が始まったとき、古い検索をサーバ側の安全な区切りで協調的に早期終了できるようにする。

**Architecture:** `AppState` にプロセス内の検索世代カウンタを持たせ、ディレクトリ検索開始時に世代を進める。`src/server/files/search.rs` は開始世代を持つ `SearchCancellation` を受け取り、blocking 検索コアのファイル単位境界で stale 判定して部分 `SearchResponse` を返す。

**Tech Stack:** Rust, Tokio `spawn_blocking`, `std::sync::atomic::AtomicU64`, Axum service layer, existing unit/integration tests.

---

## File Structure

- Modify: `src/server/state.rs`
  - `AppState` に `Arc<AtomicU64>` の検索世代カウンタを追加する。
  - `SearchGeneration` value object を追加し、開始世代と現在世代 handle を保持する。
  - `begin_search_generation()` と `current_search_generation()` を追加する。
- Modify: `src/server.rs`
  - `SearchGeneration` を `pub(crate)` で server 配下へ再公開する。
- Modify: `src/server/files/search.rs`
  - `SearchCancellation` を追加する。
  - `search_directory()` / `search_directory_blocking()` / `search_directory_with_limits_blocking()` にキャンセル引数を通す。
  - 既存テストを新 signature へ追従し、キャンセル境界テストを追加する。
- Modify: `src/server/files/mod.rs`
  - `SearchCancellation` を `pub(in crate::server)` で service 層へ再公開する。
- Modify: `src/server/service.rs`
  - ディレクトリモード検索だけ `state.begin_search_generation()` で世代を進め、`SearchCancellation` を渡す。
  - 単一ファイルモードは世代を進めず、従来通り空結果を返す。
- Modify: `docs/todo/BACKLOG.md`
  - キャンセル境界が実装対象になったことを反映し、allocation 削減は残件として分離する。

## Task 1: AppState Search Generation

**Files:**
- Modify: `src/server/state.rs`
- Modify: `src/server.rs`

- [ ] **Step 1: Write failing tests for search generation**

Add these tests inside `#[cfg(test)] mod tests` in `src/server/state.rs`, after `test_app_state_new_with_tokio_memo_fsは本番用memo_fsを組み込む`.

```rust
    #[test]
    fn test_app_state_search_generationは初期値0() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);

        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);

        assert_eq!(state.current_search_generation(), 0);
    }

    #[test]
    fn test_begin_search_generationは世代を進めてhandleを返す() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);

        let first = state.begin_search_generation();
        let second = state.begin_search_generation();

        assert_eq!(first.started_at(), 1);
        assert_eq!(second.started_at(), 2);
        assert!(first.is_stale());
        assert!(!second.is_stale());
        assert_eq!(state.current_search_generation(), 2);
    }
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --lib test_app_state_search_generation
cargo test --lib test_begin_search_generation
```

Expected: FAIL because `current_search_generation` and `begin_search_generation` do not exist.

- [ ] **Step 3: Add search generation types and fields**

In `src/server/state.rs`, replace the existing sync import:

```rust
use std::sync::Arc;
```

with:

```rust
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
```

Add this type near `AppState`, before `/// サーバー共有状態`:

```rust
/// ディレクトリ検索の開始世代と現在世代を比較するためのhandle。
#[derive(Debug, Clone)]
pub(crate) struct SearchGeneration {
    started_at: u64,
    current: Arc<AtomicU64>,
}

impl SearchGeneration {
    pub(crate) fn new(started_at: u64, current: Arc<AtomicU64>) -> Self {
        Self {
            started_at,
            current,
        }
    }

    pub(crate) fn started_at(&self) -> u64 {
        self.started_at
    }

    pub(crate) fn is_stale(&self) -> bool {
        self.current.load(Ordering::Acquire) != self.started_at
    }
}
```

Add a field to `AppState`:

```rust
    search_generation: Arc<AtomicU64>,
```

Initialize the field in `AppState::new`:

```rust
            search_generation: Arc::new(AtomicU64::new(0)),
```

Add these methods in `impl AppState`, after `memo_fs()`:

```rust
    /// ディレクトリ検索用の新しい世代を発行する。
    pub(crate) fn begin_search_generation(&self) -> SearchGeneration {
        let generation = self
            .search_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        SearchGeneration::new(generation, Arc::clone(&self.search_generation))
    }

    /// 現在のディレクトリ検索世代を返す。
    pub(crate) fn current_search_generation(&self) -> u64 {
        self.search_generation.load(Ordering::Acquire)
    }
```

In `src/server.rs`, replace:

```rust
pub(crate) use self::state::{CanonicalPath, CanonicalPathError};
```

with:

```rust
pub(crate) use self::state::{CanonicalPath, CanonicalPathError, SearchGeneration};
```

- [ ] **Step 4: Run state tests**

Run:

```bash
cargo test --lib server::state
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/server/state.rs
git add src/server.rs
git commit -m "feat: 検索世代カウンタをAppStateへ追加"
```

## Task 2: SearchCancellation in Search Core

**Files:**
- Modify: `src/server/files/search.rs`
- Modify: `src/server/files/mod.rs`

- [ ] **Step 1: Write failing tests for SearchCancellation**

Add this import near the existing imports in `src/server/files/search.rs`:

```rust
use crate::server::{CanonicalPath, SearchGeneration};
```

Replace the existing import:

```rust
use crate::server::CanonicalPath;
```

Add these tests inside the existing `#[cfg(test)] mod tests` in `src/server/files/search.rs`, before `test_search_directory_通常検索は打ち切りなしの統計を返す`.

```rust
    #[test]
    fn test_search_cancellationは新しい世代を検知する() {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        };

        let current = Arc::new(AtomicU64::new(1));
        let generation = SearchGeneration::new(1, Arc::clone(&current));
        let cancellation = SearchCancellation::new(generation);

        assert!(!cancellation.is_cancelled());

        current.store(2, Ordering::Release);

        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn test_search_directory_キャンセル済みならファイル処理へ進まない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
        let canonical = canonical_of(dir.path());
        let cancellation = SearchCancellation::cancelled_for_test();

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            cancellation,
        )
        .unwrap();

        assert_eq!(response.query, "needle");
        assert_eq!(response.searched_files, 0);
        assert_eq!(response.skipped_files, 0);
        assert!(!response.truncated);
        assert!(response.results.is_empty());
    }

    #[test]
    fn test_search_directory_1ファイル後にキャンセルされたら後続ファイルへ進まない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle first").unwrap();
        std::fs::write(dir.path().join("b.md"), "needle second").unwrap();
        let canonical = canonical_of(dir.path());
        let cancellation = SearchCancellation::cancel_after_files_for_test(1);

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            cancellation,
        )
        .unwrap();

        assert_eq!(response.searched_files, 1);
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].file, "a.md");
    }
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --lib test_search_cancellation
cargo test --lib test_search_directory_キャンセル済み
cargo test --lib test_search_directory_1ファイル後
```

Expected: FAIL because `SearchCancellation` and the new `search_directory_with_limits_blocking` signature do not exist.

- [ ] **Step 3: Add SearchCancellation**

In `src/server/files/search.rs`, change the server import to include `SearchGeneration`:

```rust
use crate::server::{CanonicalPath, SearchGeneration};
```

Add this type after `SearchStats`:

```rust
#[derive(Debug, Clone)]
pub(in crate::server) struct SearchCancellation {
    generation: Option<SearchGeneration>,
    cancel_after_files_for_test: Option<usize>,
}

impl SearchCancellation {
    pub(in crate::server) fn new(generation: SearchGeneration) -> Self {
        Self {
            generation: Some(generation),
            cancel_after_files_for_test: None,
        }
    }

    fn none() -> Self {
        Self {
            generation: None,
            cancel_after_files_for_test: None,
        }
    }

    fn is_cancelled(&self) -> bool {
        self.generation
            .as_ref()
            .is_some_and(SearchGeneration::is_stale)
    }

    fn is_cancelled_after_files(&self, searched_files: usize) -> bool {
        self.is_cancelled()
            || self
                .cancel_after_files_for_test
                .is_some_and(|limit| searched_files >= limit)
    }

    #[cfg(test)]
    fn cancelled_for_test() -> Self {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        };

        let current = Arc::new(AtomicU64::new(1));
        let generation = SearchGeneration::new(1, Arc::clone(&current));
        current.store(2, Ordering::Release);
        Self::new(generation)
    }

    #[cfg(test)]
    fn cancel_after_files_for_test(limit: usize) -> Self {
        Self {
            generation: None,
            cancel_after_files_for_test: Some(limit),
        }
    }
}
```

In `src/server/files/mod.rs`, replace:

```rust
pub(in crate::server) use self::search::{
    normalize_search_query, search_directory, SearchResponse,
};
```

with:

```rust
pub(in crate::server) use self::search::{
    normalize_search_query, search_directory, SearchCancellation, SearchResponse,
};
```

- [ ] **Step 4: Thread cancellation through search functions**

Change the public async search function signature and spawn call:

```rust
pub(in crate::server) async fn search_directory(
    base_dir: &CanonicalPath,
    raw_query: &str,
    cancellation: SearchCancellation,
) -> std::io::Result<SearchResponse> {
    let query = normalize_search_query(raw_query)?;
    let base_dir = base_dir.clone();

    tokio::task::spawn_blocking(move || search_directory_blocking(&base_dir, &query, cancellation))
        .await
        .map_err(map_search_join_error)?
}
```

Change `search_directory_blocking`:

```rust
fn search_directory_blocking(
    base_dir: &CanonicalPath,
    raw_query: &str,
    cancellation: SearchCancellation,
) -> std::io::Result<SearchResponse> {
    search_directory_with_limits_blocking(
        base_dir,
        raw_query,
        SearchLimits::default(),
        cancellation,
    )
}
```

Change `search_directory_with_limits_blocking`:

```rust
fn search_directory_with_limits_blocking(
    base_dir: &CanonicalPath,
    raw_query: &str,
    limits: SearchLimits,
    cancellation: SearchCancellation,
) -> std::io::Result<SearchResponse> {
```

Inside `search_directory_with_limits_blocking`, after `let mut stats = SearchStats::new();`, add:

```rust
    if cancellation.is_cancelled() {
        return Ok(SearchResponse::from_parts(query, results, limits, stats));
    }
```

At the start of the `for relative in files.into_iter().take(limits.max_files)` loop, add:

```rust
        if cancellation.is_cancelled_after_files(stats.searched_files) {
            break;
        }
```

After the file result loop and before `if results.len() >= limits.max_results`, add:

```rust
        if cancellation.is_cancelled_after_files(stats.searched_files) {
            break;
        }
```

- [ ] **Step 5: Update existing search tests to pass a non-cancelled handle**

For every existing call in `src/server/files/search.rs` tests:

```rust
search_directory(&canonical, "needle")
```

change it to:

```rust
search_directory(&canonical, "needle", SearchCancellation::none())
```

For every existing call:

```rust
search_directory_with_limits_blocking(
    &canonical,
    "needle",
    SearchLimits {
        max_results: 100,
        max_files: 1000,
        max_bytes: 64 * 1024 * 1024,
    },
)
```

change it to:

```rust
search_directory_with_limits_blocking(
    &canonical,
    "needle",
    SearchLimits {
        max_results: 100,
        max_files: 1000,
        max_bytes: 64 * 1024 * 1024,
    },
    SearchCancellation::none(),
)
```

After editing, run:

```bash
rg -n "search_directory\\(&canonical, [^,]+\\)" src/server/files/search.rs
rg -n "search_directory_with_limits_blocking\\(" src/server/files/search.rs
```

Expected: the first command prints no two-argument async calls. The second command prints only calls whose final argument is `SearchCancellation::none()`, `SearchCancellation::cancelled_for_test()`, or `SearchCancellation::cancel_after_files_for_test(1)`.

- [ ] **Step 6: Run search tests**

Run:

```bash
cargo test --lib server::files::search
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/server/files/search.rs
git add src/server/files/mod.rs
git commit -m "feat: ディレクトリ検索に協調キャンセルを追加"
```

## Task 3: Wire Cancellation Through Service Layer

**Files:**
- Modify: `src/server/service.rs`

- [ ] **Step 1: Write failing service tests**

Add these tests inside `#[cfg(test)] mod tests` in `src/server/service.rs`, after the state helper functions.

```rust
    #[tokio::test]
    async fn test_search_ディレクトリモードは検索世代を進める() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let state = create_directory_state(dir.path());

        let response = search(&state, "needle".to_string()).await.unwrap();

        assert_eq!(state.current_search_generation(), 1);
        assert_eq!(response.results.len(), 1);
    }

    #[tokio::test]
    async fn test_search_単一ファイルモードは検索世代を進めない() {
        let (_dir, file_path) = create_markdown_fixture("note.md", "needle");
        let state = create_single_file_state(&file_path);

        let response = search(&state, "needle".to_string()).await.unwrap();

        assert_eq!(state.current_search_generation(), 0);
        assert!(response.results.is_empty());
    }
```

- [ ] **Step 2: Run service tests and verify they fail**

Run:

```bash
cargo test --lib server::service::tests::test_search_ディレクトリモードは検索世代を進める server::service::tests::test_search_単一ファイルモードは検索世代を進めない
```

Expected: the first test FAILS because `service::search` has not advanced the generation. The second may PASS once Task 1 is complete; keep it as a regression test.

- [ ] **Step 3: Import SearchCancellation**

In `src/server/service.rs`, update the `super::files` import list to include `SearchCancellation`:

```rust
    search_directory, ResolvedTarget, RouteTargetRequest, SearchCancellation, SearchResponse,
```

- [ ] **Step 4: Create cancellation handle only for directory mode**

Replace the body of `search` after the single-file early return with:

```rust
    let cancellation = SearchCancellation::new(state.begin_search_generation());

    search_directory(base_dir, &query, cancellation)
        .await
        .map_err(map_search_error)
```

The full function should be:

```rust
pub(super) async fn search(state: &AppState, query: String) -> Result<SearchResponse, ApiError> {
    let query = normalize_search_query(&query).map_err(map_search_error)?;
    let Some(base_dir) = state.mode().directory_canonical() else {
        return Ok(SearchResponse::empty(query));
    };

    let cancellation = SearchCancellation::new(state.begin_search_generation());

    search_directory(base_dir, &query, cancellation)
        .await
        .map_err(map_search_error)
}
```

- [ ] **Step 5: Run service tests**

Run:

```bash
cargo test --lib server::service::tests::test_search_ディレクトリモードは検索世代を進める server::service::tests::test_search_単一ファイルモードは検索世代を進めない
```

Expected: PASS.

- [ ] **Step 6: Run integration search tests**

Run:

```bash
cargo test --test integration_test search
```

Expected: PASS. Existing `/api/search` JSON shape and HTTP error behavior remain unchanged.

- [ ] **Step 7: Commit**

```bash
git add src/server/service.rs
git commit -m "feat: 検索サービスで世代キャンセルを接続"
```

## Task 4: BACKLOG Follow-Up Cleanup

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Edit the directory search BACKLOG item**

Replace the current P2 item text:

```markdown
- [ ] ディレクトリ検索のキャンセル境界と allocation 削減を検討する
  - ファイル: `src/server/files/search.rs`, `src/template/assets/js/directory-search.js`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数の打ち切りも明示されている。一方、連続検索時に古い検索処理をキャンセルする仕組みはなく、`SearchResultItem` の `before/current/after` はマッチごとに `String` を確保する
  - 対応: クライアント検索世代とサーバ側処理の対応、古い検索結果の破棄、`Cow<str>` 化や検索ブロック処理の allocation 削減を、計測結果に基づいて検討する
  - 判断: 検索負荷制御は実装済みで、残件は効率化と古い結果の扱いなので BACKLOG P2 に残す
  - 由来: ディレクトリ検索 blocking 隔離の残余リスク (2026-05-04)
```

with:

```markdown
- [ ] ディレクトリ検索の allocation 削減を計測結果に基づいて検討する
  - ファイル: `src/server/files/search.rs`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数・query 長の打ち切りが明示されている。連続検索時の古い検索処理はサーバ側の検索世代と `SearchCancellation` により、ファイル単位の安全な区切りで協調的に早期終了できる
  - 対応: `SearchResultItem` の `before/current/after` がマッチごとに `String` を確保する点、検索ブロック抽出時の allocation、正規化処理のコストを計測したうえで、`Cow<str>` 化や処理単位の見直しを検討する
  - 判断: キャンセル境界は実装済み。残件は効率化であり、計測なしのマイクロ最適化を避けるため BACKLOG P2 に残す
  - 由来: ディレクトリ検索 blocking 隔離の残余リスク (2026-05-04)、ディレクトリ検索キャンセル境界実装 (2026-05-18)
```

- [ ] **Step 2: Validate BACKLOG wording**

Run:

```bash
rg -n "ディレクトリ検索|SearchCancellation|allocation|由来:" docs/todo/BACKLOG.md
```

Expected: the directory search item mentions implemented cancellation and keeps allocation reduction as the remaining work.

- [ ] **Step 3: Commit**

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: ディレクトリ検索BACKLOGを更新"
```

## Task 5: Full Verification

**Files:**
- Verify only.

- [ ] **Step 1: Run targeted Rust tests**

Run:

```bash
cargo test --lib server::state
cargo test --lib server::files::search
cargo test --lib server::service
cargo test --test integration_test search
```

Expected: all commands PASS.

- [ ] **Step 2: Run full Rust test suite**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 3: Run required repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS for format, lint, and tests.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git status --short
git diff --stat HEAD~4..HEAD
```

Expected: working tree clean after commits. Diff includes only `src/server/state.rs`, `src/server/files/search.rs`, `src/server/service.rs`, and `docs/todo/BACKLOG.md`.

## Security Notes

- Search generation is internal process state and is not accepted from request parameters, serialized into JSON, or exposed to DOM.
- `resolve_file()` remains the read boundary for each searched file. Base containment, hidden path exclusion, Markdown extension checks, regular-file checks, and symlink revalidation stay in place.
- Cancellation does not skip existing query length validation, raw query validation in routes, Host middleware, WebSocket Origin validation, CSP, HTML sanitization, or `innerHTML` sink restrictions.
- Logs must not include full query text, absolute paths, or Markdown body fragments for cancellation decisions.

## Rollback

Revert the commits from Tasks 1-4. No data migration, config change, browser compatibility handling, or generated asset cleanup is required because `SearchResponse` JSON shape and UI behavior remain unchanged.
