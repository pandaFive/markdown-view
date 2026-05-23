# Directory Search Allocation Upper-Limit Measurement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ディレクトリ検索の allocation 削減が今すぐ必要かを、上限近傍の追加計測に基づいて判断し、`docs/todo/BACKLOG.md` に結論を残す。

**Architecture:** コードは変更せず、`/tmp` 配下の deterministic fixture と HTTP `/api/search` を使って result-limit 100 件、64 MiB 近傍、10 MiB 単一ファイル、RSS plateau を観測する。永続変更は `BACKLOG.md` の判断記録に限定し、測定中の JSON や Markdown fixture はリポジトリ外に置く。

**Tech Stack:** Rust, Cargo, existing markdown-view server, `curl`, `/usr/bin/time`, `ps`, `rg`, Markdown docs.

---

## File Structure

- Modify: `docs/todo/BACKLOG.md`
  - 上限近傍の実測値、Done 化判断、残余リスク、セキュリティ境界を記録する。
- Reference: `docs/superpowers/specs/2026-05-23-directory-search-allocation-upper-limit-design.md`
  - Done 化条件、非目標、セキュリティ境界を確認する。
- Reference: `docs/todo/BACKLOG.md`
  - 現在の P2 項目と前回計測結果を確認する。
- Temporary only: `/tmp/markdown-view-search-allocation-upper-limit.XXXXXX`
  - result-limit、64 MiB 近傍、10 MiB 単一ファイルの fixture と HTTP response JSON を保存する。コミットしない。
- No source changes:
  - `src/`, `tests/`, `Cargo.toml`, `Cargo.lock`, `package.json`, generated assets は変更しない。

## Scope Check

この plan は単一サブシステム、ディレクトリ検索 allocation 再判断だけを扱う。検索ロジック、API、UI、依存関係、セキュリティ境界は変更しない。計測でリスクが見つかった場合も、この plan では最適化を実装せず、Backlog に具体的な後続候補として残す。

### Task 1: Preflight And Environment Snapshot

**Files:**
- Reference: `docs/superpowers/specs/2026-05-23-directory-search-allocation-upper-limit-design.md`
- Reference: `docs/todo/BACKLOG.md`
- Modify later: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Confirm branch and worktree**

Run:

```bash
git branch --show-current
git status --short
pwd
```

Expected:

```text
docs/search-allocation-upper-limit
```

`git status --short` should be empty or show only docs changes made by this plan. `pwd` should be `/tmp/markdown-view-search-allocation-upper-limit`. If any source file is modified, inspect it and stop before measuring.

- [ ] **Step 2: Confirm measurement tools**

Run:

```bash
command -v /usr/bin/time
command -v curl
command -v ps
command -v rg
```

Expected: each command prints an executable path. If `/usr/bin/time` is missing, use shell `time` for elapsed and `ps` for server RSS, then record that maximum RSS for the test process was unavailable.

- [ ] **Step 3: Record environment summary**

Run:

```bash
date +%F
rustc --version
cargo --version
uname -srmo
```

Expected: four lines with date, Rust version, Cargo version, and OS/kernel summary. Record these in the later `BACKLOG.md` note. Do not record hostname, local username, or absolute fixture path.

- [ ] **Step 4: Confirm current backlog wording**

Run:

```bash
sed -n '1,80p' docs/todo/BACKLOG.md
```

Expected: the P2 item `ディレクトリ検索の allocation 削減を追加計測に基づいて再判断する` is visible and still unchecked.

### Task 2: Focused Search Test Baseline

**Files:**
- Reference: `src/server/files/search.rs`
- Reference: `tests/integration/search.rs`
- Modify later: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Warm focused search tests**

Run:

```bash
cargo test search --all-targets --all-features
```

Expected: command exits successfully. If it fails, investigate the failure before using any timing numbers.

- [ ] **Step 2: Measure focused search tests, run 1**

Run:

```bash
/usr/bin/time -v cargo test search --all-targets --all-features
```

Expected: command exits successfully and prints `Elapsed (wall clock) time` and `Maximum resident set size`. Record both values.

- [ ] **Step 3: Measure focused search tests, run 2**

Run:

```bash
/usr/bin/time -v cargo test search --all-targets --all-features
```

Expected: command exits successfully. Record elapsed and maximum RSS.

- [ ] **Step 4: Measure focused search tests, run 3**

Run:

```bash
/usr/bin/time -v cargo test search --all-targets --all-features
```

Expected: command exits successfully. Record elapsed and maximum RSS as a range across runs 1-3.

### Task 3: Build Temporary Upper-Limit Fixtures

**Files:**
- Temporary create: `/tmp/markdown-view-search-allocation-upper-limit.XXXXXX`
- Modify later: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Create fixture root**

Run:

```bash
SEARCH_FIXTURE_DIR="$(mktemp -d /tmp/markdown-view-search-allocation-upper-limit.XXXXXX)"
mkdir -p "$SEARCH_FIXTURE_DIR/result-limit" "$SEARCH_FIXTURE_DIR/near-64m" "$SEARCH_FIXTURE_DIR/single-10m" "$SEARCH_FIXTURE_DIR/responses"
printf 'SEARCH_FIXTURE_DIR=%s\n' "$SEARCH_FIXTURE_DIR"
```

Expected: prints a `/tmp/markdown-view-search-allocation-upper-limit.XXXXXX` path. Keep it in the shell for later commands. In committed docs, write only `/tmp/markdown-view-search-allocation-upper-limit.***`.

- [ ] **Step 2: Generate result-limit fixture with matches distributed across files**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
for i in $(seq 1 120); do
  file="$SEARCH_FIXTURE_DIR/result-limit/doc-$i.md"
  {
    printf '# Result Limit %03d\n\n' "$i"
    printf 'needle result limit paragraph %03d with stable surrounding context.\n\n' "$i"
    printf 'plain paragraph %03d without the query word.\n' "$i"
  } > "$file"
done
find "$SEARCH_FIXTURE_DIR/result-limit" -name '*.md' | wc -l
du -sh "$SEARCH_FIXTURE_DIR/result-limit"
```

Expected:

```text
120
```

Record the file count and approximate size. This fixture should hit result-limit 100 across multiple files.

- [ ] **Step 3: Generate 64 MiB near-limit fixture**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
for i in $(seq 1 70); do
  file="$SEARCH_FIXTURE_DIR/near-64m/doc-$i.md"
  {
    printf '# Near 64 MiB %03d\n\n' "$i"
    yes 'absent-control paragraph without the search token and with enough stable text to exercise markdown block extraction.' | head -13000
  } > "$file"
done
find "$SEARCH_FIXTURE_DIR/near-64m" -name '*.md' | wc -l
du -sh "$SEARCH_FIXTURE_DIR/near-64m"
```

Expected:

```text
70
```

`du -sh` should report a size near or above 64 MiB. If it is far below 60 MiB, add more files in the same pattern before measuring. If it is above 64 MiB, byte-limit truncation is acceptable and should produce `truncated_reasons=["byte_limit"]`.

- [ ] **Step 4: Generate 10 MiB single-file fixture**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
{
  printf '# Single 10 MiB\n\n'
  yes 'needle single large file paragraph with stable context for markdown parsing and search result generation.' | head -105000
} > "$SEARCH_FIXTURE_DIR/single-10m/single.md"
find "$SEARCH_FIXTURE_DIR/single-10m" -name '*.md' | wc -l
du -sh "$SEARCH_FIXTURE_DIR/single-10m/single.md"
wc -c "$SEARCH_FIXTURE_DIR/single-10m/single.md"
```

Expected:

```text
1
```

`wc -c` should be below the project file-size limit of 10 MiB. If it is above 10 MiB, regenerate with a smaller `head` count before measuring.

- [ ] **Step 5: Confirm fixture remains outside git**

Run:

```bash
git status --short
```

Expected: no fixture files appear because they are under `/tmp`.

### Task 4: Result-Limit HTTP Measurement

**Files:**
- Temporary read/write: `$SEARCH_FIXTURE_DIR/result-limit`
- Temporary write: `$SEARCH_FIXTURE_DIR/responses/result-limit-*.json`
- Modify later: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Start server for result-limit fixture**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
cargo run -- "$SEARCH_FIXTURE_DIR/result-limit" --port 3023
```

Expected: server starts and listens on `127.0.0.1:3023`. Keep this process running until Task 4 Step 6 completes.

- [ ] **Step 2: Identify server RSS before search**

In a second terminal/session, run:

```bash
ps -o pid,rss,comm,args -C markdown-view
```

Expected: identify the process whose args contain `--port 3023`. Record only PID and RSS in KiB. Do not paste full args or absolute fixture paths into committed docs.

- [ ] **Step 3: Smoke-test result-limit response contract**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
curl --fail-with-body -sS -w '\nhttp_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/responses/result-limit-smoke.json" -H 'Host: 127.0.0.1:3023' 'http://127.0.0.1:3023/api/search?q=needle'
rg -o '"searched_files":[0-9]+|"searched_bytes":[0-9]+|"truncated":(true|false)|"truncated_reasons":\[[^]]*\]' "$SEARCH_FIXTURE_DIR/responses/result-limit-smoke.json"
```

Expected: `http_code=200`, `truncated=true`, and `truncated_reasons=["result_limit"]`. Record `searched_files` and `searched_bytes`.

- [ ] **Step 4: Measure result-limit response, run 1**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
/usr/bin/time -v curl --fail-with-body -sS -w 'http_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/responses/result-limit-1.json" -H 'Host: 127.0.0.1:3023' 'http://127.0.0.1:3023/api/search?q=needle'
ps -o pid,rss,comm,args -C markdown-view
```

Expected: `http_code=200`; record elapsed, response size, and `--port 3023` server RSS.

- [ ] **Step 5: Measure result-limit response, runs 2-5**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
for n in 2 3 4 5; do
  /usr/bin/time -v curl --fail-with-body -sS -w "http_code=%{http_code} size=%{size_download}\n" -o "$SEARCH_FIXTURE_DIR/responses/result-limit-$n.json" -H 'Host: 127.0.0.1:3023' 'http://127.0.0.1:3023/api/search?q=needle'
  ps -o pid,rss,comm,args -C markdown-view
done
```

Expected: all four requests return `http_code=200`. Record elapsed range, response size range, and server RSS series. The `curl` maximum RSS is not used for server memory judgment.

- [ ] **Step 6: Stop result-limit server**

Stop the `cargo run` process with Ctrl-C.

Run:

```bash
ps -o pid,rss,comm,args -C markdown-view
```

Expected: no `--port 3023` process remains. Do not stop unrelated `markdown-view` processes.

### Task 5: 64 MiB Near-Limit HTTP Measurement

**Files:**
- Temporary read/write: `$SEARCH_FIXTURE_DIR/near-64m`
- Temporary write: `$SEARCH_FIXTURE_DIR/responses/near-64m-*.json`
- Modify later: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Start server for near-64m fixture**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
cargo run -- "$SEARCH_FIXTURE_DIR/near-64m" --port 3024
```

Expected: server starts and listens on `127.0.0.1:3024`. Keep this process running until Task 5 Step 6 completes.

- [ ] **Step 2: Record pre-search server RSS**

Run in a second terminal/session:

```bash
ps -o pid,rss,comm,args -C markdown-view
```

Expected: identify the `--port 3024` process and record only PID/RSS.

- [ ] **Step 3: Smoke-test near-64m response contract**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
curl --fail-with-body -sS -w '\nhttp_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/responses/near-64m-smoke.json" -H 'Host: 127.0.0.1:3024' 'http://127.0.0.1:3024/api/search?q=missingneedle'
rg -o '"searched_files":[0-9]+|"searched_bytes":[0-9]+|"truncated":(true|false)|"truncated_reasons":\[[^]]*\]' "$SEARCH_FIXTURE_DIR/responses/near-64m-smoke.json"
```

Expected: `http_code=200`. If fixture size exceeds the 64 MiB byte limit, expect `truncated=true` and `truncated_reasons=["byte_limit"]`. If fixture size remains under the limit, expect `truncated=false` and `truncated_reasons=[]`. Record which branch occurred, plus `searched_files` and `searched_bytes`.

- [ ] **Step 4: Measure near-64m response, run 1**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
/usr/bin/time -v curl --fail-with-body -sS -w 'http_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/responses/near-64m-1.json" -H 'Host: 127.0.0.1:3024' 'http://127.0.0.1:3024/api/search?q=missingneedle'
ps -o pid,rss,comm,args -C markdown-view
```

Expected: `http_code=200`; record elapsed, response size, and `--port 3024` server RSS.

- [ ] **Step 5: Measure near-64m response, runs 2-5**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
for n in 2 3 4 5; do
  /usr/bin/time -v curl --fail-with-body -sS -w "http_code=%{http_code} size=%{size_download}\n" -o "$SEARCH_FIXTURE_DIR/responses/near-64m-$n.json" -H 'Host: 127.0.0.1:3024' 'http://127.0.0.1:3024/api/search?q=missingneedle'
  ps -o pid,rss,comm,args -C markdown-view
done
```

Expected: all four requests return `http_code=200`. Record elapsed range, response size range, `searched_bytes` from saved responses if it changes, and server RSS series. If RSS increases every run without leveling, do not Done the backlog item.

- [ ] **Step 6: Stop near-64m server**

Stop the `cargo run` process with Ctrl-C.

Run:

```bash
ps -o pid,rss,comm,args -C markdown-view
```

Expected: no `--port 3024` process remains. Do not stop unrelated processes.

### Task 6: 10 MiB Single-File HTTP Measurement

**Files:**
- Temporary read/write: `$SEARCH_FIXTURE_DIR/single-10m`
- Temporary write: `$SEARCH_FIXTURE_DIR/responses/single-10m-*.json`
- Modify later: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Confirm single file remains below size limit**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
wc -c "$SEARCH_FIXTURE_DIR/single-10m/single.md"
```

Expected: byte count is below 10485760. If it exceeds that number, regenerate Task 3 Step 4 with a smaller `head` count.

- [ ] **Step 2: Start server for single-10m fixture**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
cargo run -- "$SEARCH_FIXTURE_DIR/single-10m" --port 3025
```

Expected: server starts and listens on `127.0.0.1:3025`. Keep this process running until Task 6 Step 7 completes.

- [ ] **Step 3: Record pre-search server RSS**

Run in a second terminal/session:

```bash
ps -o pid,rss,comm,args -C markdown-view
```

Expected: identify the `--port 3025` process and record only PID/RSS.

- [ ] **Step 4: Smoke-test single-file match response contract**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
curl --fail-with-body -sS -w '\nhttp_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/responses/single-10m-match-smoke.json" -H 'Host: 127.0.0.1:3025' 'http://127.0.0.1:3025/api/search?q=needle'
rg -o '"searched_files":[0-9]+|"searched_bytes":[0-9]+|"truncated":(true|false)|"truncated_reasons":\[[^]]*\]' "$SEARCH_FIXTURE_DIR/responses/single-10m-match-smoke.json"
```

Expected: `http_code=200`; likely `truncated=true` and `truncated_reasons=["result_limit"]` because the single file contains many matches. Record `searched_files`, `searched_bytes`, `truncated`, and `truncated_reasons`.

- [ ] **Step 5: Measure single-file match response, runs 1-3**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
for n in 1 2 3; do
  /usr/bin/time -v curl --fail-with-body -sS -w "http_code=%{http_code} size=%{size_download}\n" -o "$SEARCH_FIXTURE_DIR/responses/single-10m-match-$n.json" -H 'Host: 127.0.0.1:3025' 'http://127.0.0.1:3025/api/search?q=needle'
  ps -o pid,rss,comm,args -C markdown-view
done
```

Expected: all three requests return `http_code=200`. Record elapsed range, response size range, and server RSS series.

- [ ] **Step 6: Measure single-file no-match response, runs 1-3**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
for n in 1 2 3; do
  /usr/bin/time -v curl --fail-with-body -sS -w "http_code=%{http_code} size=%{size_download}\n" -o "$SEARCH_FIXTURE_DIR/responses/single-10m-nomatch-$n.json" -H 'Host: 127.0.0.1:3025' 'http://127.0.0.1:3025/api/search?q=missingneedle'
  ps -o pid,rss,comm,args -C markdown-view
done
```

Expected: all three requests return `http_code=200`; expected `truncated=false`, `truncated_reasons=[]`, `searched_files=1`, and `searched_bytes` near the file byte count. Record elapsed range, response size range, and server RSS series.

- [ ] **Step 7: Stop single-10m server**

Stop the `cargo run` process with Ctrl-C.

Run:

```bash
ps -o pid,rss,comm,args -C markdown-view
```

Expected: no `--port 3025` process remains. Do not stop unrelated processes.

### Task 7: Backlog Decision Update

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Reference: `docs/superpowers/specs/2026-05-23-directory-search-allocation-upper-limit-design.md`

- [ ] **Step 1: Decide outcome from measurements**

Use this rule:

```text
If all four required areas are measured and practical:
  Move the item to Done and state optimization is not justified now.
If any required area is not measured:
  Keep the item unchecked and name the exact missing measurement.
If any measurement shows concrete risk:
  Keep or split the item into a specific follow-up naming the suspected hotspot.
If measurement is inconclusive:
  Keep the item unchecked with the exact inconclusive condition.
```

Expected: exactly one branch is selected. Do not implement optimization in this plan.

- [ ] **Step 2: Edit backlog for Done outcome**

Apply this structure only if every Done condition was satisfied:

```markdown
- [x] ディレクトリ検索の allocation 削減を追加計測に基づいて再判断する
  - 完了根拠: 2026-05-23 に上限近傍の追加計測を行い、現行の検索上限内では allocation 削減を今すぐ実装する必要は低いと判断した。環境は Task 1 Step 3 で記録した date / rustc / cargo / OS summary。検索系 targeted tests は Task 2 の elapsed 範囲と最大 RSS 範囲。HTTP result-limit 100 件の複数ファイル fixture は Task 3 Step 2 の file count / size、Task 4 の `searched_files`、`searched_bytes`、`truncated=true`、`truncated_reasons=["result_limit"]`、elapsed 範囲、response size 範囲、server RSS 系列。64 MiB 近傍 fixture は Task 3 Step 3 の file count / size、Task 5 の `searched_files`、`searched_bytes`、`truncated`、`truncated_reasons`、elapsed 範囲、response size 範囲、server RSS 系列。10 MiB 単一ファイル fixture は Task 3 Step 4 / Task 6 Step 1 の byte count、Task 6 の match 経路統計、no-match 経路統計、server RSS 系列。反復後 RSS は継続増加せず plateau と判断した。fixture は `/tmp/markdown-view-search-allocation-upper-limit.***` に生成し、repo へ追加していない。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は変更していない
```

Before saving, replace the Task references in the paragraph with the actual recorded values so the committed `BACKLOG.md` is self-contained. Move the item from P2 to the top of `## Done`.

- [ ] **Step 3: Edit backlog for non-Done outcome**

Apply this structure if any Done condition was not satisfied:

```markdown
- [ ] ディレクトリ検索の allocation 削減を追加計測に基づいて再判断する
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数・query 長の打ち切り、クライアント単位の検索キャンセル境界、全体同時実行上限を持つ
  - 計測済み: 2026-05-23 の追加計測では、成功した各 Task の fixture 規模、`searched_files`、`searched_bytes`、`truncated`、`truncated_reasons`、elapsed 範囲、response size 範囲、server RSS 系列を記録した。fixture は `/tmp/markdown-view-search-allocation-upper-limit.***` に生成し、repo へ追加していない
  - 残件: Done 条件を満たせなかった Task 名、未測定条件、または観測されたリスクを具体的に記録する。その条件が解消されるまで Done 化しない
  - 判断: 検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は変更していない。現時点では完了扱いにせず、追加計測または具体的な最適化候補が必要な P2 残件として残す
```

Before saving, replace the Task-name wording with the actual measured values and concrete missing conditions so the committed `BACKLOG.md` is self-contained.

- [ ] **Step 4: Confirm docs-only change**

Run:

```bash
git diff --name-only
```

Expected:

```text
docs/todo/BACKLOG.md
docs/superpowers/plans/2026-05-23-directory-search-allocation-upper-limit.md
```

If the design file is still uncommitted in the current execution context, it may also appear. No `src/`, `tests/`, lockfile, or generated asset path should appear.

### Task 8: Verification And Commit

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Modify: `docs/superpowers/plans/2026-05-23-directory-search-allocation-upper-limit.md`

- [ ] **Step 1: Run unfinished-marker scan**

Run:

```bash
unfinished_matches="$(rg -n "T[B]D|TO[D]O|未[定]" docs/todo/BACKLOG.md docs/superpowers/plans/2026-05-23-directory-search-allocation-upper-limit.md | rg -v 'TODO\.md|T\[B\]D|TO\[D\]O|未\[定\]' || :)"
test -z "$unfinished_matches" || { printf '%s\n' "$unfinished_matches"; exit 1; }
```

Expected: command exits successfully. Literal references to `TODO.md` are filtered because they are existing document titles, not unfinished work markers.

- [ ] **Step 2: Confirm required security boundary language remains**

Run:

```bash
for pattern in 'SearchResponse' 'Host/Origin' 'path validation' 'HTML sanitize' 'CSP' '検索キャンセル' '検索上限'; do
  rg -n --fixed-strings -- "$pattern" docs/todo/BACKLOG.md >/dev/null
done
```

Expected: command exits successfully. If it fails, update `BACKLOG.md` so the unchanged security and API boundaries are explicit.

- [ ] **Step 3: Check whitespace and diff**

Run:

```bash
git diff --check
git diff -- docs/todo/BACKLOG.md docs/superpowers/plans/2026-05-23-directory-search-allocation-upper-limit.md
```

Expected: no whitespace errors. Diff should only contain the plan and the backlog decision update.

- [ ] **Step 4: Confirm no source changes**

Run:

```bash
git diff --name-only | rg '^(src|tests|Cargo\.toml|Cargo\.lock|package\.json|src/template/assets/generated-js)' || true
```

Expected: no output. If output appears, stop and inspect before committing.

- [ ] **Step 5: Commit docs update**

Run:

```bash
git add docs/todo/BACKLOG.md docs/superpowers/plans/2026-05-23-directory-search-allocation-upper-limit.md
git commit -m "docs: 検索allocation上限計測結果を記録"
```

Expected: one docs commit is created. Do not include temporary fixture files or source changes.

- [ ] **Step 6: Final status**

Run:

```bash
git status --short --branch
git log --oneline -3
```

Expected: worktree is clean, and the latest commits include the plan/result docs commit and the design commit.

## Self-Review

- Spec coverage: The plan covers result-limit 100 件, 64 MiB 近傍, 10 MiB 単一ファイル, RSS plateau, docs-only scope, Done/non-Done decision rules, security boundaries, and verification.
- Unfinished-marker scan: Task 8 Step 1 fails if common unfinished markers remain in the executable result.
- Type and command consistency: Ports are fixed to 3023, 3024, and 3025; fixture root is consistently `SEARCH_FIXTURE_DIR`; all HTTP requests include the matching `Host` header.
