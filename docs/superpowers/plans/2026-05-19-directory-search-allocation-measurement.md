# Directory Search Allocation Measurement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ディレクトリ検索の allocation 削減が必要かを、依存追加なしの計測結果に基づいて判断し、`docs/todo/BACKLOG.md` に結論を残す。

**Architecture:** コードは変更せず、検索系 targeted tests と HTTP `/api/search` 経由の 2 層で実行時間・RSS・検索レスポンス統計を観測する。fixture は `/tmp` に生成し、永続変更は計測結果と判断を記録する docs 更新だけに限定する。

**Tech Stack:** Rust, Cargo, existing integration tests, `cargo run`, `curl`, `/usr/bin/time` or shell `time`, `ps`, Markdown docs.

---

## File Structure

- Modify: `docs/todo/BACKLOG.md`
  - 計測結果と判断を P2 項目に追記し、Done 化するか、保留理由付きで残すか、後続最適化候補へ分割する。
- Reference: `docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md`
  - 目的、非目標、判断基準、セキュリティ境界を確認する。
- No persistent fixture files
  - 計測用 Markdown は `/tmp/markdown-view-search-allocation-fixture` に生成し、リポジトリへ追加しない。
- No source changes
  - `src/`, `tests/`, `Cargo.toml`, `Cargo.lock` は変更しない。

## Scope Check

この plan は単一サブシステム、ディレクトリ検索 allocation 計測だけを扱う。検索ロジック変更、依存追加、bench harness 追加、UI 変更は含めない。

### Task 1: Baseline And Tool Inventory

**Files:**
- Reference: `docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md`
- Modify later: `docs/todo/BACKLOG.md`

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
command -v hyperfine
```

Expected:

```text
/usr/bin/time
/usr/bin/curl
/usr/bin/ps
```

`hyperfine` may be absent. If absent, use repeated `/usr/bin/time -v` and shell loops.

- [ ] **Step 3: Record environment summary for the backlog note**

Run:

```bash
rustc --version
cargo --version
uname -a
```

Expected: each command prints one line. Copy the Rust version, Cargo version, OS/kernel summary, and date into the measurement note added in Task 4.

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
- Temporary only: `/tmp/markdown-view-search-allocation-fixture`

- [ ] **Step 1: Create deterministic measurement fixture under `/tmp`**

Run:

```bash
rm -rf /tmp/markdown-view-search-allocation-fixture
mkdir -p /tmp/markdown-view-search-allocation-fixture/small /tmp/markdown-view-search-allocation-fixture/medium /tmp/markdown-view-search-allocation-fixture/large
for i in $(seq 1 20); do printf '# Small %s\n\nneedle short paragraph %s.\n\nAnother sentence.\n' "$i" "$i" > "/tmp/markdown-view-search-allocation-fixture/small/doc-$i.md"; done
for i in $(seq 1 200); do { printf '# Medium %s\n\n' "$i"; yes 'needle medium paragraph with repeated searchable text and surrounding context.' | head -80; } > "/tmp/markdown-view-search-allocation-fixture/medium/doc-$i.md"; done
for i in $(seq 1 40); do { printf '# Large %s\n\n' "$i"; yes 'needle large paragraph with repeated searchable text and enough context to exercise block extraction and result context allocation.' | head -1200; } > "/tmp/markdown-view-search-allocation-fixture/large/doc-$i.md"; done
find /tmp/markdown-view-search-allocation-fixture -name '*.md' | wc -l
du -sh /tmp/markdown-view-search-allocation-fixture
```

Expected:

```text
260
```

`du -sh` prints the fixture size. Record it.

- [ ] **Step 2: Start the preview server on a local test port**

Run:

```bash
cargo run -- /tmp/markdown-view-search-allocation-fixture --port 3019
```

Expected: server starts and listens on `127.0.0.1:3019`. Keep this process running until Task 3 Step 6 completes.

- [ ] **Step 3: Smoke-test `/api/search` and capture response contract**

In a second terminal/session, run:

```bash
curl -sS -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=needle'
```

Expected: JSON response includes `searched_files`, `searched_bytes`, `truncated`, `truncated_reasons`, and `results`. Record `searched_files`, `searched_bytes`, `truncated`, and `truncated_reasons`.

- [ ] **Step 4: Measure HTTP search response time three times**

Run:

```bash
/usr/bin/time -v curl -sS -o /tmp/markdown-view-search-allocation-response-1.json -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=needle'
/usr/bin/time -v curl -sS -o /tmp/markdown-view-search-allocation-response-2.json -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=needle'
/usr/bin/time -v curl -sS -o /tmp/markdown-view-search-allocation-response-3.json -H 'Host: 127.0.0.1:3019' 'http://127.0.0.1:3019/api/search?q=needle'
```

Expected: all three `curl` commands exit successfully. Record elapsed time for each run. Ignore `curl` maximum RSS for server memory judgment because it measures the client process.

- [ ] **Step 5: Record server RSS**

Run:

```bash
ps -o pid,rss,comm,args -C markdown-view
```

Expected: one `markdown-view` process appears. Record RSS in KiB. If more than one appears, identify the process with `--port 3019`.

- [ ] **Step 6: Stop the server and confirm the fixture is not tracked**

Stop the `cargo run` process with Ctrl-C.

Run:

```bash
git status --short
```

Expected: no fixture files appear because they are under `/tmp`. If only `docs/todo/BACKLOG.md` appears later after Task 4, that is expected.

### Task 4: Backlog Decision Update

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Open the current P2 backlog item**

Run:

```bash
sed -n '1,80p' docs/todo/BACKLOG.md
```

Expected: the P2 item `ディレクトリ検索の allocation 削減を計測結果に基づいて検討する` is visible.

- [ ] **Step 2: Choose one decision based on measurements**

Use this decision rule:

```text
If focused tests and HTTP measurement are both practical under current limits:
  mark the item Done and state that optimization is not justified now.
If either measurement shows concrete user-facing or memory risk:
  keep or split the item into a specific follow-up, naming the suspected hotspot.
If measurement was inconclusive:
  keep the item with the exact missing measurement and reason.
```

Expected: exactly one of the three branches is selected. Do not introduce source-code optimization in this task.

- [ ] **Step 3: Edit `docs/todo/BACKLOG.md`**

For the likely no-optimization-needed result, move the P2 item to `## Done` with this structure. Write concrete measured values from Tasks 1, 2, and 3 in the `計測メモ` line before saving the file:

```markdown
- [x] ディレクトリ検索の allocation 削減を計測結果に基づいて検討する
  - 完了根拠: 依存追加なしで検索系 targeted tests と HTTP `/api/search` 経由の計測を行った。検索系 test の elapsed / 最大 RSS、`/api/search?q=needle` の elapsed、server RSS、`searched_files`、`searched_bytes`、`truncated` を確認し、現行の 100 件結果上限、1000 ファイル上限、64 MiB 総読込上限、10 MiB 単一ファイル上限の範囲では、`Cow<str>` 化や検索ブロック処理単位変更を直ちに入れる根拠はないと判断した。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は変更していない
  - 計測メモ: 環境、fixture 規模、検索系 test の elapsed 範囲、検索系 test の最大 RSS 範囲、HTTP 検索 elapsed 範囲、server RSS、`searched_files`、`searched_bytes`、`truncated` を実測値で記録する。fixture は `/tmp` に 260 Markdown files を生成し、repo へ追加していないことを明記する。
```

If the result requires a follow-up instead, keep the item unchecked and replace `対応` / `判断` with the measured hotspot and the exact follow-up scope. Include security note that no validation boundary is weakened.

- [ ] **Step 4: Confirm no source changes**

Run:

```bash
git diff --name-only
```

Expected:

```text
docs/todo/BACKLOG.md
```

If source files appear, remove only changes made during this task and do not revert unrelated user changes.

### Task 5: Verification And Commit

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Check docs diff**

Run:

```bash
git diff -- docs/todo/BACKLOG.md
```

Expected: diff only updates the allocation measurement backlog item and preserves existing security/context language.

- [ ] **Step 2: Run placeholder and required-field scans**

Run:

```bash
rg -n "T[B]D|TO[D]O|未[定]|rustc[ ]version|cargo[ ]version|OS[ ]summary|elapsed[ ]range|RSS[ ]range" docs/todo/BACKLOG.md
rg -n "allocation|計測|SearchResponse|Host|CSP|path validation|HTML sanitize|検索キャンセル|検索上限" docs/todo/BACKLOG.md
```

Expected: the first command prints no matches. The second command prints the updated backlog context.

- [ ] **Step 3: Run docs-safe verification**

Run:

```bash
git diff --check
```

Expected: no whitespace errors.

For this docs-only measurement task, `./verify.sh` is not required unless source files changed. If any source file changed, stop and restore the task to docs-only scope before committing.

- [ ] **Step 4: Commit the backlog decision**

Run:

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: ディレクトリ検索allocation計測結果を反映"
```

Expected: one docs commit is created. Commit only `docs/todo/BACKLOG.md`.

- [ ] **Step 5: Final status check**

Run:

```bash
git status --short
git log --oneline -2
```

Expected: worktree is clean. The two most recent commits are the design spec commit and the backlog measurement-result commit.

## Self-Review

- Spec coverage: This plan covers docs-only scope, search-test measurement, HTTP measurement, no dependency addition, security boundaries, judgment criteria, backlog update, and verification.
- Placeholder scan: Before execution completes, Task 5 Step 2 checks that filled measurement placeholders are not left in `BACKLOG.md`.
- Type and command consistency: This plan does not introduce code types. Commands consistently use `/tmp/markdown-view-search-allocation-fixture`, port `3019`, and `docs/todo/BACKLOG.md`.
