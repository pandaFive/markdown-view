# Search RSS Plateau Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the remaining `docs/todo/TODO.md` RSS plateau item from Medium Priority to Done Summary with a documented completion judgment.

**Architecture:** This is a docs-only completion task. It does not change Rust code, measurement scripts, UI, TypeScript, or generated JavaScript. The implementation updates `TODO.md` so the existing 2026-06-04 and 2026-06-05 measurements become the completion basis rather than an open Medium item.

**Tech Stack:** Markdown documentation, ripgrep validation, git whitespace validation.

---

## File Structure

- Modify: `docs/todo/TODO.md`
  - Remove the open Medium Priority checkbox for the RSS plateau item.
  - Add a short note that there are currently no High / Medium open execution candidates.
  - Add a Done Summary entry with completion basis, security boundary notes, and residual candidates recorded in `BACKLOG.md` without reopening High / Medium.
- Modify: `docs/todo/BACKLOG.md`
  - Add a P2 item for optional future native Linux, allocator, or prefix live allocation diagnosis.
- Read-only reference: `docs/superpowers/specs/2026-06-06-search-rss-plateau-completion-design.md`
  - Use this as the acceptance source.
- Read-only reference: `docs/superpowers/specs/2026-06-04-search-rss-plateau-design.md`
  - Use this for the original measurement scope.
- Read-only reference: `docs/superpowers/specs/2026-06-05-search-rss-allocator-profile-design.md`
  - Use this for allocator profile interpretation.
- Read-only reference: `scripts/measure-search-rss-plateau.mjs`
  - Confirm no script change is needed.

## Task 1: Update TODO Completion State

**Files:**
- Modify: `docs/todo/TODO.md`
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Re-read the current Medium item**

Run:

```bash
sed -n '1,80p' docs/todo/TODO.md
```

Expected: output includes one open Medium checkbox:

```text
- [ ] ディレクトリ検索 many-match の RSS plateau を切り分ける
```

- [ ] **Step 2: Move the open RSS plateau item to Done Summary**

Confirm before editing that the user has approved the implementation scope: move the RSS plateau item out of High / Medium, keep the completion judgment, and record optional future diagnosis in `BACKLOG.md` P2. If approval is missing, stop and ask for it.

Edit `docs/todo/TODO.md` with `apply_patch`.

Replace this Medium section:

```markdown
## Medium Priority

すぐ重大事故ではないが、後続改修の前提、設計負債、検証基盤として効く項目。

- [ ] ディレクトリ検索 many-match の RSS plateau を切り分ける
  - 計測: 2026-06-04 に `scripts/measure-search-rss-plateau.mjs` を追加し、`/tmp/markdown-view-search-rss-plateau.***` fixture で dev/release、cold/warm、prefix/multifile を測定した。fallback は full fixture が既存ファイルサイズ上限で skip され、short/release の参考測定に留まった。sandbox 内では loopback bind が `Operation not permitted` で失敗したため、server 起動を伴う HTTP 測定は承認付きで実行した。script は `target/debug|release/markdown-view` を直接起動して実サーバ PID を測り、`--no-open` でブラウザ起動を止め、出力には実パス、full process args、本文断片、raw maps 行を含めていない。
  - 結果: full prefix fixture は 1 file / 4,388,889 bytes、HTTP response は dev/release とも `searched_files=1`, `searched_bytes=4388889`, `truncated=true`, `truncated_reasons=["result_limit"]`, `resultsLength=100` を維持した。release prefix は cold/warm とも elapsed 約 51ms だが、5秒後 settled RSS は 705,580→781,404 KiB、`RssAnon` は 699,060→774,516 KiB、`smaps_rollup Anonymous` は 699,616→774,964 KiB まで残った。dev prefix も elapsed 約 52ms、settled RSS 517,716→522,960 KiB、`RssAnon` 504,076→508,872 KiB だった。一方、multifile fixture は 120 files / 593,880 bytes で result-limit に到達しても release settled RSS 13,280→13,200 KiB、dev settled RSS 18,480→22,964 KiB に留まった。full fallback fixture は 23,100,013 bytes で既存ファイルサイズ上限を超え、`searched_files=0`, `searched_bytes=0` の skip 経路になった。参考として short fallback 132,013 bytes は `result_limit` まで検索でき、release settled RSS 15,084→16,880 KiB に留まった。
  - 追加確認: レビュー指摘修正で full fallback fixture を 10MiB 未満へ調整した後、release/cold 単発では 1 file / 9,900,013 bytes、`searched_files=1`, `searched_bytes=9900013`, `truncated=true`, `truncated_reasons=["result_limit"]`, `resultsLength=100` となり、skip ではなく安全境界なし巨大 block fallback 経路を測れた。elapsed は約 307ms、5秒後 settled RSS は 319,472 KiB、`RssAnon` は 313,128 KiB、`smaps_rollup Anonymous` は 313,788 KiB だった。
  - 判断: plateau は server 起動直後や multifile result-limit の基礎コストではなく、単一ファイル prefix many-match 経路で response 完了後に残る anonymous memory が支配的と判断する。direct binary PID でも再現し、`RssAnon` / `smaps_rollup Anonymous` が支配的で、file-backed RSS は小さいため、Tokio worker 全体や process 初期化より glibc allocator arena / retained anonymous memory、または WSL2 の RSS/accounting 特性が主因候補である。`maps.anonymousKb` は初期状態でも約 1.6GiB の virtual anonymous map を含むため、RSS 判断では `RssAnon` と `smaps_rollup Anonymous` を優先する。
  - 残件: 下記の追加切り分けで `MALLOC_ARENA_MAX` 比較は実施済み。残る allocator / WSL2 切り分けには同一 prefix fixture を native Linux または allocator 別 build で検証し、prefix 経路の live allocation も切り分ける。安全境界なし巨大 block fallback は 10MiB 未満の fixture で実検索できることを確認したため、今後は prefix とは別の性能経路として改善設計を検討する。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は弱めていない。
  - 追加切り分け: 2026-06-05 に `scripts/measure-search-rss-plateau.mjs` へ allocator profile 比較を追加し、同一 prefix fixture を `default` / `arena1` / `arena2` で測定した。release/full prefix は各 profile と cold/warm で HTTP response の `searched_files=1`、`searched_bytes=4388889`、`truncated=true`、`truncated_reasons=["result_limit"]`、`resultsLength=100` を維持した。測定値は `default` cold が elapsed 51.987395ms、peak RSS 662,972 KiB、settled `VmRSS` 662,972 KiB、`RssAnon` 656,200 KiB、`smaps_rollup Anonymous` 656,892 KiB、warm が elapsed 50.880588ms、peak RSS 696,952 KiB、settled `VmRSS` 696,952 KiB、`RssAnon` 690,372 KiB、`smaps_rollup Anonymous` 690,804 KiB。`arena1` cold は elapsed 51.457633ms、peak RSS 553,060 KiB、settled `VmRSS` 553,060 KiB、`RssAnon` 546,248 KiB、`smaps_rollup Anonymous` 546,588 KiB、warm は elapsed 50.436205ms、peak RSS 545,784 KiB、settled `VmRSS` 545,784 KiB、`RssAnon` 538,872 KiB、`smaps_rollup Anonymous` 539,252 KiB。`arena2` cold は elapsed 51.061918ms、peak RSS 570,856 KiB、settled `VmRSS` 570,856 KiB、`RssAnon` 564,004 KiB、`smaps_rollup Anonymous` 564,360 KiB、warm は elapsed 50.006147ms、peak RSS 609,412 KiB、settled `VmRSS` 609,412 KiB、`RssAnon` 602,644 KiB、`smaps_rollup Anonymous` 603,092 KiB。すべて `partialMeasurementReasons=[]` で、測定不能 profile はなかった。smoke 測定は sandbox 内で loopback port が `EPERM` となり失敗したため、承認付きで再実行して成功した。smoke report は `allocatorProfile.name="default"`、`allocatorProfile.env={}` の 1 report だった。レビュー修正で `default` は測定対象 server process の親環境を最小 allowlist に限定する scrubbed baseline として固定し、allocator / `LD_PRELOAD` 系の scrub key 名、profile env で親環境を上書きした key 名、env policy の `allocatorEnvScrubTargetKeys` を report へ出すようにした。self-test では全 allocator scrub target、allowlist 全 key、secret/proxy/CI 系の非継承、profile env の上書き、report/context shape を固定した。最終検証は `node scripts/measure-search-rss-plateau.mjs --self-test-sanitization`、`node scripts/measure-search-rss-plateau.mjs --help`、`git diff --check`、`./verify.sh` が成功し、`node scripts/measure-search-rss-plateau.mjs --smoke` は sandbox 内 `EPERM` 後に承認付き再実行で成功した。測定出力には実パス、full process args、本文断片、raw maps 行、親環境の値を含めていない。
  - 判断更新: `MALLOC_ARENA_MAX=1` で settled anonymous RSS が `default` 比で cold は `RssAnon` 109,952 KiB / `smaps_rollup Anonymous` 110,304 KiB、warm は `RssAnon` 151,500 KiB / `smaps_rollup Anonymous` 151,552 KiB 下がったため、glibc allocator arena retained memory を主因候補として扱う。ただし `arena1` でも settled `RssAnon` は 538,872-546,248 KiB 残るため、WSL2 RSS/accounting 特性または prefix 経路の live allocation も残候補として継続する。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は弱めていない。

## Done Summary
```

with:

```markdown
## Medium Priority

すぐ重大事故ではないが、後続改修の前提、設計負債、検証基盤として効く項目。
現時点で未完了の High / Medium 実行候補はない。新しいレビュー指摘や計測結果が出た場合は、重要度と将来影響度を再評価して追加する。

## Done Summary

- [x] ディレクトリ検索 many-match の RSS plateau を切り分ける
  - 完了根拠: 2026-06-04 と 2026-06-05 の測定で、plateau は server 起動直後や multifile result-limit の基礎コストではなく、単一ファイル prefix many-match 経路で response 完了後に残る anonymous memory が支配的と判断できた。full prefix fixture は 1 file / 4,388,889 bytes、HTTP response は dev/release とも `searched_files=1`, `searched_bytes=4388889`, `truncated=true`, `truncated_reasons=["result_limit"]`, `resultsLength=100` を維持した。release prefix は cold/warm とも elapsed 約 51ms だが、5秒後 settled RSS は 705,580→781,404 KiB、`RssAnon` は 699,060→774,516 KiB、`smaps_rollup Anonymous` は 699,616→774,964 KiB まで残った。一方、multifile fixture は release settled RSS 13,280→13,200 KiB、dev settled RSS 18,480→22,964 KiB に留まり、short fallback も release settled RSS 15,084→16,880 KiB に留まった。
  - 追加切り分け: full fallback fixture を 10MiB 未満へ調整した後、release/cold 単発では 1 file / 9,900,013 bytes、`searched_files=1`, `searched_bytes=9900013`, `truncated=true`, `truncated_reasons=["result_limit"]`, `resultsLength=100` となり、skip ではなく安全境界なし巨大 block fallback 経路を測れた。elapsed は約 307ms、5秒後 settled RSS は 319,472 KiB、`RssAnon` は 313,128 KiB、`smaps_rollup Anonymous` は 313,788 KiB だった。これにより fallback は prefix とは別の性能経路として扱える。
  - allocator 判断: `scripts/measure-search-rss-plateau.mjs` の allocator profile 比較では、release/full prefix の `default` / `arena1` / `arena2` 各 profile と cold/warm で HTTP response の `searched_files=1`、`searched_bytes=4388889`、`truncated=true`、`truncated_reasons=["result_limit"]`、`resultsLength=100` を維持した。`MALLOC_ARENA_MAX=1` で settled anonymous RSS が `default` 比で cold は `RssAnon` 109,952 KiB / `smaps_rollup Anonymous` 110,304 KiB、warm は `RssAnon` 151,500 KiB / `smaps_rollup Anonymous` 151,552 KiB 下がったため、glibc allocator arena retained memory を主因候補として扱う。ただし `arena1` でも settled `RssAnon` は 538,872-546,248 KiB 残るため、WSL2 RSS/accounting 特性または prefix 経路の live allocation は残候補として記録する。
  - 完了判断: Medium Priority の「切り分ける」目的は、支配候補、非支配経路、残候補、検索契約維持を分類できたため達成済みとする。native Linux、別 allocator、prefix 経路の live allocation 追加検証は、RSS の絶対値改善や環境差検証を行う場合の新規テーマであり、この完了判定の必須残件にはしない。必要時に再開できるよう、低優先の将来候補として `BACKLOG.md` P2 へ移した。
  - 検証とセキュリティ: 既存検証では `node scripts/measure-search-rss-plateau.mjs --self-test-sanitization`、`node scripts/measure-search-rss-plateau.mjs --help`、`git diff --check`、`./verify.sh` が成功し、`node scripts/measure-search-rss-plateau.mjs --smoke` は sandbox 内 `EPERM` 後に承認付き再実行で成功した。測定出力には実パス、full process args、本文断片、raw maps 行、親環境の値を含めていない。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は弱めていない。
```

- [ ] **Step 3: Confirm the open checkbox moved**

Run:

```bash
rg -n "^- \\[ \\]" docs/todo/TODO.md
```

Expected: no output and exit code 1.

- [ ] **Step 4: Confirm the Done Summary entry exists**

Run:

```bash
rg -n "ディレクトリ検索 many-match の RSS plateau を切り分ける|glibc allocator arena|WSL2 RSS/accounting|Host/Origin|SearchResponse|CSP" docs/todo/TODO.md
```

Expected: output includes the Done Summary checkbox and the security boundary sentence.

- [ ] **Step 5: Add the residual diagnosis candidate to BACKLOG**

Edit `docs/todo/BACKLOG.md` with `apply_patch`. Under `## P2: 保守性・局所回帰検知`, add a low-priority item for optional future native Linux, allocator, or prefix live allocation diagnosis. The item must state that it does not reopen High / Medium and must preserve the same output sanitization and security boundary constraints.

## Task 2: Validate Documentation Consistency

**Files:**
- Validate: `docs/todo/TODO.md`
- Validate: `docs/todo/BACKLOG.md`
- Validate: `docs/superpowers/specs/2026-06-06-search-rss-plateau-completion-design.md`
- Validate: `docs/superpowers/plans/2026-06-06-search-rss-plateau-completion.md`

- [ ] **Step 1: Check for accidental placeholders**

Run:

```bash
placeholder_matches="$(rg -n -P 'T[B]D|TO[D]O[:：]|未[定]' docs/todo/TODO.md docs/todo/BACKLOG.md docs/superpowers/specs/2026-06-06-search-rss-plateau-completion-design.md docs/superpowers/plans/2026-06-06-search-rss-plateau-completion.md | rg -v 'T\\[B\\]D|TO\\[D\\]O|未\\[定\\]' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
```

Expected: command exits 0 with no output.

- [ ] **Step 2: Check Markdown diff whitespace**

Run:

```bash
git diff --check
```

Expected: no output and exit code 0.

- [ ] **Step 3: Review the focused diff**

Run:

```bash
git diff -- docs/todo/TODO.md
```

Expected: diff removes the open Medium item, keeps the measurement facts, and adds a Done Summary completion entry. It must not add real temp paths, Markdown body snippets, raw `/proc/maps` rows, full process args, or parent environment variable values.

- [ ] **Step 4: Confirm the residual diagnosis is visible in BACKLOG**

Run:

```bash
rg -n "prefix many-match RSS plateau|native Linux|別 allocator|live allocation" docs/todo/BACKLOG.md
```

Expected: output includes the new P2 backlog item and its diagnosis scope.

- [ ] **Step 5: Confirm obsolete Medium follow-up wording is gone**

Run:

```bash
rg -n "Medium Priority の follow-up として継続する|RSS plateau follow-up の受け入れ条件として残す|受け入れ条件として残す" docs/todo/TODO.md
```

Expected: no output and exit code 1.

- [ ] **Step 6: Record verification scope**

Do not run `./verify.sh` for this docs-only update unless the execution owner chooses stricter final verification. If it is not run, the completion report must say:

```text
./verify.sh は production code 非変更の docs-only 更新のため未実行。文書 validation と git diff --check は実行済み。
```

## Task 3: Commit Documentation Update

**Files:**
- Commit: `docs/todo/TODO.md`
- Commit: `docs/todo/BACKLOG.md`
- Commit: `docs/superpowers/specs/2026-06-06-search-rss-plateau-completion-design.md`
- Commit: `docs/superpowers/plans/2026-06-06-search-rss-plateau-completion.md`

- [ ] **Step 1: Confirm branch is not develop or main**

Run:

```bash
git branch --show-current
```

Expected:

```text
docs/search-rss-plateau-completion-design
```

- [ ] **Step 2: Confirm only documentation files are modified**

Run:

```bash
git status --short
```

Expected output contains:

```text
 M docs/todo/TODO.md
 M docs/todo/BACKLOG.md
?? docs/superpowers/specs/2026-06-06-search-rss-plateau-completion-design.md
?? docs/superpowers/plans/2026-06-06-search-rss-plateau-completion.md
```

No production source files should be modified.

- [ ] **Step 3: Commit the documentation update**

Confirm before committing that the user has approved creating a commit for the reviewed docs-only changes. If approval is missing, stop and ask for it.

Run:

```bash
git add docs/todo/TODO.md docs/todo/BACKLOG.md docs/superpowers/specs/2026-06-06-search-rss-plateau-completion-design.md docs/superpowers/plans/2026-06-06-search-rss-plateau-completion.md
git commit -m "docs: 検索RSS plateau完了整理のレビュー指摘を反映"
```

Expected: commit succeeds with two modified and two added documentation files.

- [ ] **Step 4: Confirm clean worktree**

Run:

```bash
git status --short
```

Expected: no output.

## Self-Review Checklist

- [ ] Every spec acceptance criterion maps to a task:
  - `TODO.md` open RSS item moves to Done Summary: Task 1.
  - Measurement values, main candidate, residual candidates, security boundaries are preserved: Task 1.
  - Native Linux / allocator / prefix live allocation follow-up is visible in BACKLOG P2 without reopening High / Medium: Task 1 and Task 2.
  - Validation succeeds: Task 2.
  - Commit is isolated: Task 3.
- [ ] No placeholder text remains in this plan.
- [ ] Commands use exact paths and expected outputs.
- [ ] The plan avoids production code changes.
