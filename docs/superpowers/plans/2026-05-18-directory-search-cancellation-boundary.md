# Directory Search Cancellation Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add cooperative server-side cancellation boundaries for stale directory searches without changing the search API response shape or UI display behavior. Cancellation is scoped to a validated browser search client ID so separate tabs/clients do not cancel one another.

**Architecture:** Store process-local search generation counters in `AppState` as a bounded client registry, issue a new generation only for valid client IDs, and pass a `SearchCancellation` handle into the blocking search core. Missing or invalid client IDs use a no-cancellation fallback; over-capacity valid IDs evict the least recently used entry and are accepted. The blocking core checks cancellation only at safe processing boundaries and returns the partial `SearchResponse` instead of surfacing a user-facing error.

**Tech Stack:** Rust, axum service layer, Tokio `spawn_blocking`, `Mutex<HashMap<_, Arc<AtomicU64>>>`, browser fetch headers, existing Rust unit tests and E2E tests.

---

## File Structure

- Modify `src/server/state.rs`
  - Owns `AppState`; add client-scoped search generation counters, ID validation, bounded client storage, and tests for generation issuance.
- Modify `src/server/files/search.rs`
  - Owns directory search internals; add `SearchCancellation`, pass it through the async wrapper and blocking core, and test cooperative cancellation.
- Modify `src/server/service.rs`
  - Owns API service orchestration; issue generations only for directory mode with valid client IDs and pass cancellation into `search_directory`.
- Modify `src/server/routes.rs`
  - Extract `X-Markdown-View-Search-Client` from `/api/search` and pass it to service without logging it.
- Modify `src/template/assets/js/bootstrap.js`
  - Generate a stable per-page search client ID for the browser tab.
- Modify `src/template/assets/js/directory-search.js`
  - Send the search client ID as an `/api/search` request header.
- Modify `tests/e2e/document_search.spec.ts`
  - Confirm the header is present and stable across requests from the same page.
- Modify `docs/todo/BACKLOG.md`
  - After implementation passes, mark the cancellation-boundary portion as addressed and leave allocation reduction as a remaining long-term candidate.

## 2026-05-18 Review Follow-up Delta

The initial plan below used one process-wide generation counter. Final-review feedback found that this lets one tab/client cancel another tab/client and can produce stale server responses that the receiving UI cannot reject with its own generation check.

Apply these changes instead of the process-wide counter steps:

- `AppState::next_search_generation_for_client(client_id: Option<&str>) -> Option<(u64, Arc<AtomicU64>)>`.
- Valid client IDs are 1-64 bytes and limited to ASCII letters, digits, `-`, and `_`.
- Store at most 64 client IDs. If a new valid ID would exceed the cap, evict the least recently used entry and accept the new ID.
- Treat missing or invalid client IDs as no-cancellation fallback with `SearchCancellation::never_cancelled()`.
- Browser UI stores one tab-local ID in `sessionStorage` as `ctx.search.directorySearchClientId` and sends it with `X-Markdown-View-Search-Client`.
- Keep `SearchResponse` JSON shape and truncation semantics unchanged.
- Do not log client ID, query, path, or body as part of this flow.
- Add tests for same-client cancellation generation, different-client isolation, invalid/missing fallback, LRU eviction on over-capacity valid IDs, and mid-search cancellation before later files.

## Task 1: Add Search Generation State

**Files:**
- Modify: `src/server/state.rs`

- [ ] **Step 1: Write failing tests for generation behavior**

Add these tests near the existing `AppState` tests in `src/server/state.rs`:

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
    fn test_app_state_next_search_generationは世代を進める() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);

        assert_eq!(state.next_search_generation(), 1);
        assert_eq!(state.next_search_generation(), 2);
        assert_eq!(state.current_search_generation(), 2);
    }

    #[test]
    fn test_app_state_search_generation_handleは同じ世代を共有する() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
        let mode = AppMode::new_single_file(&file_path).unwrap();
        let (tx, _rx) = broadcast::channel(16);
        let state = AppState::new_with_tokio_memo_fs(mode, false, None, tx);
        let generation = state.search_generation();

        assert_eq!(generation.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(state.next_search_generation(), 1);
        assert_eq!(generation.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
```

- [ ] **Step 2: Run the focused state tests and verify they fail**

Run:

```bash
cargo test --all-targets --all-features app_state_search_generation -- --nocapture
```

Expected: compile failure because `AppState::current_search_generation`, `AppState::next_search_generation`, and `AppState::search_generation` do not exist.

- [ ] **Step 3: Add the minimal generation implementation**

Update the imports and `AppState` definition in `src/server/state.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
```

Add a field to `AppState`:

```rust
    search_generation: Arc<AtomicU64>,
```

Initialize it in `AppState::new`:

```rust
        Self {
            syntax_css: syntax_theme_css(theme.as_deref()),
            mode,
            dark_mode,
            tx,
            memo_fs,
            search_generation: Arc::new(AtomicU64::new(0)),
        }
```

Add these methods to `impl AppState`:

```rust
    pub(crate) fn next_search_generation(&self) -> u64 {
        self.search_generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub(crate) fn current_search_generation(&self) -> u64 {
        self.search_generation.load(Ordering::Relaxed)
    }

    pub(crate) fn search_generation(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.search_generation)
    }
```

- [ ] **Step 4: Run the focused state tests and verify they pass**

Run:

```bash
cargo test --all-targets --all-features app_state_search_generation -- --nocapture
```

Expected: the three new tests pass.

- [ ] **Step 5: Commit Task 1**

Run:

```bash
git add src/server/state.rs
git commit -m "feat: 検索世代カウンタをAppStateへ追加"
```

## Task 2: Add SearchCancellation and Blocking-Core Boundaries

**Files:**
- Modify: `src/server/files/search.rs`

- [ ] **Step 1: Write failing tests for cancellation value behavior and pre-loop cancellation**

Add these imports to the test module in `src/server/files/search.rs`:

```rust
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
```

Add these tests near the existing `search_directory` tests:

```rust
    #[test]
    fn test_search_cancellationは新しい世代を検知する() {
        let generation = Arc::new(AtomicU64::new(1));
        let cancellation = SearchCancellation::new(1, Arc::clone(&generation));

        assert!(!cancellation.is_cancelled());

        generation.store(2, Ordering::Relaxed);

        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn test_search_directory_キャンセル済みならファイル処理へ進まない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "needle").unwrap();
        let canonical = canonical_of(dir.path());
        let generation = Arc::new(AtomicU64::new(2));
        let cancellation = SearchCancellation::new(1, Arc::clone(&generation));

        let response = search_directory_with_limits_and_cancellation_blocking(
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
```

- [ ] **Step 2: Run the focused search tests and verify they fail**

Run:

```bash
cargo test --all-targets --all-features search_cancellation -- --nocapture
```

Expected: compile failure because `SearchCancellation` and `search_directory_with_limits_and_cancellation_blocking` do not exist.

- [ ] **Step 3: Implement `SearchCancellation`**

Add imports near the top of `src/server/files/search.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
```

Add this type near `SearchLimits`:

```rust
#[derive(Debug, Clone)]
pub(in crate::server) struct SearchCancellation {
    generation: u64,
    current_generation: Arc<AtomicU64>,
}

impl SearchCancellation {
    pub(in crate::server) fn new(generation: u64, current_generation: Arc<AtomicU64>) -> Self {
        Self {
            generation,
            current_generation,
        }
    }

    fn never_cancelled() -> Self {
        Self {
            generation: 0,
            current_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.current_generation.load(Ordering::Relaxed) > self.generation
    }
}
```

- [ ] **Step 4: Add the cancellation-aware blocking core**

Replace the current `search_directory_blocking` and `search_directory_with_limits_blocking` definitions with this structure:

```rust
fn search_directory_blocking(
    base_dir: &CanonicalPath,
    raw_query: &str,
    cancellation: SearchCancellation,
) -> std::io::Result<SearchResponse> {
    search_directory_with_limits_and_cancellation_blocking(
        base_dir,
        raw_query,
        SearchLimits::default(),
        cancellation,
    )
}

fn search_directory_with_limits_blocking(
    base_dir: &CanonicalPath,
    raw_query: &str,
    limits: SearchLimits,
) -> std::io::Result<SearchResponse> {
    search_directory_with_limits_and_cancellation_blocking(
        base_dir,
        raw_query,
        limits,
        SearchCancellation::never_cancelled(),
    )
}

fn search_directory_with_limits_and_cancellation_blocking(
    base_dir: &CanonicalPath,
    raw_query: &str,
    limits: SearchLimits,
    cancellation: SearchCancellation,
) -> std::io::Result<SearchResponse> {
    let query = normalize_search_query(raw_query)?;
    if query.is_empty() {
        return Ok(SearchResponse::empty(query));
    }

    let files =
        list_markdown_files_from_canonical_base(base_dir, limits.max_files.saturating_add(1))?;
    let base_path = base_dir.as_path();
    let mut results = Vec::new();
    let mut stats = SearchStats::new();

    if files.len() > limits.max_files {
        stats.mark_truncated(SearchTruncationReason::File);
    }

    if cancellation.is_cancelled() {
        return Ok(SearchResponse::from_parts(query, results, limits, stats));
    }

    for relative in files.into_iter().take(limits.max_files) {
        if cancellation.is_cancelled() {
            break;
        }

        let file_path = match resolve_file(base_path, &relative) {
            Ok(file_path) => file_path,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] 検索対象ファイル解決失敗（スキップ）: {} ({})",
                    relative,
                    error
                );
                stats.skipped_files += 1;
                continue;
            }
        };

        let markdown = match read_markdown_with_limit_blocking(&file_path) {
            Ok(markdown) => markdown,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] 検索対象ファイル読込失敗（スキップ）: {} ({})",
                    relative,
                    error
                );
                stats.skipped_files += 1;
                continue;
            }
        };

        if stats.searched_bytes.saturating_add(markdown.len()) > limits.max_bytes {
            stats.mark_truncated(SearchTruncationReason::Byte);
            break;
        }

        stats.searched_files += 1;
        stats.searched_bytes += markdown.len();
        let blocks = extract_search_blocks(&markdown);
        let file_results = find_matches_for_file(&relative, &blocks, &query);
        for item in file_results {
            results.push(item);
            if results.len() >= limits.max_results {
                stats.mark_truncated(SearchTruncationReason::Result);
                break;
            }
        }

        if cancellation.is_cancelled() || results.len() >= limits.max_results {
            break;
        }
    }

    Ok(SearchResponse::from_parts(query, results, limits, stats))
}
```

- [ ] **Step 5: Update the async wrapper temporarily with a never-cancelled handle**

In `search_directory`, update the `spawn_blocking` call:

```rust
    tokio::task::spawn_blocking(move || {
        search_directory_blocking(&base_dir, &query, SearchCancellation::never_cancelled())
    })
    .await
    .map_err(map_search_join_error)?
```

This keeps existing call sites compiling until Task 3 wires real generations from `service.rs`.

- [ ] **Step 6: Run focused cancellation tests and existing search limit tests**

Run:

```bash
cargo test --all-targets --all-features search_cancellation -- --nocapture
cargo test --all-targets --all-features search_directory_ -- --nocapture
```

Expected: cancellation tests pass, and existing search directory tests continue to pass.

- [ ] **Step 7: Commit Task 2**

Run:

```bash
git add src/server/files/search.rs
git commit -m "feat: ディレクトリ検索キャンセル境界を追加"
```

## Task 3: Wire Service-Level Generations

**Files:**
- Modify: `src/server/files/search.rs`
- Modify: `src/server/service.rs`

- [ ] **Step 1: Write service tests for directory and single-file generation behavior**

Add these tests to the `#[cfg(test)] mod tests` in `src/server/service.rs`:

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
        let (_dir, file_path) = create_markdown_fixture("README.md", "needle");
        let state = create_single_file_state(&file_path);

        let response = search(&state, "needle".to_string()).await.unwrap();

        assert_eq!(state.current_search_generation(), 0);
        assert!(response.results.is_empty());
    }
```

Add this helper inside the service test module:

```rust
    fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }
```

- [ ] **Step 2: Run focused service tests and verify the directory test fails**

Run:

```bash
cargo test --all-targets --all-features test_search_ディレクトリモードは検索世代を進める -- --nocapture
cargo test --all-targets --all-features test_search_単一ファイルモードは検索世代を進めない -- --nocapture
```

Expected: the directory-mode generation test fails because `service::search` has not yet advanced the generation. The single-file generation test passes because the generation remains at 0.

- [ ] **Step 3: Change `search_directory` to require cancellation**

In `src/server/files/search.rs`, change the public async function signature and wrapper:

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

- [ ] **Step 4: Wire generation issuance in `service::search`**

Update the `use super::files::{...}` list in `src/server/service.rs` to include `SearchCancellation`.

Replace the call in `service::search` with:

```rust
    let generation = state.next_search_generation();
    let cancellation = SearchCancellation::new(generation, state.search_generation());

    search_directory(base_dir, &query, cancellation)
        .await
        .map_err(map_search_error)
```

- [ ] **Step 5: Update existing direct `search_directory` tests**

For existing tests that call `search_directory(&canonical, "...").await`, update them to pass a never-cancelled handle. Because `never_cancelled()` is private, add this test helper in the `search.rs` test module:

```rust
    fn never_cancelled() -> SearchCancellation {
        SearchCancellation::new(0, Arc::new(AtomicU64::new(0)))
    }
```

Then update direct calls as:

```rust
        let response = search_directory(&canonical, "needle", never_cancelled())
            .await
            .unwrap();
```

For the long-query async entrance test:

```rust
        let error = search_directory(&canonical, &query, never_cancelled())
            .await
            .unwrap_err();
```

- [ ] **Step 6: Run search and service tests**

Run:

```bash
cargo test --all-targets --all-features search_directory_ -- --nocapture
cargo test --all-targets --all-features test_search_ディレクトリモードは検索世代を進める -- --nocapture
cargo test --all-targets --all-features test_search_単一ファイルモードは検索世代を進めない -- --nocapture
```

Expected: all focused tests pass.

- [ ] **Step 7: Commit Task 3**

Run:

```bash
git add src/server/files/search.rs src/server/service.rs
git commit -m "feat: 検索APIに世代キャンセルを接続"
```

## Task 4: Update Backlog and Run Verification

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Update the backlog item to reflect completed cancellation work**

In `docs/todo/BACKLOG.md`, replace the P2 item title and body for directory search with:

```markdown
- [ ] ディレクトリ検索の allocation 削減を計測ベースで検討する
  - ファイル: `src/server/files/search.rs`, `src/template/assets/js/directory-search.js`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数の打ち切りも明示されている。連続検索時の古いレスポンスはクライアント検索世代で破棄され、サーバ側の古い検索処理も検索世代による協調的キャンセル境界で早期終了できる。一方、`SearchResultItem` の `before/current/after` はマッチごとに `String` を確保する
  - 対応: `Cow<str>` 化や検索ブロック処理の allocation 削減を、計測結果に基づいて検討する
  - 判断: 検索負荷制御とキャンセル境界は実装済みで、残件は効率化なので BACKLOG P2 に残す
  - 由来: ディレクトリ検索 blocking 隔離の残余リスク (2026-05-04)、ディレクトリ検索キャンセル境界 (2026-05-18)
```

- [ ] **Step 2: Run full Rust tests**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: all Rust tests pass.

- [ ] **Step 3: Run full repository verification**

Run:

```bash
./verify.sh
```

Expected: format, lint, and tests pass.

- [ ] **Step 4: Inspect the final diff**

Run:

```bash
git diff --stat HEAD
git diff -- src/server/state.rs src/server/files/search.rs src/server/service.rs docs/todo/BACKLOG.md
```

Expected: the diff is limited to search generation state, cooperative search cancellation, service wiring, tests, and backlog wording.

- [ ] **Step 5: Commit Task 4**

Run:

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: ディレクトリ検索残件を更新"
```

## Final Verification

- [ ] **Step 1: Confirm working tree state**

Run:

```bash
git status --short --branch
```

Expected: clean working tree, branch ahead by the implementation commits.

- [ ] **Step 2: Confirm no API response-shape changes were introduced**

Run:

```bash
rg -n "cancelled|SearchResponse|serde\\(rename|currentDirectory" src/server/files/search.rs src/template/assets/js/directory-search.js
```

Expected: no `cancelled` field is added to `SearchResponse`, and `directory-search.js` has no required contract change.

- [ ] **Step 3: Record residual risk in the completion report**

Mention these residual risks in the final report:

- Cooperative cancellation does not interrupt a file read or Markdown parse already in progress.
- Allocation reduction for `SearchResultItem` remains in `BACKLOG.md`.
- No E2E was added because the UI contract is unchanged; existing search E2E remains the UI regression coverage.
