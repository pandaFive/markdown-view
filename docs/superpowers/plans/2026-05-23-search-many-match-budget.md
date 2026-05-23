# Search Many-Match Budget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ディレクトリ検索で result-limit 到達後も同一ファイル内の match/context 生成が続く経路を止め、10MiB 近傍 many-match の可用性リスクを下げる。

**Architecture:** `search_directory_with_limits_blocking()` が残り結果予算を計算し、`find_matches_for_file()` に渡す。`find_matches_for_file()` は予算件数までだけ `SearchResultItem` を生成し、予算到達時にブロック探索と match 探索を終了する。`SearchResponse` JSON、truncation reason、Host/Origin/path/sanitize/CSP のセキュリティ境界は変更しない。

**Tech Stack:** Rust, Cargo, Tokio, Axum test harness, existing markdown-view search module, `curl`, `ps`, `/usr/bin/time`, Markdown docs.

---

## File Structure

- Modify: `src/server/files/search.rs`
  - `search_directory_with_limits_blocking()` から `find_matches_for_file()` へ残り結果予算を渡す。
  - `find_matches_for_file()` の signature を変更し、予算到達時の早期停止を実装する。
  - unit test を追加して、1ファイル内の大量 match でも予算件数だけ返ることを固定する。
- Modify if HTTP contract needs an explicit regression: `tests/integration/search.rs`
  - 既存の `/api/search` result-limit JSON test が十分なら変更しない。
  - 追加する場合は JSON 契約のみを確認し、性能値の閾値は入れない。
- Modify after implementation and measurement: `docs/todo/TODO.md`
  - Medium Priority 項目に実装内容、計測結果、残余リスクを記録する。
- Reference: `docs/superpowers/specs/2026-05-23-search-many-match-budget-design.md`
  - 目的、非目標、受け入れ基準、セキュリティ境界を確認する。
- Temporary only: `/tmp/markdown-view-search-many-match-budget.XXXXXX`
  - 10MiB 近傍 many-match fixture と HTTP response JSON を保存する。コミットしない。

## Scope Check

この plan は単一サブシステム、ディレクトリ検索の result-limit 早期停止だけを扱う。検索インデックス、`extract_search_blocks()` の逐次化、Markdown parser profile の変更、UI 変更、公開 JSON 変更は含めない。実装後の計測で改善が不十分な場合は、今回の変更を戻さず、ブロック抽出途中停止または逐次 search iterator 化を次段の別設計として残す。

### Task 1: Preflight

**Files:**
- Reference: `docs/superpowers/specs/2026-05-23-search-many-match-budget-design.md`
- Reference: `docs/todo/TODO.md`
- Reference: `src/server/files/search.rs`

- [ ] **Step 1: Confirm branch and worktree**

Run:

```bash
git branch --show-current
git status --short
pwd
```

Expected:

```text
docs/search-many-match-budget-design
```

`pwd` should be the active `<worktree>` path for this task. `git status --short` may show only the plan file if this plan has not been committed yet. If source files are already modified, inspect them before continuing.

- [ ] **Step 2: Read the accepted design**

Run:

```bash
sed -n '1,180p' docs/superpowers/specs/2026-05-23-search-many-match-budget-design.md
```

Expected: the design states that `find_matches_for_file()` receives a remaining result budget, `extract_search_blocks()` stays unchanged, and performance thresholds are not added to CI.

- [ ] **Step 3: Confirm the current many-match issue**

Run:

```bash
rg -n "10MiB|many-match|早期停止|処理単位" docs/todo/TODO.md
```

Expected: the Medium Priority item for `ディレクトリ検索の 10MiB 近傍 many-match 経路を早期停止・処理単位見直しで抑制する` is visible.

### Task 2: Add Failing Unit Tests For File-Local Result Budget

**Files:**
- Modify: `src/server/files/search.rs`

- [ ] **Step 1: Add tests that call the new four-argument signature**

Edit `src/server/files/search.rs` inside the existing `#[cfg(test)] mod tests` block. Add these tests near the other search result limit tests:

```rust
    #[test]
    fn test_find_matches_for_file_残り件数で同一ファイル内探索を停止する() {
        let block_text = (0..120)
            .map(|index| format!("needle sentence {index}."))
            .collect::<Vec<_>>()
            .join(" ");
        let blocks = vec![SearchBlockEntry {
            text: block_text.clone(),
            sentences: split_text_into_sentence_ranges(&block_text),
        }];

        let results = find_matches_for_file("many.md", &blocks, "needle", 3);

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].file_match_index, 0);
        assert_eq!(results[1].file_match_index, 1);
        assert_eq!(results[2].file_match_index, 2);
        assert!(results.iter().all(|item| item.file == "many.md"));
    }

    #[test]
    fn test_find_matches_for_file_残り件数0なら結果を生成しない() {
        let block_text = "needle first. needle second.";
        let blocks = vec![SearchBlockEntry {
            text: block_text.to_string(),
            sentences: split_text_into_sentence_ranges(block_text),
        }];

        let results = find_matches_for_file("many.md", &blocks, "needle", 0);

        assert!(results.is_empty());
    }
```

- [ ] **Step 2: Run the targeted tests and verify they fail for the expected reason**

Run:

```bash
cargo test -p markdown-view test_find_matches_for_file_ -- --nocapture
```

Expected: compile fails because `find_matches_for_file` currently takes 3 arguments and the new tests call it with 4 arguments. The relevant error should mention that the function takes 3 arguments but 4 were supplied.

### Task 3: Implement The Result Budget In `find_matches_for_file`

**Files:**
- Modify: `src/server/files/search.rs`

- [ ] **Step 1: Change `find_matches_for_file()` signature and add early return**

Replace the function header and initial setup with:

```rust
fn find_matches_for_file(
    file: &str,
    blocks: &[SearchBlockEntry],
    query: &str,
    remaining_results: usize,
) -> Vec<SearchResultItem> {
    let mut results = Vec::new();
    if remaining_results == 0 {
        return results;
    }

    let normalized_query = query.to_lowercase();
    let mut file_match_index = 0usize;
```

- [ ] **Step 2: Add a loop label and stop when the budget is reached**

In the same function, change the block loop from:

```rust
    for (block_index, block) in blocks.iter().enumerate() {
```

to:

```rust
    'blocks: for (block_index, block) in blocks.iter().enumerate() {
```

Then, immediately after `results.push(SearchResultItem::new(...));`, add:

```rust
            if results.len() >= remaining_results {
                break 'blocks;
            }
```

The resulting push section should look like this:

```rust
            results.push(SearchResultItem::new(
                file.to_string(),
                file_match_index,
                context.before,
                context.current,
                context.after,
            ));
            if results.len() >= remaining_results {
                break 'blocks;
            }
            file_match_index += 1;
            search_start = normalized_match_end;
```

Keep `file_match_index += 1` after the budget check. The returned result indexes remain `0..remaining_results-1`, and no later result needs an index when the function exits.

- [ ] **Step 3: Update the caller to pass remaining result capacity**

In `search_directory_with_limits_blocking()`, replace:

```rust
        let blocks = extract_search_blocks(&markdown);
        let file_results = find_matches_for_file(&relative, &blocks, &query);
```

with:

```rust
        let remaining_results = limits.max_results.saturating_sub(results.len());
        let blocks = extract_search_blocks(&markdown);
        let file_results = find_matches_for_file(&relative, &blocks, &query, remaining_results);
```

- [ ] **Step 4: Run the new unit tests**

Run:

```bash
cargo test -p markdown-view test_find_matches_for_file_ -- --nocapture
```

Expected: both new tests pass.

- [ ] **Step 5: Run existing search limit tests**

Run:

```bash
cargo test -p markdown-view search_directory_結果数上限到達 -- --nocapture
```

Expected: `test_search_directory_結果数上限到達を明示する` passes and still reports exactly 100 results with `SearchTruncationReason::Result`.

- [ ] **Step 6: Commit the unit-level implementation**

Run:

```bash
git add src/server/files/search.rs
git commit -m "fix: 検索結果上限でファイル内探索を停止"
```

Expected: commit succeeds with only `src/server/files/search.rs` staged.

### Task 4: Decide Whether Integration Test Changes Are Needed

**Files:**
- Reference: `tests/integration/search.rs`
- Modify if needed: `tests/integration/search.rs`

- [ ] **Step 1: Inspect the existing HTTP result-limit test**

Run:

```bash
sed -n '150,190p' tests/integration/search.rs
```

Expected: `test_ディレクトリモード_api_searchは結果数打ち切りをjsonで返す` already checks HTTP 200, `truncated=true`, `truncated_reasons[0] == "result_limit"`, and `results.len() == 100`.

- [ ] **Step 2: Run the existing integration result-limit test**

Run:

```bash
cargo test --test integration_test search::test_ディレクトリモード_api_searchは結果数打ち切りをjsonで返す -- --nocapture
```

Expected: the test passes.

- [ ] **Step 3: Leave integration tests unchanged when the existing contract is covered**

If Step 1 and Step 2 pass, do not edit `tests/integration/search.rs`. The new behavior is an internal processing-budget guarantee covered by unit tests; the existing integration test already covers the HTTP contract.

If the existing test is missing any of the expected assertions, add this exact assertion block after parsing `json`:

```rust
    assert!(json["truncated"].as_bool().unwrap());
    assert_eq!(json["truncated_reasons"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["truncated_reasons"][0].as_str().unwrap(),
        "result_limit"
    );
    assert_eq!(json["results"].as_array().unwrap().len(), 100);
```

- [ ] **Step 4: Commit integration test changes only if the file changed**

Run this only when `tests/integration/search.rs` was modified:

```bash
git add tests/integration/search.rs
git commit -m "test: 検索apiの結果上限契約を補強"
```

Expected: commit succeeds. If `tests/integration/search.rs` was not modified, skip this step.

### Task 5: Run Required Verification

**Files:**
- Reference: `src/server/files/search.rs`
- Reference: `tests/integration/search.rs`

- [ ] **Step 1: Run focused search tests**

Run:

```bash
cargo test search --all-targets --all-features
```

Expected: all search-related tests pass.

- [ ] **Step 2: Run full verification**

Run:

```bash
./verify.sh
```

Expected: formatting, clippy, and tests all pass. If this fails, fix the failure before measuring performance or updating docs.

### Task 6: Measure 10MiB Many-Match Behavior Locally

**Files:**
- Temporary create: `/tmp/markdown-view-search-many-match-budget.XXXXXX`
- Modify later: `docs/todo/TODO.md`

- [ ] **Step 1: Record environment summary**

Run:

```bash
date +%F
rustc --version
cargo --version
uname -srmo
```

Expected: four lines with date, Rust version, Cargo version, and OS/kernel summary. Record these in the later `docs/todo/TODO.md` note. Do not record hostname, local username, full `ps args`, or absolute fixture path.

- [ ] **Step 2: Build a temporary 10MiB many-match fixture**

Run:

```bash
SEARCH_FIXTURE_DIR="$(mktemp -d /tmp/markdown-view-search-many-match-budget.XXXXXX)"
mkdir -p "$SEARCH_FIXTURE_DIR/workspace" "$SEARCH_FIXTURE_DIR/responses"
{
  printf '# Single 10 MiB many match\n\n'
  yes 'needle single large file paragraph with stable context for markdown parsing and search result generation.' | head -105000
} > "$SEARCH_FIXTURE_DIR/workspace/single.md"
du -sh "$SEARCH_FIXTURE_DIR/workspace"
wc -c "$SEARCH_FIXTURE_DIR/workspace/single.md"
```

Expected: `wc -c` reports a size below `10 * 1024 * 1024 + 1` bytes so the file is not skipped by the existing file-size guard. If it is above the limit, reduce `head -105000` to `head -100000` and regenerate the file.

- [ ] **Step 3: Start the server on a local port**

Run:

```bash
cargo run -- "$SEARCH_FIXTURE_DIR/workspace" --port 3017
```

Expected: server starts and binds to `127.0.0.1:3017`. Keep this process running in a separate terminal/session for the remaining measurement steps.

- [ ] **Step 4: Confirm the server process RSS before search**

Run in another terminal/session:

```bash
ps -o pid,rss,comm,args -C markdown-view
```

Expected: one `markdown-view` process for port 3017 is visible. Record only PID and RSS in docs; do not record full args.

- [ ] **Step 5: Run HTTP search five times and save compact JSON summaries**

Run:

```bash
for i in 1 2 3 4 5; do
  /usr/bin/time -f "elapsed=%E maxrss_kb=%M" \
    curl -sS -H 'Host: 127.0.0.1:3017' \
      'http://127.0.0.1:3017/api/search?q=needle' \
      -o "$SEARCH_FIXTURE_DIR/responses/run-$i.json"
  python3 -m json.tool "$SEARCH_FIXTURE_DIR/responses/run-$i.json" \
    | rg '"truncated"|"truncated_reasons"|"searched_files"|"searched_bytes"|"results"'
  ps -o pid,rss,comm -C markdown-view
done
```

Expected for each run:

- HTTP command exits successfully.
- JSON contains `truncated: true`.
- JSON contains `result_limit`.
- `searched_files` is `1`.
- `searched_bytes` is near the fixture file size.
- `results` contains 100 items.

Record elapsed and server RSS snapshots. Compare with the previous observation in `docs/todo/TODO.md`: elapsed about 36-38 seconds and server RSS about 0.9-1.3GiB.

- [ ] **Step 6: Stop the measurement server**

Stop the `cargo run` process with `Ctrl-C`. Then run:

```bash
ps -o pid,rss,comm,args -C markdown-view
```

Expected: no port 3017 measurement server remains. If another project server is still running, leave it alone.

### Task 7: Update The Medium Priority Item

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Update the many-match item with implementation and measurement results**

Edit the Medium Priority item `ディレクトリ検索の 10MiB 近傍 many-match 経路を早期停止・処理単位見直しで抑制する`.

If measurement improved clearly, replace the item with a done summary under `## Done Summary` using this structure:

```markdown
- [x] ディレクトリ検索の 10MiB 近傍 many-match 経路を早期停止・処理単位見直しで抑制する
  - 完了根拠: `search_directory_with_limits_blocking()` が `limits.max_results` から残り結果予算を計算し、`find_matches_for_file()` が予算到達時に同一ファイル内の match/context 生成を停止する構成にした。`SearchResponse` JSON、`truncated_reasons=["result_limit"]`、`searched_files`、`searched_bytes`、Host/Origin 検証、path validation、HTML sanitize、CSP、ファイルサイズ上限、検索キャンセル境界は変更していない。構造回帰は `find_matches_for_file()` の予算 test、context 生成回数 test、既存 result-limit test で固定した。10MiB 近傍 many-match fixture の手元計測では、実装前の elapsed 約 36-38 秒、server RSS 約 0.9-1.3GiB と比べて改善傾向を確認した。計測 fixture は `/tmp/markdown-view-search-many-match-budget.***` に生成し、repo へ追加していない。
  - 残余リスク: `extract_search_blocks()` は全ブロック抽出のままなので、巨大 Markdown parsing と block allocation は残る。今回の抑制で不足が出る場合は、ブロック抽出の途中停止または逐次 search iterator 化を別設計で扱う。
```

Replace `改善傾向を確認した` with concrete elapsed/RSS numbers from Task 6.

If measurement does not improve enough, keep the item unchecked and update `現状` / `対応` with the exact run values. Use the measured elapsed range and server RSS snapshot range from Task 6 in the `観測値` sentence:

```markdown
  - 現状: result-limit 到達後のファイル内 match/context 生成は予算で停止するようになったが、10MiB 近傍 many-match fixture の手元計測では elapsed/RSS の改善が不十分だった。観測値は run1-run5 の elapsed 範囲と server RSS after-search 範囲を記録した。主因は `extract_search_blocks()` が全ブロックを抽出する経路、または 100 件分の context が巨大になる経路に残っている可能性がある。
  - 対応: 次段では `extract_search_blocks()` の途中停止、または block 抽出と match 生成を逐次化する search iterator を設計する。今回の result budget 変更は不要な全 match 生成を止める構造改善として残す。
```

- [ ] **Step 2: Verify docs wording has no local absolute paths or process args**

Run:

```bash
rg -P -n "<real-worktree-path>|/tmp/markdown-view-search-many-match-budget\\.(?!\\*\\*\\*|XXXXXX)[A-Za-z0-9]+|target/debug/[m]arkdown-view" docs/superpowers/plans/2026-05-23-search-many-match-budget.md docs/todo/TODO.md
```

Replace `<real-worktree-path>` with the active worktree path before running. Expected: no output. `docs/todo/TODO.md` may mention `/tmp/markdown-view-search-many-match-budget.***`, and this plan may mention `/tmp/markdown-view-search-many-match-budget.XXXXXX`, but neither docs file should contain the real temporary path, real worktree path, or full process args.

- [ ] **Step 3: Commit the docs update**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: 検索many-match抑制結果を記録"
```

Expected: commit succeeds with only `docs/todo/TODO.md` staged.

### Task 8: Final Verification And Summary

**Files:**
- Reference: `src/server/files/search.rs`
- Reference: `docs/todo/TODO.md`
- Reference: `docs/superpowers/specs/2026-05-23-search-many-match-budget-design.md`

- [ ] **Step 1: Run final repository verification**

Run:

```bash
./verify.sh
```

Expected: all required checks pass.

- [ ] **Step 2: Inspect final diff against develop**

Run:

```bash
git diff --stat develop...HEAD
git diff --name-only develop...HEAD
```

Expected names:

```text
docs/superpowers/specs/2026-05-23-search-many-match-budget-design.md
docs/superpowers/plans/2026-05-23-search-many-match-budget.md
src/server/files/search.rs
docs/todo/TODO.md
```

`tests/integration/search.rs` appears only if Task 4 required an integration assertion update.

- [ ] **Step 3: Prepare completion report**

Report:

- Changed files with reason and rough line impact.
- Affected dependent files: `tests/integration/search.rs`, `/api/search` clients, `docs/todo/TODO.md`.
- Verification results: focused tests, `./verify.sh`, and 10MiB many-match measurement values.
- Security considerations: localhost-only availability risk reduced; Host/Origin/path/sanitize/CSP/file-size/cancellation contracts unchanged.
- Residual risks: full block extraction remains; context generation for 100 results remains; performance values are environment-dependent.
