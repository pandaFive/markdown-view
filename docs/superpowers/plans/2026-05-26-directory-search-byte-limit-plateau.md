# Directory Search Byte-Limit RSS Plateau Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 64MiB byte-limit 近傍検索の RSS peak / after / settled を測定し、必要な場合は byte-limit 超過候補ファイルを本文読込前に打ち切る。

**Architecture:** まず `/tmp` fixture と対象 server PID の短周期 RSS sampling で dev / release、cold / warm、peak / after / settled を分けて測定する。測定で必要性が確認できたら、`search_directory_with_limits_blocking()` に metadata ベースの読込前 byte-budget 判定を追加し、読んだファイルだけを `searched_files` / `searched_bytes` に含める既存契約を維持する。

**Tech Stack:** Rust, Cargo, Axum server, existing search module, shell, curl, ps, awk, Markdown docs.

---

## File Structure

- Reference: `docs/superpowers/specs/2026-05-26-directory-search-byte-limit-plateau-design.md`
  - 承認済み設計。測定項目、実装分岐、セキュリティ境界を確認する。
- Modify after measurement: `docs/todo/BACKLOG.md`
  - 測定結果、実装判断、残余リスク、検証結果を記録する。
- Modify if implementation is needed: `src/server/files/search.rs`
  - byte-limit 超過候補ファイルを本文読込前に判定する。
  - test-only read counter hook を追加して、超過候補ファイルを読まないことを固定する。
- Temporary only: `/tmp/markdown-view-search-byte-limit-plateau.XXXXXX`
  - 測定 fixture、HTTP response、RSS sample を保存する。repo へ追加しない。

## Scope Check

この plan は単一サブシステム、ディレクトリ検索 byte-limit 経路だけを扱う。検索 API の JSON shape、検索結果の意味、UI、Host/Origin 検証、path validation の public contract、HTML sanitization、CSP、検索インデックス、parser 全面逐次化は扱わない。内部実装を base identity 検証と capability-based access に寄せる場合も、base 外、hidden、非 Markdown、symlink 差し替え拒否は弱めない。

測定だけで plateau が許容できると判断できた場合は Task 4 から Task 6 へ進み、Task 5 のコード変更は実行しない。plateau しない、または byte-limit 超過候補ファイル読込の peak RSS が明確に大きい場合だけ Task 5 を実行する。

### Task 1: Preflight And Branch Check

**Files:**
- Reference: `docs/superpowers/specs/2026-05-26-directory-search-byte-limit-plateau-design.md`
- Reference: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Confirm branch and worktree**

Run:

```bash
git branch --show-current
git status --short
pwd
```

Expected:

```text
docs/byte-limit-plateau-design
```

`git status --short` should be empty before starting execution. `pwd` should be `/tmp/markdown-view-byte-limit-plateau-design`.

- [ ] **Step 2: Read the accepted design**

Run:

```bash
sed -n '1,220p' docs/superpowers/specs/2026-05-26-directory-search-byte-limit-plateau-design.md
```

Expected: the design includes `peak / after / settled RSS`, the `near-64m-many-files` and `overshoot-file` fixtures, and the optional metadata byte-budget implementation.

- [ ] **Step 3: Confirm current backlog item**

Run:

```bash
rg -n "64MiB byte-limit|RSS plateau|searched_bytes|truncated_reasons" docs/todo/BACKLOG.md
```

Expected: the P2 item for `ディレクトリ検索 64MiB byte-limit 反復時の RSS plateau` is visible and still unchecked.

- [ ] **Step 4: Confirm required tools**

Run:

```bash
command -v cargo
command -v curl
command -v ps
command -v awk
command -v date
command -v mktemp
```

Expected: each command prints an executable path.

- [ ] **Step 5: Record environment summary**

Run:

```bash
date +%F
rustc --version
cargo --version
uname -srmo
```

Expected: four lines with date, Rust version, Cargo version, and OS/kernel summary. Record these later without hostname, username, or absolute fixture path.

### Task 2: Create Byte-Limit Measurement Fixtures

**Files:**
- Temporary create: `/tmp/markdown-view-search-byte-limit-plateau.XXXXXX`

- [ ] **Step 1: Create fixture root**

Run:

```bash
SEARCH_FIXTURE_DIR="$(mktemp -d /tmp/markdown-view-search-byte-limit-plateau.XXXXXX)"
mkdir -p "$SEARCH_FIXTURE_DIR/near-64m-many-files" "$SEARCH_FIXTURE_DIR/overshoot-file" "$SEARCH_FIXTURE_DIR/responses" "$SEARCH_FIXTURE_DIR/rss"
printf 'SEARCH_FIXTURE_DIR=%s\n' "$SEARCH_FIXTURE_DIR"
```

Expected: prints a `/tmp/markdown-view-search-byte-limit-plateau.XXXXXX` path. Keep the variable in the shell for the rest of this task. In committed docs, write only `/tmp/markdown-view-search-byte-limit-plateau.***`.

- [ ] **Step 2: Generate near-64m-many-files fixture**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
for i in $(seq 1 70); do
  file="$SEARCH_FIXTURE_DIR/near-64m-many-files/doc-$i.md"
  {
    printf '# Near 64 MiB %03d\n\n' "$i"
    yes 'absent-control paragraph without the search token and with stable text to exercise markdown block extraction.' | head -15000
  } > "$file"
done
find "$SEARCH_FIXTURE_DIR/near-64m-many-files" -name '*.md' | wc -l
du -sh "$SEARCH_FIXTURE_DIR/near-64m-many-files"
```

Expected: `wc -l` prints `70`. `du -sh` should report a size above 64 MiB.

- [ ] **Step 3: Generate overshoot-file fixture**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
for i in $(seq 1 42); do
  file="$SEARCH_FIXTURE_DIR/overshoot-file/doc-$i.md"
  {
    printf '# Overshoot Prefix %03d\n\n' "$i"
    yes 'absent-control paragraph without the search token and with stable text to approach the byte budget.' | head -15000
  } > "$file"
done
{
  printf '# Overshoot Trigger\n\n'
  yes 'absent-control paragraph without the search token and with stable text that should not need to be read after metadata byte-budget checking.' | head -50000
} > "$SEARCH_FIXTURE_DIR/overshoot-file/zz-overshoot.md"
find "$SEARCH_FIXTURE_DIR/overshoot-file" -name '*.md' | wc -l
du -sh "$SEARCH_FIXTURE_DIR/overshoot-file"
```

Expected: `wc -l` prints `43`. `du -sh` should report a size above 64 MiB, with `zz-overshoot.md` as the final sorted file.

- [ ] **Step 4: Record fixture summary without absolute paths**

Run:

```bash
printf 'near-64m files='
find "$SEARCH_FIXTURE_DIR/near-64m-many-files" -name '*.md' | wc -l
printf 'near-64m size='
du -sh "$SEARCH_FIXTURE_DIR/near-64m-many-files" | awk '{print $1}'
printf 'overshoot files='
find "$SEARCH_FIXTURE_DIR/overshoot-file" -name '*.md' | wc -l
printf 'overshoot size='
du -sh "$SEARCH_FIXTURE_DIR/overshoot-file" | awk '{print $1}'
```

Expected: file counts and approximate sizes only. Do not paste absolute paths into committed docs.

### Task 3: Baseline Measurement

**Files:**
- Temporary create: `/tmp/markdown-view-search-byte-limit-plateau.XXXXXX/responses/*`
- Temporary create: `/tmp/markdown-view-search-byte-limit-plateau.XXXXXX/rss/*`
- Modify later: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Build debug binary once**

Run:

```bash
cargo build
```

Expected: exits successfully. If it fails, stop and fix the build before measuring.

- [ ] **Step 2: Define measurement helper in the measuring shell**

Run in the shell that will issue HTTP requests:

```bash
measure_search_run() {
  local port="$1"
  local label="$2"
  local run="$3"
  local server_pid
  server_pid="${MARKDOWN_VIEW_SERVER_PID:?set MARKDOWN_VIEW_SERVER_PID from the server shell before measuring}"
  test -n "$server_pid"
  printf 'SERVER_PID=%s label=%s run=%s\n' "$server_pid" "$label" "$run"
  ps -o pid=,rss=,comm= -p "$server_pid"

  (
    while kill -0 "$server_pid" 2>/dev/null; do
      printf '%s ' "$(date +%s%3N)"
      ps -o rss= -p "$server_pid"
      sleep 0.05
    done
  ) > "$SEARCH_FIXTURE_DIR/rss/${label}-run${run}.tsv" &
  local sampler_pid=$!
  local start_ms
  local end_ms
  start_ms="$(date +%s%3N)"
  curl -sS --fail-with-body -H "Host: 127.0.0.1:${port}" "http://127.0.0.1:${port}/api/search?q=missingneedle" -o "$SEARCH_FIXTURE_DIR/responses/${label}-run${run}.json"
  end_ms="$(date +%s%3N)"
  kill "$sampler_pid"
  wait "$sampler_pid" 2>/dev/null || true
  local after_rss
  local settled_rss
  local peak_rss
  after_rss="$(ps -o rss= -p "$server_pid" | awk '{print $1}')"
  sleep 5
  settled_rss="$(ps -o rss= -p "$server_pid" | awk '{print $1}')"
  peak_rss="$(awk 'NF >= 2 { if ($2 > max) max=$2 } END { print max+0 }' "$SEARCH_FIXTURE_DIR/rss/${label}-run${run}.tsv")"
  printf 'label=%s run=%s elapsed_ms=%s peak_rss_kib=%s after_rss_kib=%s settled_rss_kib=%s\n' "$label" "$run" "$((end_ms - start_ms))" "$peak_rss" "$after_rss" "$settled_rss"
}
```

Expected: the function is defined with no output.

- [ ] **Step 3: Start debug server for near-64m fixture**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
cargo build
target/debug/markdown-view "$SEARCH_FIXTURE_DIR/near-64m-many-files" --port 3026 &
export MARKDOWN_VIEW_SERVER_PID=$!
printf 'MARKDOWN_VIEW_SERVER_PID=%s\n' "$MARKDOWN_VIEW_SERVER_PID"
ps -o pid=,comm= -p "$MARKDOWN_VIEW_SERVER_PID"
```

Expected: server starts and binds to `127.0.0.1:3026`, and `comm` is `markdown-view`. If measuring from another terminal/session, copy the printed PID and run `export MARKDOWN_VIEW_SERVER_PID=<pid>` in the measuring shell before calling `measure_search_run`.

- [ ] **Step 4: Run near-64m debug cold request with RSS sampling**

Run in another terminal/session:

```bash
measure_search_run 3026 near-debug 1
```

Expected: curl exits successfully. The printed line includes elapsed, peak RSS, after RSS, and settled RSS. Do not record full process args in docs.

- [ ] **Step 5: Validate near-64m debug cold response**

Run:

```bash
rg -n '"searched_files"|"searched_bytes"|"truncated"|"truncated_reasons"|"byte_limit"' "$SEARCH_FIXTURE_DIR/responses/near-debug-run1.json"
```

Expected: output includes `"truncated":true` and `"byte_limit"` in `truncated_reasons`.

- [ ] **Step 6: Run near-64m debug warm requests**

Run:

```bash
measure_search_run 3026 near-debug 2
measure_search_run 3026 near-debug 3
measure_search_run 3026 near-debug 4
measure_search_run 3026 near-debug 5
```

Expected: four successful runs. Record the elapsed and RSS series, not a single cherry-picked value.

- [ ] **Step 7: Stop debug server**

Stop the debug server:

```bash
kill "$MARKDOWN_VIEW_SERVER_PID"
wait "$MARKDOWN_VIEW_SERVER_PID" 2>/dev/null || true
```

Expected: server exits. Confirm:

```bash
ps -o pid=,rss=,comm= -p "$MARKDOWN_VIEW_SERVER_PID"
```

Expected: no output.

- [ ] **Step 8: Measure debug overshoot fixture**

Start:

```bash
cargo build
target/debug/markdown-view "$SEARCH_FIXTURE_DIR/overshoot-file" --port 3027 &
export MARKDOWN_VIEW_SERVER_PID=$!
ps -o pid=,comm= -p "$MARKDOWN_VIEW_SERVER_PID"
```

Expected: `comm` is `markdown-view`.

Run in the measuring shell:

```bash
measure_search_run 3027 overshoot-debug 1
measure_search_run 3027 overshoot-debug 2
measure_search_run 3027 overshoot-debug 3
measure_search_run 3027 overshoot-debug 4
measure_search_run 3027 overshoot-debug 5
rg -n '"searched_files"|"searched_bytes"|"truncated"|"truncated_reasons"|"byte_limit"' "$SEARCH_FIXTURE_DIR/responses/overshoot-debug-run1.json"
```

Stop the debug server with `kill "$MARKDOWN_VIEW_SERVER_PID"` and `wait "$MARKDOWN_VIEW_SERVER_PID" 2>/dev/null || true`.

Confirm:

```bash
ps -o pid=,rss=,comm= -p "$MARKDOWN_VIEW_SERVER_PID"
```

Expected: response includes `"truncated":true` and `"byte_limit"`. The final `ps` command prints no output.

- [ ] **Step 9: Measure release near-64m fixture**

Run:

```bash
cargo build --release
```

Expected: exits successfully.

Start:

```bash
target/release/markdown-view "$SEARCH_FIXTURE_DIR/near-64m-many-files" --port 3028 &
export MARKDOWN_VIEW_SERVER_PID=$!
```

Run in the measuring shell:

```bash
measure_search_run 3028 near-release 1
measure_search_run 3028 near-release 2
measure_search_run 3028 near-release 3
measure_search_run 3028 near-release 4
measure_search_run 3028 near-release 5
rg -n '"searched_files"|"searched_bytes"|"truncated"|"truncated_reasons"|"byte_limit"' "$SEARCH_FIXTURE_DIR/responses/near-release-run1.json"
```

Stop the release server with `kill "$MARKDOWN_VIEW_SERVER_PID"` and `wait "$MARKDOWN_VIEW_SERVER_PID" 2>/dev/null || true`.

Confirm:

```bash
ps -o pid=,rss=,comm= -p "$MARKDOWN_VIEW_SERVER_PID"
```

Expected: release near-64m cold and warm series are recorded. The final `ps` command prints no output.

- [ ] **Step 10: Measure release overshoot fixture**

Start:

```bash
target/release/markdown-view "$SEARCH_FIXTURE_DIR/overshoot-file" --port 3029 &
export MARKDOWN_VIEW_SERVER_PID=$!
```

Run in the measuring shell:

```bash
measure_search_run 3029 overshoot-release 1
measure_search_run 3029 overshoot-release 2
measure_search_run 3029 overshoot-release 3
measure_search_run 3029 overshoot-release 4
measure_search_run 3029 overshoot-release 5
rg -n '"searched_files"|"searched_bytes"|"truncated"|"truncated_reasons"|"byte_limit"' "$SEARCH_FIXTURE_DIR/responses/overshoot-release-run1.json"
```

Stop the release server with `kill "$MARKDOWN_VIEW_SERVER_PID"` and `wait "$MARKDOWN_VIEW_SERVER_PID" 2>/dev/null || true`.

Confirm:

```bash
ps -o pid=,rss=,comm= -p "$MARKDOWN_VIEW_SERVER_PID"
```

Expected: release overshoot cold and warm series are recorded. The final `ps` command prints no output.

### Task 4: Measurement Decision

**Files:**
- Reference: `/tmp/markdown-view-search-byte-limit-plateau.XXXXXX/responses/*`
- Reference: `/tmp/markdown-view-search-byte-limit-plateau.XXXXXX/rss/*`
- Modify later: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Summarize response contracts**

Run:

```bash
for file in "$SEARCH_FIXTURE_DIR"/responses/*.json; do
  printf '%s ' "$(basename "$file")"
  rg -o '"searched_files":[0-9]+|"searched_bytes":[0-9]+|"truncated":(true|false)|"truncated_reasons":\[[^]]*\]' "$file" | tr '\n' ' '
  printf '\n'
done
```

Expected: every response includes `searched_files`, `searched_bytes`, `truncated`, and `truncated_reasons`. The byte-limit runs include `"byte_limit"`.

- [ ] **Step 2: Summarize peak RSS from sample files**

Run:

```bash
for file in "$SEARCH_FIXTURE_DIR"/rss/*.tsv; do
  printf '%s peak_rss_kib=' "$(basename "$file")"
  awk 'NF >= 2 { if ($2 > max) max=$2 } END { print max+0 }' "$file"
done
```

Expected: one peak RSS line per run.

- [ ] **Step 3: Decide whether implementation is required**

Use this rule:

- If settled RSS for debug and release warm runs reaches a stable range and peak RSS is acceptable for a localhost preview tool, skip Task 5 and update `BACKLOG.md` with measurement-only conclusion.
- If settled RSS grows across warm runs, or overshoot peak RSS is materially higher than the searched bytes would justify, execute Task 5.

Expected: write the decision in working notes before editing docs. The committed `BACKLOG.md` entry must state whether Task 5 was executed.

### Task 5: Implement Pre-Read Byte-Budget Guard

**Files:**
- Modify: `src/server/files/search.rs`
- Test: `src/server/files/search.rs`

- [ ] **Step 1: Add failing read-count regression test and hook**

In `src/server/files/search.rs`, add this test-only hook near the existing search test hooks:

```rust
#[cfg(test)]
std::thread_local! {
    static SEARCH_MARKDOWN_READ_COUNT_FOR_TEST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_search_markdown_read_count_for_test() -> usize {
    SEARCH_MARKDOWN_READ_COUNT_FOR_TEST.with(|count| {
        count.set(0);
        count.get()
    })
}

#[cfg(test)]
fn search_markdown_read_count_for_test() -> usize {
    SEARCH_MARKDOWN_READ_COUNT_FOR_TEST.with(std::cell::Cell::get)
}

#[cfg(test)]
fn notify_search_markdown_read_for_test() {
    SEARCH_MARKDOWN_READ_COUNT_FOR_TEST.with(|count| count.set(count.get() + 1));
}

#[cfg(not(test))]
fn notify_search_markdown_read_for_test() {}
```

Then add the notify call as the first line in `read_markdown_with_limit_blocking()`:

```rust
fn read_markdown_with_limit_blocking(file_path: &Path) -> std::io::Result<String> {
    notify_search_markdown_read_for_test();
    let metadata = std::fs::metadata(file_path)?;
```

Finally, update the existing test `test_search_directory_総読込バイト上限到達を明示し超過ファイルは検索しない()`:

```rust
    #[tokio::test]
    async fn test_search_directory_総読込バイト上限到達を明示し超過ファイルは検索しない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle").unwrap();
        std::fs::write(dir.path().join("b.md"), "needle should not be searched").unwrap();

        let canonical = canonical_of(dir.path());
        reset_search_markdown_read_count_for_test();
        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: "needle".len(),
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Byte]
        );
        assert_eq!(response.searched_files, 1);
        assert_eq!(response.searched_bytes, "needle".len());
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].file, "a.md");
        assert_eq!(
            search_markdown_read_count_for_test(),
            1,
            "byte-limit超過候補ファイルは本文読込前に打ち切る"
        );
    }
```

- [ ] **Step 2: Run the targeted test and verify it fails**

Run:

```bash
cargo test server::files::search::tests::test_search_directory_総読込バイト上限到達を明示し超過ファイルは検索しない -- --nocapture
```

Expected: FAIL because current code reads both `a.md` and `b.md`, so `search_markdown_read_count_for_test()` is `2`.

- [ ] **Step 3: Add capability-based byte-budget helper**

In `src/server/files/search.rs`, add a helper after `log_search_cancelled()` that:

- Opens the candidate from the canonical base directory capability instead of re-opening an ambient absolute path.
- Uses metadata from the opened handle for `MAX_FILE_SIZE` and byte-budget checks.
- Returns a byte-limit outcome as soon as the opened handle metadata would exceed the remaining byte budget.
- Returns an error for open/metadata failure so the caller keeps the existing `skipped_files` contract for files that cannot be inspected.

- [ ] **Step 4: Use the helper before building the Markdown String**

In `search_directory_with_limits_blocking()`, use the verified base directory capability and pass each listed relative path to the capability-based helper. The helper performs canonical relative validation and NoFollow open before reading. If it returns byte-limit, re-check cancellation before marking `SearchTruncationReason::Byte`; if it returns a Markdown `String`, continue through the existing search flow; if it returns an error, log and increment `skipped_files`.

Keep the existing post-read check:

```rust
        if stats.searched_bytes.saturating_add(markdown.len()) > limits.max_bytes {
            stats.mark_truncated(SearchTruncationReason::Byte);
            break;
        }
```

- [ ] **Step 5: Run the targeted test and verify it passes**

Run:

```bash
cargo test server::files::search::tests::test_search_directory_総読込バイト上限到達を明示し超過ファイルは検索しない -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Add helper edge tests**

Add tests near the byte-limit search tests in `src/server/files/search.rs` for: budget-in-range reads, over-budget files return byte-limit before `String` construction even when their contents are invalid UTF-8, metadata/open failure returns an error for skip handling, and `MAX_FILE_SIZE + 1` returns an error for skip handling.

- [ ] **Step 7: Run focused search tests**

Run:

```bash
cargo test search --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 8: Review implementation diff**

Run:

```bash
git status --short
git diff -- src/server/files/search.rs
```

Expected: only the intended search implementation is changed. Commit only after explicit user approval and a final clean verification pass.

### Task 6: Re-Measure And Update Backlog

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Reference: `/tmp/markdown-view-search-byte-limit-plateau.XXXXXX/responses/*`
- Reference: `/tmp/markdown-view-search-byte-limit-plateau.XXXXXX/rss/*`

- [ ] **Step 1: Re-run measurement if Task 5 changed code**

If Task 5 was executed, run these commands for the overshoot fixture after rebuilding the debug and release binaries:

```bash
cargo build
target/debug/markdown-view "$SEARCH_FIXTURE_DIR/overshoot-file" --port 3030 &
export MARKDOWN_VIEW_SERVER_PID=$!
ps -o pid=,comm= -p "$MARKDOWN_VIEW_SERVER_PID"
```

Expected: `comm` is `markdown-view`.

In the measuring shell:

```bash
measure_search_run 3030 overshoot-debug-after 1
measure_search_run 3030 overshoot-debug-after 2
measure_search_run 3030 overshoot-debug-after 3
measure_search_run 3030 overshoot-debug-after 4
measure_search_run 3030 overshoot-debug-after 5
rg -n '"searched_files"|"searched_bytes"|"truncated"|"truncated_reasons"|"byte_limit"' "$SEARCH_FIXTURE_DIR/responses/overshoot-debug-after-run1.json"
```

Stop the debug server with `kill "$MARKDOWN_VIEW_SERVER_PID"` and `wait "$MARKDOWN_VIEW_SERVER_PID" 2>/dev/null || true`, then run:

```bash
cargo build --release
target/release/markdown-view "$SEARCH_FIXTURE_DIR/overshoot-file" --port 3031 &
export MARKDOWN_VIEW_SERVER_PID=$!
```

In the measuring shell:

```bash
measure_search_run 3031 overshoot-release-after 1
measure_search_run 3031 overshoot-release-after 2
measure_search_run 3031 overshoot-release-after 3
measure_search_run 3031 overshoot-release-after 4
measure_search_run 3031 overshoot-release-after 5
rg -n '"searched_files"|"searched_bytes"|"truncated"|"truncated_reasons"|"byte_limit"' "$SEARCH_FIXTURE_DIR/responses/overshoot-release-after-run1.json"
```

Stop the release server with `kill "$MARKDOWN_VIEW_SERVER_PID"` and `wait "$MARKDOWN_VIEW_SERVER_PID" 2>/dev/null || true`.

Expected: response still includes `"byte_limit"`, and `searched_bytes` does not include the overshoot file. Record before/after peak and settled RSS series.

- [ ] **Step 2: Update BACKLOG P2 item**

Edit `docs/todo/BACKLOG.md` P2 item for `ディレクトリ検索 64MiB byte-limit 反復時の RSS plateau を測定方法改善込みで再確認する`.

If Task 5 was not executed, keep the item open only if measurement remains inconclusive. If measurement is conclusive and no implementation is needed, move it to Done with evidence.

If Task 5 was executed, update the P2 item with:

```markdown
  - 計測済み: 2026-05-26 に `/tmp/markdown-view-search-byte-limit-plateau.***` の near-64m-many-files と overshoot-file fixture で、dev/release、cold/warm、request 中 peak RSS、request 後 after RSS、5秒待機後 settled RSS を分けて測定した。HTTP response は `searched_files`、`searched_bytes`、`truncated=true`、`truncated_reasons=["byte_limit"]` を維持した。実パス、full process args、本文断片は記録していない
  - 対応: 測定で byte-limit 超過候補ファイルの本文読込が peak RSS に寄与し得ることを確認したため、`src/server/files/search.rs` で base directory capability から検索対象を開き、open 済み handle の metadata size と残り byte 予算を本文 `String` 構築前に比較する構成にした。本文読込後の既存 `markdown.len()` チェックは残し、capability 外 symlink 差し替え、TOCTOU、特殊ファイルシステム差異に備えている
  - 検証: `cargo test search --all-targets --all-features`、`./verify.sh` が通過した。再測定では overshoot-file の `searched_bytes` が読込済みファイル分に留まり、超過候補ファイルは検索対象に含めないことを確認した
  - 判断: byte-limit 超過候補ファイルの不要な本文読込は解消した。RSS plateau の最終判断は再測定結果に基づき、許容範囲なら Done 化し、settled RSS が継続増加する場合は allocator / parser / JSON 直列化の追加切り分けとして別項目化する
```

Replace the measurement values in prose with the actual observed values. Keep the security boundary sentence if it already exists, and do not record absolute paths.

- [ ] **Step 3: Run docs checks**

Run:

```bash
rg -n "64MiB byte-limit|peak RSS|settled RSS|byte_limit|Host/Origin|path validation|HTML sanitize|CSP" docs/todo/BACKLOG.md docs/superpowers/specs/2026-05-26-directory-search-byte-limit-plateau-design.md
git diff --check
```

Expected: required phrases are present and `git diff --check` exits successfully.

- [ ] **Step 4: Review docs diff**

Run:

```bash
git status --short
git diff -- docs/todo/BACKLOG.md
```

Expected: docs changes are limited to the measurement record. Commit only after explicit user approval and a final clean verification pass.

### Task 7: Final Verification

**Files:**
- Reference: `src/server/files/search.rs`
- Reference: `docs/todo/BACKLOG.md`
- Reference: `docs/superpowers/specs/2026-05-26-directory-search-byte-limit-plateau-design.md`

- [ ] **Step 1: Run full verification**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 2: Confirm final git state**

Run:

```bash
git status --short --branch
git log --oneline -5
```

Expected: before explicit user approval to commit, `git status` shows only the intended uncommitted implementation/docs diff. After commit, working tree is clean and recent commits include the plan/spec commits and any implementation/docs commits from this execution.

- [ ] **Step 3: Prepare completion report**

Report:

- Changed files and rough line impact.
- Affected dependent files.
- Measurement summary with fixture names but no absolute paths.
- Test and verification results.
- Residual risks, including whether RSS plateau is fully resolved or still needs allocator / parser / JSON follow-up.

## Self-Review

- Spec coverage: measurement fixtures, peak / after / settled RSS, dev / release, cold / warm, optional pre-read byte-budget implementation, security boundaries, rollback, and residual risks are covered by Tasks 1-7.
- Placeholder scan: this plan uses concrete paths, commands, expected results, and code snippets. It contains no unresolved placeholders.
- Type consistency: functions and types match the current codebase names: `search_directory_with_limits_blocking`, `SearchLimits`, `SearchTruncationReason::Byte`, `MAX_FILE_SIZE`, and `SearchResponse`.
