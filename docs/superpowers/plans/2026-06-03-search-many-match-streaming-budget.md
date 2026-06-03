# Search Many-Match Streaming Budget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ディレクトリ検索の many-match 経路を block 逐次処理へ変え、result-limit 到達後の不要な Markdown parse、block allocation、巨大 block tail 探索を止める。

**Architecture:** `src/server/files/search.rs` に block visitor を追加し、既存の block 抽出契約を保ったまま `search_file_streaming_blocks()` で抽出と照合を接続する。`/api/search` の JSON 契約、Host/Origin/path validation、HTML sanitize、CSP、ファイルサイズ上限は変更しない。

**Tech Stack:** Rust, pulldown-cmark, axum integration tests, existing test hooks in `src/server/files/search.rs`, shell verification via `cargo test` and `./verify.sh`.

---

## Preconditions

- Work on a feature/fix branch, not `develop` or `main`.
- Current design spec: `docs/superpowers/specs/2026-06-03-search-many-match-streaming-budget-design.md`.
- Use TDD. Add the failing test before implementation for each behavior change.
- Keep measurement fixtures in `/tmp/markdown-view-search-many-match-streaming.XXXXXX`; do not commit generated fixtures.

## Effort Estimate

- Human effort: 1.5-2.5 days, including profiling and local measurement.
- Codex/AI-assisted effort: 4-7 hours, assuming tests and measurement commands run without environment failures.

## File Structure

- Modify: `src/server/files/search.rs`
  - Add `SearchBlockVisit`, `SearchBlockVisitOutcome`, `build_search_block_entry()`, and `visit_search_blocks_until_cancelled()`.
  - Keep `extract_search_blocks()` as a wrapper for existing tests and compatibility.
  - Add `search_file_streaming_blocks()` and use it from `search_directory_with_limits_blocking()`.
  - Add and adjust unit tests for visitor behavior, streaming result budget, context, cancellation, and large block scanning.
- Confirm only: `tests/integration/search.rs`
  - Existing HTTP contract tests should continue to pass.
- Confirm only: `tests/integration/security.rs`
  - Existing Host/security smoke tests should continue to pass.
- Update after implementation: `docs/todo/TODO.md`
  - Move the Medium item to Done Summary or update it with implementation results, measurement values, and residual risk.

## Acceptance Criteria

- Result-limit の 100 件到達後、同一ファイルの後続 block 抽出が進まない。
- 巨大 many-match block で result budget 到達後、tail 全体を走査しない。
- Stale cancellation が block 抽出中、large-block 探索中、通常 block 照合中で止まる。
- `/api/search` の `SearchResponse` JSON 契約、`truncated_reasons=["result_limit"]`、`searched_files`、`searched_bytes` が維持される。
- Host/Origin/path validation、HTML sanitize、CSP、ファイルサイズ上限が変わらない。
- dev/release、cold/warm、単一/複数ファイルの測定結果が、実パスや full process args なしで `docs/todo/TODO.md` に記録される。
- `./verify.sh` が pass する。

## Task 1: Add Streaming Block Visitor Contract Tests

**Files:**
- Modify: `src/server/files/search.rs`

- [ ] **Step 1: Write failing tests for the new block visitor**

In `src/server/files/search.rs`, inside the existing `#[cfg(test)] mod tests`, insert these tests near the existing `test_extract_search_blocks_*` tests:

```rust
    #[test]
    fn test_visit_search_blocks_抽出契約を維持する() {
        let mut visited = Vec::new();

        let outcome = visit_search_blocks_until_cancelled(
            "# 表示見出し {#custom-id}\n\n本文 [link](https://example.com) visible\n\n```sh\nignored\n```\n\n| col |\n| --- |\n| セル |\n",
            &|| false,
            |block| {
                visited.push(block.clone());
                SearchBlockVisit::Continue
            },
        )
        .expect("キャンセルなしのvisitorは完了する");

        assert_eq!(outcome, SearchBlockVisitOutcome::Completed);
        assert_eq!(visited.len(), 4);
        assert_eq!(visited[0].text, "表示見出し");
        assert_eq!(visited[1].text, "本文  visible");
        assert_eq!(visited[2].text, "col");
        assert_eq!(visited[3].text, "セル");
    }

    #[test]
    fn test_visit_search_blocks_visitor停止後は後続blockを抽出しない() {
        let mut visited = Vec::new();
        let before_extracts = reset_search_block_extract_count_for_test();

        let outcome = visit_search_blocks_until_cancelled(
            "needle first.\n\nneedle second.\n\nneedle third.",
            &|| false,
            |block| {
                visited.push(block.text.clone());
                SearchBlockVisit::StopResultLimit
            },
        )
        .expect("visitor停止はキャンセルではない");
        let extracts = search_block_extract_count_for_test() - before_extracts;

        assert_eq!(outcome, SearchBlockVisitOutcome::StoppedByResultLimit);
        assert_eq!(visited, vec!["needle first.".to_string()]);
        assert!(
            extracts < 6,
            "visitor停止後に後続blockのevent streamを読み進めすぎている: {extracts}"
        );
    }

    #[test]
    fn test_visit_search_blocks_途中staleならnoneを返す() {
        let checks = Cell::new(0usize);
        let mut visited = Vec::new();

        let outcome = visit_search_blocks_until_cancelled(
            "# title\n\nfirst paragraph\n\nsecond paragraph",
            &|| {
                let next = checks.get() + 1;
                checks.set(next);
                next >= 3
            },
            |block| {
                visited.push(block.text.clone());
                SearchBlockVisit::Continue
            },
        );

        assert!(outcome.is_none());
        assert!(
            visited.len() <= 1,
            "stale後にblock visitorが処理を続けている: {visited:?}"
        );
    }
```

- [ ] **Step 2: Run the new tests and verify they fail**

Run:

```bash
cargo test server::files::search::tests::test_visit_search_blocks_ -- --nocapture
```

Expected: compile failure because `visit_search_blocks_until_cancelled`, `SearchBlockVisit`, `SearchBlockVisitOutcome`, `reset_search_block_extract_count_for_test`, or `search_block_extract_count_for_test` is not defined yet.

- [ ] **Step 3: Add test counter accessors for block extraction**

In `src/server/files/search.rs`, near `reset_search_context_build_count_for_test()`, add:

```rust
#[cfg(test)]
fn reset_search_block_extract_count_for_test() -> usize {
    SEARCH_BLOCK_EXTRACT_COUNT_FOR_TEST.with(|count| {
        count.set(0);
        count.get()
    })
}

#[cfg(test)]
fn search_block_extract_count_for_test() -> usize {
    SEARCH_BLOCK_EXTRACT_COUNT_FOR_TEST.with(std::cell::Cell::get)
}
```

- [ ] **Step 4: Add the visitor types and implementation**

In `src/server/files/search.rs`, replace `extract_search_blocks_until_cancelled()` and `finalize_search_block()` with the following structure. Keep `extract_search_blocks()` as a compatibility wrapper:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchBlockVisit {
    Continue,
    StopResultLimit,
    StopCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchBlockVisitOutcome {
    Completed,
    StoppedByResultLimit,
}

fn extract_search_blocks(markdown: &str) -> Vec<SearchBlockEntry> {
    extract_search_blocks_until_cancelled(markdown, &|| false)
        .expect("キャンセルなしの検索ブロック抽出は常に完了する")
}

fn extract_search_blocks_until_cancelled(
    markdown: &str,
    is_cancelled: &impl Fn() -> bool,
) -> Option<Vec<SearchBlockEntry>> {
    let mut blocks = Vec::new();
    let outcome = visit_search_blocks_until_cancelled(markdown, is_cancelled, |block| {
        blocks.push(block.clone());
        SearchBlockVisit::Continue
    })?;
    debug_assert_eq!(outcome, SearchBlockVisitOutcome::Completed);
    Some(blocks)
}

fn visit_search_blocks_until_cancelled(
    markdown: &str,
    is_cancelled: &impl Fn() -> bool,
    mut visitor: impl FnMut(&SearchBlockEntry) -> SearchBlockVisit,
) -> Option<SearchBlockVisitOutcome> {
    let mut current_block = String::new();
    let mut block_depth = 0usize;
    let mut item_depth = 0usize;
    let mut link_depth = 0usize;
    let mut image_depth = 0usize;
    let mut code_block_depth = 0usize;
    let mut inline_html_depth = 0usize;

    for event in Parser::new_ext(markdown, markdown_options(MarkdownProfile::Search)) {
        if is_cancelled() {
            return None;
        }
        notify_search_block_extract_for_test();
        if is_cancelled() {
            return None;
        }

        match event {
            Event::Start(tag) => {
                if matches!(tag, Tag::Item) {
                    if item_depth == 0 {
                        if block_depth == 0 {
                            current_block.clear();
                        } else if !current_block.ends_with('\n') {
                            current_block.push('\n');
                        }
                        block_depth += 1;
                    }
                    item_depth += 1;
                } else if is_search_block_tag(&tag) {
                    if block_depth == 0 {
                        current_block.clear();
                    } else if !current_block.ends_with('\n') {
                        current_block.push('\n');
                    }
                    block_depth += 1;
                }

                match tag {
                    Tag::Link { .. } => link_depth += 1,
                    Tag::Image { .. } => image_depth += 1,
                    Tag::CodeBlock(_) => code_block_depth += 1,
                    _ => {}
                }
            }
            Event::End(tag) => {
                let mut finalized = false;
                if matches!(tag, TagEnd::Item) {
                    item_depth = item_depth.saturating_sub(1);
                    if item_depth == 0 {
                        block_depth = block_depth.saturating_sub(1);
                        if block_depth == 0 {
                            finalized = true;
                            inline_html_depth = 0;
                        }
                    }
                } else if is_search_block_end_tag(&tag) {
                    block_depth = block_depth.saturating_sub(1);
                    if !matches!(tag, TagEnd::Paragraph) {
                        debug_assert_eq!(
                            inline_html_depth, 0,
                            "inline HTML depth must be balanced before non-paragraph block end"
                        );
                    }
                    inline_html_depth = 0;
                    if block_depth == 0 {
                        finalized = true;
                    }
                }

                match tag {
                    TagEnd::Link => link_depth = link_depth.saturating_sub(1),
                    TagEnd::Image => image_depth = image_depth.saturating_sub(1),
                    TagEnd::CodeBlock => code_block_depth = code_block_depth.saturating_sub(1),
                    _ => {}
                }

                if finalized {
                    if let Some(block) = build_search_block_entry(&current_block) {
                        match visitor(&block) {
                            SearchBlockVisit::Continue => {}
                            SearchBlockVisit::StopResultLimit => {
                                return Some(SearchBlockVisitOutcome::StoppedByResultLimit);
                            }
                            SearchBlockVisit::StopCancelled => return None,
                        }
                    }
                    current_block.clear();
                }
            }
            Event::Text(text) => {
                if should_capture_text(
                    block_depth,
                    item_depth,
                    link_depth,
                    image_depth,
                    code_block_depth,
                    inline_html_depth,
                ) {
                    current_block.push_str(&text);
                }
            }
            Event::Code(text) => {
                if should_capture_text(
                    block_depth,
                    item_depth,
                    link_depth,
                    image_depth,
                    code_block_depth,
                    inline_html_depth,
                ) {
                    current_block.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if should_capture_text(
                    block_depth,
                    item_depth,
                    link_depth,
                    image_depth,
                    code_block_depth,
                    inline_html_depth,
                ) && !current_block.ends_with('\n')
                {
                    current_block.push('\n');
                }
            }
            Event::Rule => {
                if block_depth > 0 && !current_block.ends_with('\n') {
                    current_block.push('\n');
                }
            }
            Event::InlineHtml(html) => update_inline_html_depth(&mut inline_html_depth, &html),
            Event::Html(_)
            | Event::TaskListMarker(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::FootnoteReference(_) => {}
        }
    }

    if block_depth == 0 {
        if let Some(block) = build_search_block_entry(&current_block) {
            match visitor(&block) {
                SearchBlockVisit::Continue => {}
                SearchBlockVisit::StopResultLimit => {
                    return Some(SearchBlockVisitOutcome::StoppedByResultLimit);
                }
                SearchBlockVisit::StopCancelled => return None,
            }
        }
    }

    Some(SearchBlockVisitOutcome::Completed)
}

fn build_search_block_entry(text: &str) -> Option<SearchBlockEntry> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(SearchBlockEntry {
        text: trimmed.to_string(),
        sentences: if is_large_search_block(trimmed) {
            Vec::new()
        } else {
            split_text_into_sentence_ranges(trimmed)
        },
    })
}

fn finalize_search_block(blocks: &mut Vec<SearchBlockEntry>, text: &str) {
    if let Some(block) = build_search_block_entry(text) {
        blocks.push(block);
    }
}
```

The `finalize_search_block()` wrapper may remain for existing code or tests. It can be removed later only if no callers remain.

- [ ] **Step 5: Run visitor and extraction tests**

Run:

```bash
cargo test server::files::search::tests::test_visit_search_blocks_ -- --nocapture
cargo test server::files::search::tests::test_extract_search_blocks_ -- --nocapture
```

Expected: all targeted tests pass.

- [ ] **Step 6: Commit Task 1**

```bash
git add src/server/files/search.rs
git commit -m "test: 検索block visitor契約を固定"
```

## Task 2: Add Streaming File Search and Preserve Match Contracts

**Files:**
- Modify: `src/server/files/search.rs`

- [ ] **Step 1: Write failing tests for file-local streaming search**

Insert these tests near the existing `test_find_matches_for_file_*` tests:

```rust
    #[test]
    fn test_search_file_streaming_blocks_残り件数で後続block抽出を停止する() {
        let markdown = (0..120)
            .map(|index| format!("needle sentence {index}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let before_extracts = reset_search_block_extract_count_for_test();
        let before_context_builds = reset_search_context_build_count_for_test();

        let result =
            search_file_streaming_blocks("many.md", &markdown, "needle", 3, &|| false).unwrap();
        let extracts = search_block_extract_count_for_test() - before_extracts;
        let context_builds = search_context_build_count_for_test() - before_context_builds;

        assert_eq!(result.outcome, SearchBlockVisitOutcome::StoppedByResultLimit);
        assert_eq!(result.results.len(), 3);
        assert_eq!(context_builds, 3);
        assert!(
            extracts < 30,
            "result-limit後も後続block抽出が進んでいる: {extracts}"
        );
        assert_eq!(result.results[0].file_match_index, 0);
        assert_eq!(result.results[1].file_match_index, 1);
        assert_eq!(result.results[2].file_match_index, 2);
    }

    #[test]
    fn test_search_file_streaming_blocks_unicode小文字化でバイト長が変わっても安全に一致する()
    {
        let markdown = "İstanbul is here. Another line.";

        let result =
            search_file_streaming_blocks("README.md", markdown, "i̇stanbul", 100, &|| false)
                .unwrap();

        assert_eq!(result.outcome, SearchBlockVisitOutcome::Completed);
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].file_match_index, 0);
        assert_eq!(result.results[0].current, "İstanbul is here.");
    }

    #[test]
    fn test_search_file_streaming_blocks_staleならnoneを返す() {
        let checks = Cell::new(0usize);
        let markdown = "# title\n\nneedle first.\n\nneedle second.";

        let result = search_file_streaming_blocks("README.md", markdown, "needle", 100, &|| {
            let next = checks.get() + 1;
            checks.set(next);
            next >= 4
        });

        assert!(result.is_none());
    }
```

- [ ] **Step 2: Run the new tests and verify they fail**

Run:

```bash
cargo test server::files::search::tests::test_search_file_streaming_blocks_ -- --nocapture
```

Expected: compile failure because `search_file_streaming_blocks` and `StreamingFileSearchResult` are not defined.

- [ ] **Step 3: Add streaming file result type and helper**

In `src/server/files/search.rs`, after `find_matches_for_file()` or just before it, add:

```rust
#[derive(Debug)]
struct StreamingFileSearchResult {
    results: Vec<SearchResultItem>,
    outcome: SearchBlockVisitOutcome,
}

fn search_file_streaming_blocks(
    file: &str,
    markdown: &str,
    query: &str,
    remaining_results: usize,
    is_cancelled: &impl Fn() -> bool,
) -> Option<StreamingFileSearchResult> {
    if remaining_results == 0 {
        return Some(StreamingFileSearchResult {
            results: Vec::new(),
            outcome: SearchBlockVisitOutcome::StoppedByResultLimit,
        });
    }

    let mut results = Vec::new();
    let mut previous_blocks: VecDeque<SearchBlockEntry> = VecDeque::new();
    let normalized_query = query.to_lowercase();
    let mut file_match_index = 0usize;

    let outcome = visit_search_blocks_until_cancelled(markdown, is_cancelled, |block| {
        if results.len() >= remaining_results {
            return SearchBlockVisit::StopResultLimit;
        }

        previous_blocks.push_back(block.clone());
        while previous_blocks.len() > 2 {
            previous_blocks.pop_front();
        }
        let blocks_for_context: Vec<SearchBlockEntry> = previous_blocks.iter().cloned().collect();
        let context_block_index = blocks_for_context.len().saturating_sub(1);

        let block_results = if is_large_search_block(&block.text) {
            find_matches_for_large_block(
                file,
                &block.text,
                query,
                file_match_index,
                remaining_results.saturating_sub(results.len()),
                is_cancelled,
            )
        } else {
            find_matches_for_block_with_context(
                file,
                &blocks_for_context,
                context_block_index,
                &normalized_query,
                file_match_index,
                remaining_results.saturating_sub(results.len()),
                is_cancelled,
            )
        };

        let Some(block_results) = block_results else {
            return SearchBlockVisit::StopCancelled;
        };

        file_match_index += block_results.len();
        results.extend(block_results);

        if results.len() >= remaining_results {
            SearchBlockVisit::StopResultLimit
        } else {
            SearchBlockVisit::Continue
        }
    })?;

    Some(StreamingFileSearchResult { results, outcome })
}
```

Then add a focused block matcher for normal blocks:

```rust
fn find_matches_for_block_with_context(
    file: &str,
    blocks: &[SearchBlockEntry],
    block_index: usize,
    normalized_query: &str,
    file_match_index_start: usize,
    remaining_results: usize,
    is_cancelled: &impl Fn() -> bool,
) -> Option<Vec<SearchResultItem>> {
    if remaining_results == 0 {
        return Some(Vec::new());
    }

    let block = &blocks[block_index];
    if block.text.is_empty() {
        return Some(Vec::new());
    }

    let normalized = build_case_fold_index(&block.text, is_cancelled)?;
    let mut search_start = 0usize;
    let mut results = Vec::new();

    while search_start <= normalized.normalized_text.len() {
        if is_cancelled() {
            return None;
        }
        let Some(relative_index) = normalized.normalized_text[search_start..].find(normalized_query)
        else {
            break;
        };
        let normalized_match_start = search_start + relative_index;
        let normalized_match_end = normalized_match_start + normalized_query.len();
        let match_start = normalized.original_offset(normalized_match_start);
        let match_end = normalized.original_offset(normalized_match_end);
        let context = build_search_context(blocks, block_index, match_start, match_end);
        results.push(SearchResultItem::new(
            file.to_string(),
            file_match_index_start + results.len(),
            context.before,
            context.current,
            context.after,
        ));
        if is_cancelled() {
            return None;
        }
        if results.len() >= remaining_results {
            break;
        }
        search_start = normalized_match_end;
    }

    Some(results)
}
```

If Rust reports the `block_index` local is unused, remove the local and keep only `context_block_index`.

- [ ] **Step 4: Run streaming tests**

Run:

```bash
cargo test server::files::search::tests::test_search_file_streaming_blocks_ -- --nocapture
```

Expected: the new streaming tests pass.

- [ ] **Step 5: Run existing match tests**

Run:

```bash
cargo test server::files::search::tests::test_find_matches_for_file_ -- --nocapture
```

Expected: all existing `find_matches_for_file` tests still pass. These tests keep the old helper honest while the directory path migrates to streaming.

- [ ] **Step 6: Commit Task 2**

```bash
git add src/server/files/search.rs
git commit -m "feat: 検索block逐次照合を追加"
```

## Task 3: Use Streaming Search in Directory Search

**Files:**
- Modify: `src/server/files/search.rs`

- [ ] **Step 1: Write a failing directory-level result-budget test**

Insert this test near existing `test_search_directory_*` tests:

```rust
    #[test]
    fn test_search_directory_result_limit到達後に同一ファイルの後続blockを抽出しない() {
        let dir = tempfile::tempdir().unwrap();
        let markdown = (0..120)
            .map(|index| format!("needle sentence {index}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        std::fs::write(dir.path().join("many.md"), markdown).unwrap();
        let canonical = canonical_of(dir.path());
        let before_extracts = reset_search_block_extract_count_for_test();

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 3,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();
        let extracts = search_block_extract_count_for_test() - before_extracts;

        assert_eq!(response.results.len(), 3);
        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Result]
        );
        assert_eq!(response.searched_files, 1);
        assert!(
            extracts < 30,
            "directory searchがresult-limit後もblock抽出を続けている: {extracts}"
        );
    }
```

- [ ] **Step 2: Run the directory-level test and verify it fails**

Run:

```bash
cargo test server::files::search::tests::test_search_directory_result_limit到達後に同一ファイルの後続blockを抽出しない -- --nocapture
```

Expected: FAIL because `search_directory_with_limits_blocking()` still calls `extract_search_blocks_until_cancelled()` before matching and extracts the whole file.

- [ ] **Step 3: Replace full extraction in `search_directory_with_limits_blocking()`**

In `src/server/files/search.rs`, replace this block inside `search_directory_with_limits_blocking()`:

```rust
        let Some(blocks) =
            extract_search_blocks_until_cancelled(&markdown, &|| cancellation.is_cancelled())
        else {
            log_search_cancelled("extract_blocks", &stats, results.len());
            results.clear();
            break;
        };
        let Some(file_results) =
            find_matches_for_file(&relative, &blocks, &query, remaining_results, &|| {
                cancellation.is_cancelled()
            })
        else {
            log_search_cancelled("find_matches", &stats, results.len());
            results.clear();
            break;
        };
```

with:

```rust
        let Some(file_search) = search_file_streaming_blocks(
            &relative,
            &markdown,
            &query,
            remaining_results,
            &|| cancellation.is_cancelled(),
        ) else {
            log_search_cancelled("streaming_find_matches", &stats, results.len());
            results.clear();
            break;
        };
        let file_results = file_search.results;
        if matches!(
            file_search.outcome,
            SearchBlockVisitOutcome::StoppedByResultLimit
        ) {
            stats.mark_truncated(SearchTruncationReason::Result);
        }
```

Keep the existing loop that pushes `file_results` into `results`.

- [ ] **Step 4: Run the directory-level test**

Run:

```bash
cargo test server::files::search::tests::test_search_directory_result_limit到達後に同一ファイルの後続blockを抽出しない -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run search unit tests**

Run:

```bash
cargo test server::files::search -- --nocapture
```

Expected: all `src/server/files/search.rs` unit tests pass.

- [ ] **Step 6: Commit Task 3**

```bash
git add src/server/files/search.rs
git commit -m "feat: ディレクトリ検索をblock逐次処理に変更"
```

## Task 4: Tighten Large-Block Budget and Cancellation Tests

**Files:**
- Modify: `src/server/files/search.rs`

- [ ] **Step 1: Add large-block budget tests for streaming path**

Insert these tests near existing large-block tests:

```rust
    #[test]
    fn test_search_file_streaming_blocks_巨大many_matchはresult_limit後にtailを走査しない() {
        let block_text = format!(
            "{}{}{}",
            (0..3).map(|_| "needle ").collect::<String>(),
            "a".repeat(LARGE_SEARCH_BLOCK_BYTES + 20_000),
            (0..100).map(|_| " needle").collect::<String>()
        );
        let scanned_bytes = std::rc::Rc::new(Cell::new(0usize));
        let scanned_bytes_for_hook = std::rc::Rc::clone(&scanned_bytes);
        let _guard = set_search_large_block_find_hook_for_test(move |search_bytes| {
            scanned_bytes_for_hook.set(scanned_bytes_for_hook.get() + search_bytes);
        });

        let result =
            search_file_streaming_blocks("many.md", &block_text, "needle", 3, &|| false).unwrap();

        assert_eq!(result.outcome, SearchBlockVisitOutcome::StoppedByResultLimit);
        assert_eq!(result.results.len(), 3);
        assert!(
            scanned_bytes.get() < block_text.len(),
            "巨大many-matchでresult-limit後もtailを走査している: {} bytes",
            scanned_bytes.get()
        );
    }

    #[test]
    fn test_search_file_streaming_blocks_巨大ブロック正規化中staleならnoneを返す() {
        let block_text = format!("{}needle", "a".repeat(LARGE_SEARCH_BLOCK_BYTES + 1));
        let cancel_checks = Cell::new(0usize);
        reset_search_context_build_count_for_test();

        let result = search_file_streaming_blocks("many.md", &block_text, "needle", 1, &|| {
            let next = cancel_checks.get() + 1;
            cancel_checks.set(next);
            next >= 3
        });

        assert!(result.is_none());
        assert_eq!(search_context_build_count_for_test(), 0);
    }
```

- [ ] **Step 2: Run the large-block streaming tests**

Run:

```bash
cargo test server::files::search::tests::test_search_file_streaming_blocks_巨大 -- --nocapture
```

Expected: PASS if Task 2 preserved `find_matches_for_large_block()` budget behavior. If the first test fails, inspect whether `find_matches_for_large_block()` receives `remaining_results.saturating_sub(results.len())` from `search_file_streaming_blocks()`.

- [ ] **Step 3: Remove accidental full extraction from streaming path**

Run:

```bash
rg -n "extract_search_blocks_until_cancelled\\(&markdown|find_matches_for_file\\(&relative" src/server/files/search.rs
```

Expected: no output. If output exists in `search_directory_with_limits_blocking()`, replace that call site with `search_file_streaming_blocks()` as described in Task 3.

- [ ] **Step 4: Run cancellation-related tests**

Run:

```bash
cargo test server::files::search::tests::test_.*stale -- --nocapture
```

Expected: all stale/cancellation unit tests pass.

- [ ] **Step 5: Commit Task 4**

```bash
git add src/server/files/search.rs
git commit -m "test: 検索逐次処理の巨大block予算を固定"
```

## Task 5: Verify HTTP and Security Contracts

**Files:**
- Confirm: `tests/integration/search.rs`
- Confirm: `tests/integration/security.rs`
- Modify only if a real contract regression is found: `src/server/files/search.rs`

- [ ] **Step 1: Run directory search integration tests**

Run:

```bash
cargo test --test integration search -- --nocapture
```

Expected: all search integration tests pass. In particular, these contracts remain true:

- `/api/search?q=alpha%20note` returns HTTP 200 in directory mode.
- Result-limit responses include `truncated=true` and `truncated_reasons` containing `"result_limit"`.
- Byte-limit responses include `truncated=true` and `truncated_reasons` containing `"byte_limit"`.
- `searched_files` and `searched_bytes` remain numeric JSON fields.

- [ ] **Step 2: Run security integration tests for search Host handling**

Run:

```bash
cargo test --test integration security -- --nocapture
```

Expected: Host smoke tests pass, including `/api/search?q=test`.

- [ ] **Step 3: Inspect TypeScript search response contract**

Run:

```bash
rg -n "SearchTruncationReason|searched_bytes|truncated_reasons|max_results|result_limit|byte_limit|file_limit" src/template/assets/ts src/template/assets/generated-js
```

Expected: only the existing `result_limit`, `file_limit`, and `byte_limit` response contract appears. No source edit is needed because this plan does not add a truncation reason.

- [ ] **Step 4: Commit Task 5 only if files changed**

If no files changed, do not commit. If a real regression required a fix:

```bash
git add src/server/files/search.rs tests/integration/search.rs tests/integration/security.rs
git commit -m "fix: 検索逐次化後のHTTP契約を維持"
```

Expected: commit only includes necessary contract-preserving fixes.

## Task 6: Run Local Many-Match Measurement

**Files:**
- Temporary only: `/tmp/markdown-view-search-many-match-streaming.XXXXXX`
- Modify after measurement: `docs/todo/TODO.md`

- [ ] **Step 1: Build debug and release binaries**

Run:

```bash
cargo build
cargo build --release
```

Expected: both commands pass.

- [ ] **Step 2: Create temporary fixtures**

Run:

```bash
FIXTURE_DIR="$(mktemp -d /tmp/markdown-view-search-many-match-streaming.XXXXXX)"
SINGLE_DIR="$FIXTURE_DIR/single"
MULTI_DIR="$FIXTURE_DIR/multi"
mkdir -p "$SINGLE_DIR" "$MULTI_DIR"
python3 - <<'PY' "$SINGLE_DIR" "$MULTI_DIR"
import pathlib
import sys

single = pathlib.Path(sys.argv[1])
multi = pathlib.Path(sys.argv[2])
needle_line = "needle sentence for many match measurement.\n\n"
single_text = needle_line * (10 * 1024 * 1024 // len(needle_line) + 1)
(single / "many.md").write_text(single_text[:10 * 1024 * 1024], encoding="utf-8")

for index in range(120):
    text = (needle_line * 90)[:32 * 1024]
    (multi / f"many-{index:03}.md").write_text(text, encoding="utf-8")
print(single)
print(multi)
PY
```

Expected: prints two temporary fixture directories. Do not copy the exact temp path into committed docs; use `/tmp/markdown-view-search-many-match-streaming.***` in docs.

- [ ] **Step 3: Measure debug single-file cold and warm runs**

Run the server in one terminal:

```bash
target/debug/markdown-view "$SINGLE_DIR" --port 3000
```

In another terminal, run:

```bash
SERVER_PID="$(pgrep -n markdown-view)"
for run in 1 2 3 4 5; do
  /usr/bin/time -f "elapsed=%e" curl -sS "http://127.0.0.1:3000/api/search?q=needle" -o "$FIXTURE_DIR/debug-single-$run.json"
  ps -o rss= -p "$SERVER_PID"
  sleep 5
  ps -o rss= -p "$SERVER_PID"
done
```

Expected: five JSON files under the temp directory and RSS readings. Run 1 is cold for this server process; later runs are warm.

- [ ] **Step 4: Measure debug multi-file runs**

Stop the previous server, then run:

```bash
target/debug/markdown-view "$MULTI_DIR" --port 3000
```

In another terminal:

```bash
SERVER_PID="$(pgrep -n markdown-view)"
for run in 1 2 3 4 5; do
  /usr/bin/time -f "elapsed=%e" curl -sS "http://127.0.0.1:3000/api/search?q=needle" -o "$FIXTURE_DIR/debug-multi-$run.json"
  ps -o rss= -p "$SERVER_PID"
  sleep 5
  ps -o rss= -p "$SERVER_PID"
done
```

Expected: five JSON files and RSS readings for the multi-file fixture.

- [ ] **Step 5: Measure release single-file and multi-file runs**

Repeat Steps 3 and 4 with:

```bash
target/release/markdown-view "$SINGLE_DIR" --port 3000
target/release/markdown-view "$MULTI_DIR" --port 3000
```

Expected: release elapsed values are lower than debug values, and result-limit JSON contracts match debug.

- [ ] **Step 6: Summarize JSON contract without leaking paths**

Run:

```bash
python3 - <<'PY' "$FIXTURE_DIR"
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
for path in sorted(root.glob("*.json")):
    data = json.loads(path.read_text(encoding="utf-8"))
    print(path.name, {
        "results": len(data.get("results", [])),
        "searched_files": data.get("searched_files"),
        "searched_bytes": data.get("searched_bytes"),
        "truncated": data.get("truncated"),
        "truncated_reasons": data.get("truncated_reasons"),
    })
PY
```

Expected: each response has `results` at or below the max result count, `truncated=true`, and `truncated_reasons` containing `result_limit` for many-match result-limit cases.

## Task 7: Update TODO Documentation With Results

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Update the Medium item**

Edit the item `ディレクトリ検索 many-match の cold run と RSS plateau を切り分ける`.

If implementation and measurement satisfy the acceptance criteria, move it into Done Summary with this structure. Replace the example measurement sentence with the actual elapsed and RSS ranges collected in Task 6 before committing:

```markdown
- [x] ディレクトリ検索 many-match の cold run と RSS plateau を切り分け、逐次処理で抑制する
  - 完了根拠: ディレクトリ検索のファイル内検索を、Markdown 全体の `Vec<SearchBlockEntry>` 一括抽出から block 逐次抽出・逐次照合へ変更した。result-limit 到達時は同一ファイルの残り Markdown parse、後続 block allocation、巨大 block tail 探索を停止する。`/api/search` の JSON 契約、`truncated_reasons=["result_limit"]`、`searched_files`、`searched_bytes`、Host/Origin 検証、path validation、HTML sanitize、CSP、ファイルサイズ上限は変更していない。構造回帰は result-limit 後の block 抽出停止、巨大 many-match tail 探索停止、stale cancellation、Unicode case-fold offset、bounded context snippet、既存 HTTP 契約で固定した。
  - 計測: 2026-06-03 に dev/release、cold/warm、単一 10MiB 近傍 many-match、複数ファイル many-match を分けて測定した。fixture は `/tmp/markdown-view-search-many-match-streaming.***` に生成し、repo へ追加していない。debug single、debug multi、release single、release multi のそれぞれについて elapsed、request 中 peak RSS、request 後 after RSS、5秒後 settled RSS の範囲を記録した。HTTP response は `truncated=true`、`truncated_reasons=["result_limit"]`、`results=100` を維持した。
  - 残余リスク: allocator の RSS plateau は実行環境に依存するため、CI では固定秒数・固定 RSS 上限を置かず、構造テストとローカル測定記録で保証する。
```

Before committing, expand the measurement sentence so it contains the concrete ranges from Task 6 for each matrix entry.

- [ ] **Step 2: Scan docs for leaked paths and placeholders**

Run:

```bash
rg -n "/tmp/markdown-view-search-many-match-streaming\\.[A-Za-z0-9]+|target/(debug|release)/markdown-view .*--port|/home/propan/personal_dev/markdown-view|elapsed \\[|RSS \\[" docs/todo/TODO.md
```

Expected: no output. The TODO may mention `/tmp/markdown-view-search-many-match-streaming.XXXXXX` or `/tmp/markdown-view-search-many-match-streaming.***`, but must not include the exact temp suffix, full process args from measurement logs, or bracketed unresolved measurement ranges. This plan intentionally contains command examples, so process-args leak scanning is limited to the committed TODO output.

- [ ] **Step 3: Commit TODO update**

```bash
git add docs/todo/TODO.md
git commit -m "docs: 検索many-match逐次化結果を記録"
```

## Task 8: Final Verification

**Files:**
- Confirm all modified files.

- [ ] **Step 1: Run focused search tests**

Run:

```bash
cargo test server::files::search -- --nocapture
```

Expected: all search unit tests pass.

- [ ] **Step 2: Run integration search and security tests**

Run:

```bash
cargo test --test integration search -- --nocapture
cargo test --test integration security -- --nocapture
```

Expected: both commands pass.

- [ ] **Step 3: Run full verification**

Run:

```bash
./verify.sh
```

Expected: format, clippy, and tests pass.

- [ ] **Step 4: Inspect git history and working tree**

Run:

```bash
git status --short --branch
git log --oneline -6
```

Expected: working tree is clean, branch is not `develop` or `main`, and recent commits are focused:

- visitor contract
- streaming block matching
- directory search streaming
- large-block budget test
- TODO measurement docs

## Rollback Path

If the streaming implementation causes contract regressions that cannot be resolved cleanly, revert the implementation commits from Tasks 1-4 and keep the design/plan docs. The previous directory search path is `extract_search_blocks_until_cancelled()` followed by `find_matches_for_file()`. Because the plan preserves `/api/search` JSON and UI contracts, rollback should not require TypeScript or UI changes.

## Residual Risks To Report

- RSS plateau can remain allocator- and OS-dependent even after structural allocation is reduced.
- Cold run includes build profile and parser initialization effects that CI should not encode as fixed thresholds.
- Context compatibility may require a bounded previous-block context rather than unlimited adjacent block lookup; report any visible before/after snippet differences if tests reveal them.
