# Directory Search Allocation Measurement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ディレクトリ検索の allocation 削減が必要かを、依存追加なしの計測結果に基づいて判断し、`docs/todo/BACKLOG.md` に結論を残す。

**Architecture:** コードは変更せず、検索系 targeted tests と HTTP `/api/search` 経由の 2 層で実行時間・RSS・検索レスポンス統計を観測する。fixture は `mktemp -d` で作った `/tmp` 配下の一意ディレクトリに生成し、永続変更は計測結果と判断を記録する docs 更新だけに限定する。

**Tech Stack:** Rust, Cargo, existing integration tests, `cargo run`, `curl`, `/usr/bin/time` or shell `time`, `ps`, Markdown docs.

---

## File Structure

- Modify: `docs/todo/BACKLOG.md`
  - 計測結果と判断を P2 項目に追記し、Done 化するか、保留理由付きで残すか、後続最適化候補へ分割する。
- Reference: `docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md`
  - 目的、非目標、判断基準、セキュリティ境界を確認する。
- No persistent fixture files
  - 計測用 Markdown は `mktemp -d` で作った `/tmp` 配下の一意ディレクトリに生成し、リポジトリへ追加しない。
- No source changes
  - `src/`, `tests/`, `Cargo.toml`, `Cargo.lock` は変更しない。

## Scope Check

この plan は単一サブシステム、ディレクトリ検索 allocation 計測だけを扱う。検索ロジック変更、依存追加、bench harness 追加、UI 変更は含めない。

### Task 1: Baseline And Tool Inventory

**Files:**
- Reference: `docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md`
- Modify later: `docs/todo/BACKLOG.md`

- [ ] **Step 0: Confirm execution approval**

Run no command. Confirm in the current session that the user explicitly asked to execute this plan. If approval is missing, stop before changing docs or committing.

- [ ] **Step 1: Confirm branch and clean worktree**

Run:

```bash
git branch --show-current
git status --short
```

Expected:

```text
docs/directory-search-allocation-measurement
```

`git status --short` should be empty before measurement starts. If it is not empty, inspect the paths and do not overwrite unrelated user changes.

- [ ] **Step 2: Confirm available measurement tools**

Run:

```bash
command -v /usr/bin/time
command -v curl
command -v ps
command -v hyperfine || printf 'hyperfine unavailable\n'
```

Expected:

```text
/usr/bin/time
/usr/bin/curl
/usr/bin/ps
```

`hyperfine` may print either its path or `hyperfine unavailable`. If absent, use repeated `/usr/bin/time -v` and shell loops.

- [ ] **Step 3: Record environment summary for the backlog note**

Run:

```bash
rustc --version
cargo --version
uname -srmo
```

Expected: each command prints one line. Copy the Rust version, Cargo version, OS/kernel summary, and date into the measurement note added in Task 4. Do not copy hostname, local username, or absolute local paths into docs.

### Task 2: Search-Test Measurement

**Files:**
- Modify later: `docs/todo/BACKLOG.md`
- Do not modify: `src/server/files/search.rs`
- Do not modify: `tests/integration/search.rs`

- [ ] **Step 1: Run the focused Rust search tests once to warm build artifacts**

Run:

```bash
cargo test search --all-targets --all-features
```

Expected: test command completes successfully. The exact test count may change; failure must be investigated before using timing numbers.

- [ ] **Step 2: Measure focused search tests with verbose time output**

Run:

```bash
/usr/bin/time -v cargo test search --all-targets --all-features
```

Expected: command exits successfully and prints `Elapsed (wall clock) time` and `Maximum resident set size`. Record both values.

- [ ] **Step 3: Repeat focused measurement twice more**

Run:

```bash
/usr/bin/time -v cargo test search --all-targets --all-features
/usr/bin/time -v cargo test search --all-targets --all-features
```

Expected: both commands pass. Record the three elapsed-time values and three maximum-RSS values as a range, not as a single absolute truth.

- [ ] **Step 4: Confirm no source files changed**

Run:

```bash
git status --short
```

Expected: still empty. If any `src/`, `tests/`, `Cargo.toml`, or `Cargo.lock` path appears, stop and inspect before proceeding.

### Task 3: HTTP Search Measurement

**Files:**
- Modify later: `docs/todo/BACKLOG.md`
- Temporary only: `SEARCH_FIXTURE_DIR` under `/tmp`

- [ ] **Step 1: Create deterministic measurement fixture under a unique `/tmp` directory**

Run:

```bash
SEARCH_FIXTURE_DIR="$(mktemp -d /tmp/markdown-view-search-allocation-fixture.XXXXXX)"
mkdir -p "$SEARCH_FIXTURE_DIR/small" "$SEARCH_FIXTURE_DIR/medium" "$SEARCH_FIXTURE_DIR/large"
for i in $(seq 1 20); do printf '# Small %s\n\nneedle short paragraph %s.\n\nAnother sentence.\n' "$i" "$i" > "$SEARCH_FIXTURE_DIR/small/doc-$i.md"; done
for i in $(seq 1 200); do { printf '# Medium %s\n\n' "$i"; yes 'needle medium paragraph with repeated searchable text and surrounding context.' | head -80; } > "$SEARCH_FIXTURE_DIR/medium/doc-$i.md"; done
for i in $(seq 1 40); do { printf '# Large %s\n\n' "$i"; yes 'needle large paragraph with repeated searchable text and enough context to exercise block extraction and result context allocation.' | head -1200; } > "$SEARCH_FIXTURE_DIR/large/doc-$i.md"; done
printf 'SEARCH_FIXTURE_DIR=%s\n' "$SEARCH_FIXTURE_DIR"
find "$SEARCH_FIXTURE_DIR" -name '*.md' | wc -l
du -sh "$SEARCH_FIXTURE_DIR"
```

Expected:

```text
260
```

`du -sh` prints the fixture size. Keep the exact `SEARCH_FIXTURE_DIR` local-only for later commands. In docs, record only the fixture pattern, file count, and size; if the path must be mentioned, redact it as `/tmp/markdown-view-search-allocation-fixture.***`. Do not delete or overwrite a fixed `/tmp` path.

- [ ] **Step 2: Start the preview server on a local test port**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
test -d "$SEARCH_FIXTURE_DIR"
cargo run -- "$SEARCH_FIXTURE_DIR" --port 3019
```

Expected: server starts and listens on `127.0.0.1:3019`. Keep this process running until Task 3 Step 9 completes.

- [ ] **Step 3: Record server RSS before HTTP measurement**

In a second terminal/session, run:

```bash
export SEARCH_FIXTURE_DIR='<printed local path from Step 1>'
test -n "${SEARCH_FIXTURE_DIR:-}"
test -d "$SEARCH_FIXTURE_DIR"
ps -o pid,rss,comm,args -C markdown-view
```

Expected: identify the `markdown-view` process whose args contain `--port 3019` and the actual directory path printed as `SEARCH_FIXTURE_DIR` in Step 1. Record only its PID and RSS as the pre-search server RSS; do not paste full `ps` output or absolute local paths into docs. If the process cannot be identified unambiguously, stop.

- [ ] **Step 4: Smoke-test result-limit `/api/search` and capture response contract**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
test -d "$SEARCH_FIXTURE_DIR"
curl --fail-with-body -sS -w '\nhttp_code=%{http_code} size=%{size_download}\n' -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=needle'
```

Expected: `http_code=200`, JSON response includes `searched_files`, `searched_bytes`, `truncated`, `truncated_reasons`, and `results`, and this query reaches `truncated_reasons=["result_limit"]`. Record `searched_files`, `searched_bytes`, `truncated`, and `truncated_reasons`.

- [ ] **Step 5: Measure result-limit HTTP search response time three times**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
test -d "$SEARCH_FIXTURE_DIR"
/usr/bin/time -v curl --fail-with-body -sS -w 'http_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/result-limit-1.json" -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=needle'
/usr/bin/time -v curl --fail-with-body -sS -w 'http_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/result-limit-2.json" -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=needle'
/usr/bin/time -v curl --fail-with-body -sS -w 'http_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/result-limit-3.json" -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=needle'
```

Expected: all three commands exit successfully, each prints `http_code=200`, and each saved response under `SEARCH_FIXTURE_DIR` contains `searched_files`, `searched_bytes`, `truncated`, and `truncated_reasons`. Record elapsed time and response size for each run. Ignore `curl` maximum RSS for server memory judgment because it measures the client process. If any HTTP status is not 200 or any required JSON field is absent, treat the measurement as inconclusive and keep the backlog item open.

- [ ] **Step 6: Smoke-test full-scan `/api/search` and capture response contract**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
test -d "$SEARCH_FIXTURE_DIR"
curl --fail-with-body -sS -w '\nhttp_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/fullscan-smoke.json" -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=absentneedle'
rg -o '"searched_files":[0-9]+|"searched_bytes":[0-9]+|"truncated":(true|false)|"truncated_reasons":\[[^]]*\]' "$SEARCH_FIXTURE_DIR/fullscan-smoke.json"
```

Expected: `http_code=200`, `searched_files=260`, `truncated=false`, `truncated_reasons=[]`, and a nonzero `searched_bytes`. Record the response size and searched byte count.

- [ ] **Step 7: Measure full-scan HTTP search response time three times**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
test -d "$SEARCH_FIXTURE_DIR"
/usr/bin/time -v curl --fail-with-body -sS -w 'http_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/fullscan-1.json" -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=absentneedle'
ps -o pid,rss,comm,args -C markdown-view
/usr/bin/time -v curl --fail-with-body -sS -w 'http_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/fullscan-2.json" -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=absentneedle'
ps -o pid,rss,comm,args -C markdown-view
/usr/bin/time -v curl --fail-with-body -sS -w 'http_code=%{http_code} size=%{size_download}\n' -o "$SEARCH_FIXTURE_DIR/fullscan-3.json" -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=absentneedle'
ps -o pid,rss,comm,args -C markdown-view
```

Expected: all three timed commands exit successfully, each prints `http_code=200`, and each saved response under `SEARCH_FIXTURE_DIR` contains `searched_files=260`, `truncated=false`, and `truncated_reasons=[]`. Record elapsed time, response size, and the `--port 3019` server RSS after each run. Record only PID/RSS from `ps`; do not paste full `args` or absolute local paths into docs. If server RSS keeps increasing and no plateau is observed, record the measurement as incomplete and leave a follow-up instead of Done.

- [ ] **Step 8: Record final server RSS**

Run:

```bash
test -n "${SEARCH_FIXTURE_DIR:-}"
test -d "$SEARCH_FIXTURE_DIR"
ps -o pid,rss,comm,args -C markdown-view
```

Expected: identify the `--port 3019` process and record final RSS in KiB. Record only the PID/RSS needed for the measurement. If more than one `markdown-view` appears, do not touch pre-existing processes.

- [ ] **Step 9: Stop the server and confirm the fixture is not tracked**

Stop the `cargo run` process with Ctrl-C.

Run:

```bash
git status --short
```

Expected: no fixture or response files appear because they are under `SEARCH_FIXTURE_DIR` in `/tmp`. If only docs files appear later after Task 4, that is expected. Remove the `SEARCH_FIXTURE_DIR` only after confirming it is the directory created by `mktemp -d` in this task. Do not record the exact local directory path in committed docs.

### Task 4: Backlog Decision Update

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Open the current P2 backlog item**

Run:

```bash
sed -n '1,80p' docs/todo/BACKLOG.md
```

Expected: the P2 item `ディレクトリ検索の allocation 削減を追加計測に基づいて再判断する` is visible.

- [ ] **Step 2: Choose one decision based on measurements**

Use this decision rule:

```text
If focused tests and HTTP measurement cover the stated upper-limit scenarios and are practical:
  mark the item Done and state that optimization is not justified now.
If measurements are representative only and do not cover the upper-limit scenarios:
  keep the item unchecked with the exact missing measurements and reason.
If either measurement shows concrete user-facing or memory risk:
  keep or split the item into a specific follow-up, naming the suspected hotspot.
If measurement was inconclusive:
  keep the item with the exact missing measurement and reason.
```

Expected: exactly one branch is selected. Do not introduce source-code optimization in this task. Do not mark the item Done unless the measured fixture actually covers the upper-limit scope named in the conclusion.

- [ ] **Step 3: Edit `docs/todo/BACKLOG.md`**

For a representative-only result, keep the P2 item unchecked with this structure. Write concrete measured values from Tasks 1, 2, and 3 in the `計測済み` line before saving the file:

```markdown
- [ ] ディレクトリ検索の allocation 削減を追加計測に基づいて再判断する
  - 現状: ディレクトリ検索の隔離、検索上限、キャンセル境界、同時実行上限を短く記録する。
  - 計測済み: 環境、fixture 規模、検索系 test の elapsed 範囲、検索系 test の最大 RSS 範囲、HTTP result-limit 経路、HTTP full-scan 経路、server RSS 系列、`searched_files`、`searched_bytes`、`truncated`、`truncated_reasons` を実測値で記録する。fixture は `mktemp -d` の `/tmp` 配下に生成し、repo へ追加していないことを明記する。
  - 残件: 複数ファイルに分散した 100 件 result-limit、64 MiB 近傍 full-scan または byte-limit 近傍、10 MiB 単一ファイル、RSS plateau など、Done 判定に不足している計測を列挙する。
  - 判断: 検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は変更していない。現時点では完了扱いにせず、追加計測が必要な P2 残件として残す。
```

If all upper-limit scenarios are later measured and are practical, move the item to `## Done` only after the conclusion names the measured upper-limit fixture and preserves the security note. If the result shows a concrete hotspot, keep or split the item into the smallest actionable follow-up.

- [ ] **Step 4: Confirm no source changes**

Run:

```bash
git diff --name-only
```

Expected:

```text
docs/todo/BACKLOG.md
docs/superpowers/plans/2026-05-19-directory-search-allocation-measurement.md
docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md
```

If source files appear, remove only changes made during this task and do not revert unrelated user changes.

### Task 5: Verification And Commit

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Modify: `docs/superpowers/plans/2026-05-19-directory-search-allocation-measurement.md`
- Modify: `docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md`

- [ ] **Step 1: Check docs diff**

Run:

```bash
git diff -- docs/todo/BACKLOG.md docs/superpowers/plans/2026-05-19-directory-search-allocation-measurement.md docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md
```

Expected: diff only updates the allocation measurement backlog item, the measurement plan, and the design note. Existing security/context language is preserved or made more explicit.

- [ ] **Step 2: Run placeholder and required-field scans**

Run:

```bash
rg -n "T[B]D|TO[D]O|未[定]|rustc[ ]version|cargo[ ]version|OS[ ]summary|elapsed[ ]range|RSS[ ]range" docs/todo/BACKLOG.md docs/superpowers/plans/2026-05-19-directory-search-allocation-measurement.md docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md
placeholder_matches="$(rg -n "T[B]D|TO[D]O|未[定]|rustc[ ]version|cargo[ ]version|OS[ ]summary|elapsed[ ]range|RSS[ ]range" docs/todo/BACKLOG.md docs/superpowers/plans/2026-05-19-directory-search-allocation-measurement.md docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md | rg -v 'TODO\.md|T\[B\]D|TO\[D\]O|未\[定\]|rustc\[ \]version|cargo\[ \]version|OS\[ \]summary|elapsed\[ \]range|RSS\[ \]range' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
rg -n "q=absentneedle|--fail-with-body|mktemp|RSS|git log --oneline -3|明示" docs/superpowers/plans/2026-05-19-directory-search-allocation-measurement.md
rg -n "Host|CSP|path validation|HTML sanitize|SearchResponse|検索キャンセル|検索上限" docs/todo/BACKLOG.md docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md
```

Expected: the first command may print known literal references such as `TODO.md` and the scan command itself. The second and third commands fail if any unknown placeholder remains after filtering known non-placeholder matches. The fourth command confirms the plan includes the full-scan path, HTTP failure handling, unique fixture directory, RSS handling, and explicit approval/commit status references. The fifth command confirms security boundaries remain documented.

- [ ] **Step 3: Run docs-safe verification**

Run:

```bash
git diff --check
```

Expected: no whitespace errors.

For this docs-only measurement task, `./verify.sh` is not required unless source files changed. If any source file changed, stop and restore the task to docs-only scope before committing.

- [ ] **Step 4: Commit the backlog decision if approval is explicit**

Proceed only if the user has explicitly approved committing this docs update in the current workflow. Otherwise stop after verification and report the pending commit.

Run:

```bash
git add docs/todo/BACKLOG.md docs/superpowers/plans/2026-05-19-directory-search-allocation-measurement.md docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md
git commit -m "docs: 検索allocation計測判断を保留へ修正"
```

Expected: one docs commit is created. Commit only the three docs files above.

- [ ] **Step 5: Final status check**

Run:

```bash
git status --short
git log --oneline -3
```

Expected: worktree is clean. The three most recent commits include the design spec, the original measurement plan/result, and this correction commit.

## Self-Review

- Spec coverage: This plan covers docs-only scope, search-test measurement, HTTP measurement, no dependency addition, security boundaries, judgment criteria, backlog update, and verification.
- Placeholder scan: Before execution completes, Task 5 Step 2 checks that filled measurement placeholders are not left in `BACKLOG.md`.
- Type and command consistency: This plan does not introduce code types. Commands consistently use `SEARCH_FIXTURE_DIR` from `mktemp -d`, port `3019`, and the three docs files named in Task 5.
