# Directory Search Blocking Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move directory search traversal, file reads, Markdown parsing, and match extraction into a request-scoped `spawn_blocking` task while preserving the existing `/api/search` API response.

**Architecture:** Keep `search_directory()` as the async boundary used by `service::search()`. Add a synchronous search core in `src/server/files/search.rs`, call it from `spawn_blocking`, and add a search-local synchronous read helper with the same 10MB double-check contract as the async reader. Existing JSON fields, truncation semantics, route behavior, and UI remain unchanged.

**Tech Stack:** Rust, tokio `spawn_blocking`, std filesystem I/O, pulldown-cmark, axum integration tests.

---

## File Structure

- Modify `src/server/files/search.rs`
  - Owns the async wrapper, synchronous search core, synchronous read helper, join error mapping, and unit tests.
- Keep `src/server/service.rs` unchanged
  - It continues calling `search_directory(base_dir, &query).await`.
- Keep `tests/integration_test.rs` unchanged unless an existing assertion breaks
  - Existing `/api/search` JSON compatibility and Host rejection tests cover the public API.
- Keep `tests/e2e/document_search.spec.ts` unchanged
  - UI behavior does not change.

## Task 1: Add Search-Local Synchronous Markdown Reader

**Files:**
- Modify: `src/server/files/search.rs`
- Test: `src/server/files/search.rs`

- [ ] **Step 1: Write failing tests for the synchronous reader**

Add these tests inside the existing `#[cfg(test)] mod tests` block in `src/server/files/search.rs`.

```rust
    #[test]
    fn test_read_markdown_with_limit_blocking_utf8本文を読む() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "見出し\n\nneedle").unwrap();

        let markdown = read_markdown_with_limit_blocking(&path).unwrap();

        assert_eq!(markdown, "見出し\n\nneedle");
    }

    #[test]
    fn test_read_markdown_with_limit_blocking_utf8以外はinvalid_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.md");
        std::fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();

        let error = read_markdown_with_limit_blocking(&path).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --all-targets --all-features server::files::search::tests::test_read_markdown_with_limit_blocking
```

Expected: FAIL because `read_markdown_with_limit_blocking` is not defined.

- [ ] **Step 3: Add the synchronous reader implementation**

At the top of `src/server/files/search.rs`, replace the current imports:

```rust
use std::ops::Range;
use std::path::Path;
```

with:

```rust
use std::io::Read;
use std::ops::Range;
use std::path::Path;
```

Replace this import:

```rust
use super::content::read_markdown_with_limit;
```

with:

```rust
use super::content::MAX_FILE_SIZE;
```

Add this helper after `search_directory_with_limits` for now. It will remain in the file after Task 2 moves the search core to blocking mode.

```rust
fn read_markdown_with_limit_blocking(file_path: &Path) -> std::io::Result<String> {
    let metadata = std::fs::metadata(file_path)?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err(file_too_large_error());
    }

    let file = std::fs::File::open(file_path)?;
    let mut limited_reader = file.take(MAX_FILE_SIZE + 1);
    let mut buffer = Vec::new();
    limited_reader.read_to_end(&mut buffer)?;
    if buffer.len() as u64 > MAX_FILE_SIZE {
        return Err(file_too_large_error());
    }

    String::from_utf8(buffer).map_err(|error| {
        tracing::warn!(
            "[markdown-view] UTF-8デコード失敗: バイトオフセット {} で無効なバイト列",
            error.utf8_error().valid_up_to()
        );
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "ファイルがUTF-8テキストではありません",
        )
    })
}

fn file_too_large_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "ファイルサイズが上限（10MB）を超えています",
    )
}
```

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
cargo test --all-targets --all-features server::files::search::tests::test_read_markdown_with_limit_blocking
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add src/server/files/search.rs
git commit -m "test: 検索用同期Markdown読込を追加"
```

## Task 2: Move Directory Search Core Into Blocking Task

**Files:**
- Modify: `src/server/files/search.rs`
- Test: `src/server/files/search.rs`

- [ ] **Step 1: Add a regression assertion for the async wrapper**

The existing `test_search_directory_通常検索は打ち切りなしの統計を返す` already calls the public async wrapper. Add this assertion to that test after `assert_eq!(response.results.len(), 1);`.

```rust
        assert_eq!(response.results[0].file, "README.md");
```

- [ ] **Step 2: Run the focused test before refactor**

Run:

```bash
cargo test --all-targets --all-features server::files::search::tests::test_search_directory_通常検索は打ち切りなしの統計を返す
```

Expected: PASS. This establishes the wrapper behavior before the refactor.

- [ ] **Step 3: Replace the async search implementation with a blocking wrapper and synchronous core**

In `src/server/files/search.rs`, replace the existing `search_directory` and `search_directory_with_limits` functions with this code:

```rust
/// ディレクトリ内のMarkdownを横断検索する。
pub(in crate::server) async fn search_directory(
    base_dir: &Path,
    raw_query: &str,
) -> std::io::Result<SearchResponse> {
    let base_dir = base_dir.to_path_buf();
    let raw_query = raw_query.to_owned();

    tokio::task::spawn_blocking(move || search_directory_blocking(&base_dir, &raw_query))
        .await
        .map_err(map_search_join_error)?
}

fn search_directory_blocking(
    base_dir: &Path,
    raw_query: &str,
) -> std::io::Result<SearchResponse> {
    search_directory_with_limits_blocking(base_dir, raw_query, SearchLimits::default())
}

fn search_directory_with_limits_blocking(
    base_dir: &Path,
    raw_query: &str,
    limits: SearchLimits,
) -> std::io::Result<SearchResponse> {
    let query = raw_query.trim().to_string();
    if query.is_empty() {
        return Ok(SearchResponse::empty(query));
    }

    let files = list_markdown_files_with_limit(base_dir, limits.max_files.saturating_add(1))?;
    let mut results = Vec::new();
    let mut stats = SearchStats::new();

    if files.len() > limits.max_files {
        stats.mark_truncated(SearchTruncationReason::File);
    }

    for relative in files.into_iter().take(limits.max_files) {
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

        if results.len() >= limits.max_results {
            break;
        }
    }

    Ok(SearchResponse::from_parts(query, results, limits, stats))
}
```

Add this join error mapper after `search_directory_with_limits_blocking`:

```rust
fn map_search_join_error(error: tokio::task::JoinError) -> std::io::Error {
    if error.is_panic() {
        tracing::error!(
            "[markdown-view] ディレクトリ検索blockingタスクがpanicしました: {}",
            error
        );
    } else {
        tracing::warn!(
            "[markdown-view] ディレクトリ検索blockingタスクのjoinエラー: {}",
            error
        );
    }

    std::io::Error::other("ディレクトリ検索タスクの実行に失敗しました")
}
```

- [ ] **Step 4: Update limit-injection tests to call the synchronous core**

In `src/server/files/search.rs`, replace these calls:

```rust
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
```

with:

```rust
        let response = search_directory_with_limits_blocking(
            dir.path(),
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 2,
                max_bytes: 64 * 1024 * 1024,
            },
        )
        .unwrap();
```

Make the same replacement in these tests:

```text
test_search_directory_ファイル数上限到達を明示する
test_search_directory_ファイル数が上限ちょうどなら打ち切り扱いにしない
test_search_directory_総読込バイト上限到達を明示し超過ファイルは検索しない
```

- [ ] **Step 5: Run focused search tests**

Run:

```bash
cargo test --all-targets --all-features server::files::search::tests
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add src/server/files/search.rs
git commit -m "refactor: ディレクトリ検索をblockingタスクへ隔離"
```

## Task 3: Fix Compile and Clippy Details

**Files:**
- Modify: `src/server/files/search.rs`
- Test: `src/server/files/search.rs`

- [ ] **Step 1: Run clippy for the touched target**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: FAIL only if imports, async test annotations, or `std::io::Error::other` compatibility need adjustment.

- [ ] **Step 2: Remove stale async import and stale `.await` usage**

If clippy or compile reports `read_markdown_with_limit` is unused or unresolved, ensure the import block in `src/server/files/search.rs` contains these lines:

```rust
use std::io::Read;
use std::ops::Range;
use std::path::Path;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::catalog::list_markdown_files_with_limit;
use super::content::MAX_FILE_SIZE;
use super::resolve::resolve_file;
use crate::markdown::{markdown_options, MarkdownProfile};
```

If compile reports `.await` is used on `search_directory_with_limits_blocking`, remove the `.await` from that call and keep the surrounding `.unwrap()`.

- [ ] **Step 3: Run clippy again**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Commit compile cleanup**

Run this only when Step 2 changed files:

```bash
git add src/server/files/search.rs
git commit -m "chore: 検索blocking隔離のlintを整える"
```

If Step 2 made no changes, record no commit for this task.

## Task 4: Verify Public API Compatibility

**Files:**
- Test: `tests/integration_test.rs`
- Test: `tests/e2e/document_search.spec.ts`
- Test: `src/server/files/search.rs`

- [ ] **Step 1: Run search unit tests**

Run:

```bash
cargo test --all-targets --all-features server::files::search::tests
```

Expected: PASS.

- [ ] **Step 2: Run `/api/search` integration tests**

Run:

```bash
cargo test --all-targets --all-features api_search
```

Expected: PASS.

- [ ] **Step 3: Run the full repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 4: Commit verification-only test adjustments**

Run this only if Task 4 required changes to tests:

```bash
git add tests/integration_test.rs tests/e2e/document_search.spec.ts
git commit -m "test: 検索blocking隔離の公開契約を固定"
```

If no test files changed, record no commit for this task.

## Task 5: Update TODO Tracking

**Files:**
- Modify: `docs/todo/TODO.md`
- Test: `docs/todo/TODO.md`

- [ ] **Step 1: Update the High Priority search item**

In `docs/todo/TODO.md`, change the `ディレクトリ検索の負荷制御をサーバ側に追加する` item from unchecked to checked only after `./verify.sh` passes.

Replace its `現状` and `対応` paragraphs with text that records completion and residual risks:

```markdown
  - 現状: PR #121 で検索結果数・検索対象ファイル数・総読込 byte 数の打ち切りが API/UI に明示され、今回の blocking 隔離でディレクトリ走査、ファイル読込、Markdown パース、検索一致抽出をリクエスト単位の `spawn_blocking` 内へ移した
  - 対応: 完了。残る改善候補は、クライアント世代と対応するサーバ側キャンセルまたは古い検索の破棄、検索結果コンテキストの allocation 削減、検索インデックス導入の必要性評価として BACKLOG.md へ分離する
```

- [ ] **Step 2: Add residual risks to BACKLOG**

Add this item under `docs/todo/BACKLOG.md` section `P2: 保守性・局所回帰検知`:

```markdown
- [ ] ディレクトリ検索のキャンセル境界と allocation 削減を検討する
  - ファイル: `src/server/files/search.rs`, `src/template/assets/js/content.js`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数の打ち切りも明示されている。一方、連続検索時に古い検索処理をキャンセルする仕組みはなく、`SearchResultItem` の `before/current/after` はマッチごとに `String` を確保する
  - 対応: クライアント検索世代とサーバ側処理の対応、古い検索結果の破棄、`Cow<str>` 化や検索ブロック処理の allocation 削減を、計測結果に基づいて検討する
  - 由来: ディレクトリ検索 blocking 隔離の残余リスク (2026-05-04)
```

- [ ] **Step 3: Validate docs diff**

Run:

```bash
git diff -- docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected: Diff shows only the completed search TODO and the new backlog residual-risk item.

- [ ] **Step 4: Commit docs tracking**

Run:

```bash
git add docs/todo/TODO.md docs/todo/BACKLOG.md
git commit -m "docs: ディレクトリ検索負荷制御の残リスクを整理"
```

## Final Verification

- [ ] **Step 1: Check branch status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

- [ ] **Step 2: Report completion**

Include these items in the completion report:

```text
Changed files and reasons:
- src/server/files/search.rs: moved directory search execution into a request-scoped blocking task and added a synchronous bounded reader.
- docs/todo/TODO.md: marked the search load-control item complete after verification.
- docs/todo/BACKLOG.md: captured cancellation and allocation residual risks.

Affected dependent files:
- src/server/service.rs: behavior depends on the unchanged async search_directory API.
- tests/integration_test.rs: /api/search JSON and Host rejection coverage.
- tests/e2e/document_search.spec.ts: unchanged UI compatibility coverage.

Verification:
- cargo test --all-targets --all-features server::files::search::tests
- cargo test --all-targets --all-features api_search
- cargo clippy --all-targets --all-features -- -D warnings
- ./verify.sh

Residual risks:
- No server-side cancellation or search generation management.
- Blocking pool can still be occupied by extreme repeated searches.
- Search result context allocation and Markdown parse count are unchanged.
```
