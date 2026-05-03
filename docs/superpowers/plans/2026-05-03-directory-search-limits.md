# Directory Search Limits Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `/api/search` report directory-search truncation caused by result count, file count, and total searched bytes, then show a minimal warning in the browser search panel.

**Architecture:** Keep the existing request path and result item shape. Add internal search budget types in `src/server/files/search.rs`, expose new additive fields on `SearchResponse`, and make `content.js` consume those fields defensively. This is a bounded change: no indexing, no server-side cancellation, no worker-pool redesign.

**Tech Stack:** Rust, axum, serde, tokio tests, Playwright TypeScript E2E.

**Estimate:** Human effort 2.5-4 hours. Codex/AI-assisted effort 45-90 minutes including verification.

---

## File Structure

- Modify `src/server/files/search.rs`
  - Owns `SearchResponse`, `SearchLimits`, `SearchStats`, truncation reasons, and the directory search loop.
- Modify `src/server/service.rs`
  - Keeps single-file mode response shape aligned with the new `SearchResponse`.
- Modify `src/template/assets/js/bootstrap.js`
  - Adds initial browser state for directory-search truncation flags.
- Modify `src/template/assets/js/content.js`
  - Reads additive API fields and renders one minimal warning row above directory results.
- Modify `src/server/files/mod.rs` only if test visibility needs `SearchLimits`.
  - Prefer keeping new types private unless tests in another module require narrower visibility.
- Modify `tests/integration_test.rs`
  - Verifies `/api/search` JSON contains the new fields and preserves Host rejection.
- Modify `tests/e2e/document_search.spec.ts`
  - Verifies the minimal warning row for truncated directory search responses.

## Task 1: Add Search Response Budget Model

**Files:**
- Modify: `src/server/files/search.rs`
- Test: `src/server/files/search.rs`

- [ ] **Step 1: Write failing unit tests for default non-truncated response and result limit**

Add these tests at the bottom of `src/server/files/search.rs` inside a new `#[cfg(test)] mod tests` block.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_directory_通常検索は打ち切りなしの統計を返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
        std::fs::write(dir.path().join("other.md"), "# Other").unwrap();

        let response = search_directory(dir.path(), "needle").await.unwrap();

        assert_eq!(response.query, "needle");
        assert!(!response.truncated);
        assert!(response.truncated_reasons.is_empty());
        assert_eq!(response.limits.max_results, 100);
        assert_eq!(response.limits.max_files, 1000);
        assert_eq!(response.limits.max_bytes, 64 * 1024 * 1024);
        assert_eq!(response.searched_files, 2);
        assert_eq!(response.skipped_files, 0);
        assert_eq!(
            response.searched_bytes,
            "# Home\n\nneedle".len() + "# Other".len()
        );
        assert_eq!(response.results.len(), 1);
    }

    #[tokio::test]
    async fn test_search_directory_結果数上限到達を明示する() {
        let dir = tempfile::tempdir().unwrap();
        let markdown = (0..120)
            .map(|index| format!("needle sentence {index}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        std::fs::write(dir.path().join("many.md"), markdown).unwrap();

        let response = search_directory(dir.path(), "needle").await.unwrap();

        assert!(response.truncated);
        assert_eq!(response.truncated_reasons, vec![SearchTruncationReason::ResultLimit]);
        assert_eq!(response.results.len(), 100);
        assert_eq!(response.searched_files, 1);
    }
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
cargo test --all-targets --all-features server::files::search::tests
```

Expected: FAIL because `SearchResponse` has no `truncated`, `truncated_reasons`, `limits`, or `searched_bytes` fields, and `SearchTruncationReason` is not defined.

- [ ] **Step 3: Add response and limit types**

In `src/server/files/search.rs`, replace the old result constant with typed defaults and add serializable public-in-server fields:

```rust
const MAX_SEARCH_RESULTS: usize = 100;
const MAX_SEARCH_FILES: usize = 1000;
const MAX_SEARCH_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(in crate::server) struct SearchLimits {
    pub(in crate::server) max_results: usize,
    pub(in crate::server) max_files: usize,
    pub(in crate::server) max_bytes: usize,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_results: MAX_SEARCH_RESULTS,
            max_files: MAX_SEARCH_FILES,
            max_bytes: MAX_SEARCH_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::server) enum SearchTruncationReason {
    ResultLimit,
    FileLimit,
    ByteLimit,
}

#[derive(Debug, Clone)]
struct SearchStats {
    searched_files: usize,
    skipped_files: usize,
    searched_bytes: usize,
    truncated_reasons: Vec<SearchTruncationReason>,
}

impl SearchStats {
    fn new() -> Self {
        Self {
            searched_files: 0,
            skipped_files: 0,
            searched_bytes: 0,
            truncated_reasons: Vec::new(),
        }
    }

    fn mark_truncated(&mut self, reason: SearchTruncationReason) {
        if !self.truncated_reasons.contains(&reason) {
            self.truncated_reasons.push(reason);
        }
    }

    fn truncated(&self) -> bool {
        !self.truncated_reasons.is_empty()
    }
}
```

Extend `SearchResponse`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(in crate::server) struct SearchResponse {
    pub(in crate::server) query: String,
    pub(in crate::server) results: Vec<SearchResultItem>,
    pub(in crate::server) searched_files: usize,
    pub(in crate::server) skipped_files: usize,
    pub(in crate::server) truncated: bool,
    pub(in crate::server) truncated_reasons: Vec<SearchTruncationReason>,
    pub(in crate::server) limits: SearchLimits,
    pub(in crate::server) searched_bytes: usize,
}

impl SearchResponse {
    pub(in crate::server) fn empty(query: String) -> Self {
        let limits = SearchLimits::default();
        Self {
            query,
            results: Vec::new(),
            searched_files: 0,
            skipped_files: 0,
            truncated: false,
            truncated_reasons: Vec::new(),
            limits,
            searched_bytes: 0,
        }
    }

    fn from_parts(
        query: String,
        results: Vec<SearchResultItem>,
        limits: SearchLimits,
        stats: SearchStats,
    ) -> Self {
        Self {
            query,
            results,
            searched_files: stats.searched_files,
            skipped_files: stats.skipped_files,
            truncated: stats.truncated(),
            truncated_reasons: stats.truncated_reasons,
            limits,
            searched_bytes: stats.searched_bytes,
        }
    }
}
```

- [ ] **Step 4: Apply limits in the search loop**

Update `search_directory` to delegate to a limit-aware helper:

```rust
pub(in crate::server) async fn search_directory(
    base_dir: &Path,
    raw_query: &str,
) -> std::io::Result<SearchResponse> {
    search_directory_with_limits(base_dir, raw_query, SearchLimits::default()).await
}

async fn search_directory_with_limits(
    base_dir: &Path,
    raw_query: &str,
    limits: SearchLimits,
) -> std::io::Result<SearchResponse> {
    let query = raw_query.trim().to_string();
    if query.is_empty() {
        return Ok(SearchResponse::empty(query));
    }

    let files = list_markdown_files(base_dir)?;
    let mut stats = SearchStats::new();
    let mut results = Vec::new();

    if files.len() >= limits.max_files {
        stats.mark_truncated(SearchTruncationReason::FileLimit);
    }

    for relative in files.into_iter().take(limits.max_files) {
        if results.len() >= limits.max_results {
            stats.mark_truncated(SearchTruncationReason::ResultLimit);
            break;
        }

        let file_path = match resolve_file(base_dir, &relative) {
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

        let markdown = match read_markdown_with_limit(&file_path).await {
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

        let markdown_len = markdown.len();
        if stats.searched_bytes.saturating_add(markdown_len) > limits.max_bytes {
            stats.mark_truncated(SearchTruncationReason::ByteLimit);
            break;
        }

        stats.searched_bytes += markdown_len;
        stats.searched_files += 1;

        let blocks = extract_search_blocks(&markdown);
        let file_results = find_matches_for_file(&relative, &blocks, &query);
        for item in file_results {
            results.push(item);
            if results.len() >= limits.max_results {
                stats.mark_truncated(SearchTruncationReason::ResultLimit);
                break;
            }
        }
    }

    Ok(SearchResponse::from_parts(query, results, limits, stats))
}
```

- [ ] **Step 5: Run focused tests and fix compile errors**

Run:

```bash
cargo test --all-targets --all-features server::files::search::tests
```

Expected: PASS. Keep `SearchResponse::empty` as `pub(in crate::server)` because Task 2 uses it from `src/server/service.rs`.

- [ ] **Step 6: Commit Task 1**

```bash
git add src/server/files/search.rs
git commit -m "feat: 検索レスポンスに打ち切り統計を追加"
```

## Task 2: Cover File and Byte Limits and Align Service Empty Response

**Files:**
- Modify: `src/server/files/search.rs`
- Modify: `src/server/service.rs`
- Test: `src/server/files/search.rs`
- Test: `src/server/service.rs`

- [ ] **Step 1: Write failing unit tests for file and byte limits**

Append these tests to the `src/server/files/search.rs` test module:

```rust
    #[tokio::test]
    async fn test_search_directory_ファイル数上限到達を明示する() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..3 {
            std::fs::write(
                dir.path().join(format!("note-{index}.md")),
                format!("needle {index}"),
            )
            .unwrap();
        }

        let response = search_directory_with_limits(
            dir.path(),
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 2,
                max_bytes: 64 * 1024 * 1024,
            },
        )
        .await
        .unwrap();

        assert!(response.truncated);
        assert_eq!(response.truncated_reasons, vec![SearchTruncationReason::FileLimit]);
        assert_eq!(response.searched_files, 2);
        assert_eq!(response.results.len(), 2);
    }

    #[tokio::test]
    async fn test_search_directory_総読込バイト上限到達を明示し超過ファイルは検索しない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle").unwrap();
        std::fs::write(dir.path().join("b.md"), "needle should not be searched").unwrap();

        let response = search_directory_with_limits(
            dir.path(),
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: "needle".len(),
            },
        )
        .await
        .unwrap();

        assert!(response.truncated);
        assert_eq!(response.truncated_reasons, vec![SearchTruncationReason::ByteLimit]);
        assert_eq!(response.searched_files, 1);
        assert_eq!(response.searched_bytes, "needle".len());
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].file, "a.md");
    }
```

- [ ] **Step 2: Update service tests for single-file empty response**

In `src/server/service.rs`, extend `test_search_単一ファイルモードでは空結果を返す`:

```rust
        assert!(!response.truncated);
        assert!(response.truncated_reasons.is_empty());
        assert_eq!(response.limits.max_results, 100);
        assert_eq!(response.limits.max_files, 1000);
        assert_eq!(response.limits.max_bytes, 64 * 1024 * 1024);
        assert_eq!(response.searched_bytes, 0);
```

- [ ] **Step 3: Run tests and verify failures**

Run:

```bash
cargo test --all-targets --all-features server::files::search::tests
cargo test --all-targets --all-features server::service::tests::test_search_単一ファイルモードでは空結果を返す
```

Expected: The service test may fail because `service::search` still builds `SearchResponse` manually. The file/byte tests should pass if Task 1 already implemented `search_directory_with_limits`.

- [ ] **Step 4: Replace manual empty response in `service::search`**

In `src/server/service.rs`, replace the single-file-mode return block:

```rust
    let Some(base_dir) = state.mode().directory() else {
        return Ok(SearchResponse::empty(query.trim().to_string()));
    };
```

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test --all-targets --all-features server::files::search::tests server::service::tests::test_search_単一ファイルモードでは空結果を返す
```

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

```bash
git add src/server/files/search.rs src/server/service.rs
git commit -m "test: 検索上限の境界条件を固定"
```

## Task 3: Expose New API Fields Through Integration Tests

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Extend existing `/api/search` success test**

Find the integration test that asserts `searched_files == 2` for `/api/search?q=alpha%20note`. Add these assertions after existing `searched_files` and `skipped_files` checks:

```rust
    assert_eq!(json["truncated"].as_bool().unwrap(), false);
    assert!(json["truncated_reasons"].as_array().unwrap().is_empty());
    assert_eq!(json["limits"]["max_results"].as_u64().unwrap(), 100);
    assert_eq!(json["limits"]["max_files"].as_u64().unwrap(), 1000);
    assert_eq!(json["limits"]["max_bytes"].as_u64().unwrap(), 64 * 1024 * 1024);
    assert!(json["searched_bytes"].as_u64().unwrap() > 0);
```

- [ ] **Step 2: Add integration test for result truncation JSON**

Add this test near the existing `/api/search` integration tests:

```rust
#[tokio::test]
async fn test_ディレクトリモード_api_searchは結果数打ち切りをjsonで返す() {
    let dir = tempfile::tempdir().unwrap();
    let markdown = (0..120)
        .map(|index| format!("alpha note {index}."))
        .collect::<Vec<_>>()
        .join("\n\n");
    std::fs::write(dir.path().join("many.md"), markdown).unwrap();

    let state = build_dir_state(dir.path());
    let addr = spawn_test_server(state).await;
    let resp = reqwest::get(format!("http://{}/api/search?q=alpha%20note", addr))
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["truncated"].as_bool().unwrap(), true);
    assert_eq!(json["truncated_reasons"].as_array().unwrap().len(), 1);
    assert_eq!(json["truncated_reasons"][0].as_str().unwrap(), "result_limit");
    assert_eq!(json["results"].as_array().unwrap().len(), 100);
}
```

- [ ] **Step 3: Run integration search tests**

Run:

```bash
cargo test --all-targets --all-features --test integration_test search
```

Expected: PASS.

- [ ] **Step 4: Commit Task 3**

```bash
git add tests/integration_test.rs
git commit -m "test: 検索APIの打ち切りレスポンスを固定"
```

## Task 4: Render Minimal UI Warning for Truncated Directory Search

**Files:**
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/content.js`
- Test: `tests/e2e/document_search.spec.ts`

- [ ] **Step 1: Add failing E2E test for UI warning**

Append this test to `tests/e2e/document_search.spec.ts` near other mocked `/api/search` tests:

```ts
test('ディレクトリモードでは検索打ち切り警告を結果一覧の先頭に表示する', async ({ page }) => {
  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'alpha',
        results: [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'Alpha result is visible.',
            after: ''
          }
        ],
        searched_files: 1,
        skipped_files: 0,
        truncated: true,
        truncated_reasons: ['result_limit'],
        limits: {
          max_results: 100,
          max_files: 1000,
          max_bytes: 67108864
        },
        searched_bytes: 1024
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha result is visible.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha');

  await expect(page.locator('#document-search-results')).toContainText('上限により一部のみ表示しています。');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(1);
  await expect(page.locator('#document-search-results .document-search-result').first())
    .toContainText('Alpha result is visible.');
});
```

- [ ] **Step 2: Run E2E test and verify it fails**

Run:

```bash
npx playwright test tests/e2e/document_search.spec.ts -g "検索打ち切り警告"
```

Expected: FAIL because no warning row is rendered.

- [ ] **Step 3: Initialize truncation state**

In `src/template/assets/js/bootstrap.js`, add these fields under `search`:

```js
      currentDirectoryTruncated: false,
      currentDirectoryTruncatedReasons: [],
```

- [ ] **Step 4: Store response truncation fields and reset them on clear/error**

In `runDirectorySearch(query)` inside `src/template/assets/js/content.js`, reset before fetch:

```js
  appContext.search.currentDirectoryTruncated = false;
  appContext.search.currentDirectoryTruncatedReasons = [];
```

In the success handler after `currentDirectorySkippedFiles`:

```js
    appContext.search.currentDirectoryTruncated = data.truncated === true ||
      (Array.isArray(data.truncated_reasons) && data.truncated_reasons.length > 0);
    appContext.search.currentDirectoryTruncatedReasons = Array.isArray(data.truncated_reasons)
      ? data.truncated_reasons.slice()
      : [];
```

In the catch handler and in query-clear paths that already reset `currentDirectorySkippedFiles`, also set:

```js
    appContext.search.currentDirectoryTruncated = false;
    appContext.search.currentDirectoryTruncatedReasons = [];
```

Apply the same two reset assignments in `clearDocumentSearch()` and in the branch of `setDocumentSearchQuery()` that handles an empty query.

- [ ] **Step 5: Render warning row above directory results**

Add this helper near `createDocumentSearchEmptyState` or near `renderDirectorySearchResults`:

```js
function createDirectorySearchTruncatedState() {
  var item = document.createElement('div');
  item.className = 'document-search-empty';
  item.textContent = '上限により一部のみ表示しています。';
  return item;
}
```

In `renderDirectorySearchResults()`, after the no-query/loading/error/empty checks and before iterating results:

```js
  if (appContext.search.currentDirectoryTruncated) {
    appContext.elements.documentSearchResultsEl.appendChild(createDirectorySearchTruncatedState());
  }
```

This intentionally reuses `document-search-empty` so no CSS change is needed.

- [ ] **Step 6: Run focused E2E test**

Run:

```bash
npx playwright test tests/e2e/document_search.spec.ts -g "検索打ち切り警告"
```

Expected: PASS.

- [ ] **Step 7: Commit Task 4**

```bash
git add src/template/assets/js/bootstrap.js src/template/assets/js/content.js tests/e2e/document_search.spec.ts
git commit -m "feat: ディレクトリ検索打ち切りをUIに表示"
```

## Task 5: Full Verification and Completion Notes

**Files:**
- No planned source changes.
- Use failures to decide whether earlier tasks need small corrective edits.

- [ ] **Step 1: Run Rust verification**

Run:

```bash
./verify.sh
```

Expected: PASS for formatting, clippy, and Rust tests.

- [ ] **Step 2: Run targeted E2E verification**

Run:

```bash
npx playwright test tests/e2e/document_search.spec.ts
```

Expected: PASS for document search E2E coverage.

- [ ] **Step 3: Inspect final diff**

Run:

```bash
git status --short
git log --oneline -5
```

Expected: clean working tree after commits, with the latest commits matching Tasks 1-4.

- [ ] **Step 4: Prepare completion report**

Include:

- Changed files with reason and rough line impact.
- Dependent files affected by the API shape.
- Verification commands and results.
- Residual risks: list truncation precision still depends on `list_markdown_files()`, byte-limit overflow file is temporarily read before being discarded, server-side cancellation remains out of scope.

## Self-Review

- Spec coverage: Tasks 1-2 implement result/file/byte budgets and additive response fields. Task 3 covers API JSON. Task 4 covers UI minimum warning and backward-compatible JS parsing. Task 5 covers required verification and residual risk reporting.
- Placeholder scan: No unresolved placeholder sections remain.
- Type consistency: `SearchLimits`, `SearchStats`, `SearchTruncationReason`, and `SearchResponse` field names are consistent across Rust, JSON, and JS plan steps.
