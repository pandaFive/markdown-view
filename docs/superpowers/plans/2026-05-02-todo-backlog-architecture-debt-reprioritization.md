# TODO/BACKLOG Architecture Debt Reprioritization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reclassify `docs/todo/TODO.md` and `docs/todo/BACKLOG.md` around architecture-debt impact while preserving verified history and security-sensitive priorities.

**Architecture:** This is a docs-only change. Keep `TODO.md` as the High/Medium working queue and `BACKLOG.md` as the Low Priority queue, moving only verified completed items to Done and moving unfinished items between priority buckets with explicit rationale.

**Tech Stack:** Markdown documentation, `rg`, `git`, repository verification via `./verify.sh`.

---

## File Structure

- Modify: `docs/todo/TODO.md`
  - Responsibility: High and Medium priority active work items.
  - Keep: top-level High/Medium structure and existing item detail style.
  - Change: remove verified completed items, move lower-priority unfinished items to `BACKLOG.md`, and reorder remaining items by architecture-debt impact.

- Modify: `docs/todo/BACKLOG.md`
  - Responsibility: Low priority P1/P2/P3 parking lot and Done history.
  - Keep: P1/P2/P3 structure and Done section.
  - Change: receive downgraded items with preserved context, record completed items with evidence, and avoid duplicate unfinished entries.

- Reference only: `docs/superpowers/specs/2026-05-02-todo-backlog-architecture-debt-reprioritization-design.md`
  - Responsibility: approved design and acceptance criteria.

- Reference only: `docs/superpowers/specs/`, `docs/superpowers/plans/`, `src/`, `tests/`
  - Responsibility: evidence for completion and current code shape. Do not edit these files during this plan.

## Task 1: Inventory Current Items And Evidence

**Files:**
- Read: `docs/todo/TODO.md`
- Read: `docs/todo/BACKLOG.md`
- Read: `docs/superpowers/specs/2026-05-02-todo-backlog-architecture-debt-reprioritization-design.md`
- Modify: none

- [ ] **Step 1: Confirm the branch and working tree**

Run:

```bash
git status --short --branch
```

Expected: branch is `docs/todo-backlog-architecture-debt-reprioritization` and there are no unstaged changes before editing.

- [ ] **Step 2: List active TODO items with headings**

Run:

```bash
rg -n "^- \\[ \\]|^## " docs/todo/TODO.md
```

Expected: output shows `High Priority`, `Medium Priority`, and every active item in `TODO.md`.

- [ ] **Step 3: List active BACKLOG items with headings**

Run:

```bash
rg -n "^- \\[ \\]|^## |^### " docs/todo/BACKLOG.md
```

Expected: output shows `P1`, `P2`, `P3`, `Done`, and every active item in `BACKLOG.md`.

- [ ] **Step 4: Find likely completed items by recent history**

Run:

```bash
git log --oneline -20
```

Expected: recent commits include enough context to identify completed items, especially `notify_update` receiver-zero observability and recent docs cleanup work.

- [ ] **Step 5: Check existing design and plan names for overlap**

Run:

```bash
rg -n "notify_update|receiver=0|Host 検証|Markdown 方言|watcher|AppState|RenderState|browser JS|検索|canonicalize" docs/superpowers/specs docs/superpowers/plans
```

Expected: output identifies existing specs/plans that can be used as evidence, but no files are modified.

## Task 2: Move Verified Completed Items To Done

**Files:**
- Modify: `docs/todo/TODO.md`
- Modify: `docs/todo/BACKLOG.md`
- Reference: `git log --oneline -20`
- Reference: `src/server/broadcast.rs`
- Reference: `src/server/files/content.rs`

- [ ] **Step 1: Verify `notify_update` receiver-zero work is complete**

Run:

```bash
rg -n "受信者ゼロ|receiver_count|warnログ|log_change_error_without_receivers|inspect_change_error_without_receivers" src/server/broadcast.rs src/server/files/content.rs tests
```

Expected: output shows tests or implementation proving receiver-zero file-change errors are now logged or inspected instead of silently dropped.

- [ ] **Step 2: Remove the completed `notify_update` High item from `TODO.md`**

Edit `docs/todo/TODO.md`:

- Remove the unchecked High Priority item whose title is:
  `notify_update の receiver=0 早期 return で Error メッセージが silent drop されない経路にする`
- Do not remove unrelated High Priority items.
- Keep the High Priority section valid Markdown after removal.

- [ ] **Step 3: Add a Done entry for the completed `notify_update` item**

Edit `docs/todo/BACKLOG.md` under `## Done` and add this completed entry near the top of Done:

```markdown
- [x] `notify_update` receiver=0 エラー観測性改善
  - ファイル: `src/server/broadcast.rs`, `src/server/files/content.rs`
  - 内容: WebSocket 受信者が 0 の場合でも、ファイル変更イベント由来の検証・読込前エラーを warn ログへ残す経路を追加した。正常更新では従来通り本文読込と描画を避ける
  - 完了根拠: `6de7137 fix: notify_updateの受信者なしエラーをログ化 (#114)`、現行の `server::broadcast::tests::*受信者ゼロ*` 系テスト
  - 由来: TODO.md High Priority
```

- [ ] **Step 4: Check duplicate completed entries**

Run:

```bash
rg -n "notify_update|receiver=0|受信者ゼロ|受信者なし" docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected: `TODO.md` no longer contains the removed active item. `BACKLOG.md` contains one Done entry for the completed work.

- [ ] **Step 5: Commit the completion cleanup**

Run:

```bash
git diff -- docs/todo/TODO.md docs/todo/BACKLOG.md
git add docs/todo/TODO.md docs/todo/BACKLOG.md
git commit -m "docs: 完了済みTODOをDoneへ移動"
```

Expected: commit succeeds and only `docs/todo/TODO.md` and `docs/todo/BACKLOG.md` are included.

## Task 3: Reclassify Active Items By Architecture Debt

**Files:**
- Modify: `docs/todo/TODO.md`
- Modify: `docs/todo/BACKLOG.md`
- Reference: `docs/superpowers/specs/2026-05-02-todo-backlog-architecture-debt-reprioritization-design.md`

- [ ] **Step 1: Promote architecture-boundary items to the top of `TODO.md`**

Edit `docs/todo/TODO.md` so High Priority starts with active items that define cross-module boundaries or security boundaries. Use this order unless current evidence proves an item is already complete:

1. `見出し ID 生成を単一パス化し render と toc で HeadingInfo を共有する`
2. `Markdown 方言オプションを共通化し、表示・TOC・検索の差分を明示する`
3. `監視イベント経由の変更ファイルを最終読込前に再検証する`
4. `Host 検証を router middleware 化して新規 route の守り忘れを防ぐ`
5. `ディレクトリ検索の負荷制御をサーバ側に追加する`

Expected: High Priority emphasizes shared parser/profile boundaries, watcher read security, route security middleware, and server-side search load control.

- [ ] **Step 2: Keep structural but less blocking work in Medium Priority**

Edit `docs/todo/TODO.md` so Medium Priority contains active items like:

- `async ハンドラ内の同期 I/O を spawn_blocking ないし起動時固定化で解消する`
- `watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する`
- `shutdown チェーンの観測性を統合する`
- `toc.rs の build_toc_html で level=0 インデックス OOB ガードを入れる`
- `AppState の Arc 二重ラップと with_memo_fs の API 整合を解消する`
- `BroadcastMessage::Refresh の memo_refresh 契約と、メモ読込失敗時の degrade 通知を明示する`
- `watcher の WatchEvent::Error 後の健全性 API と atomic save 耐性 E2E を追加する`
- `RenderState を enum BlockContext スタックに置き換えて open/close 対応を型化する`
- `template/mod.rs のテストをサブモジュールへ分割し、render_page の 62 行 format! を関数分割する`
- `canonicalize 失敗時の再帰挙動の非対称を解消する`
- `ブラウザ JS の責務境界を小モジュールへ分割する`

Expected: these items remain active, but below High because they do not define as many future cross-route or cross-parser constraints.

- [ ] **Step 3: Move local or lower-risk active items from `TODO.md` to `BACKLOG.md` if found**

If `TODO.md` still contains an item that is local, low-risk, and not a broad architectural dependency, move it to `BACKLOG.md` P1 or P2 with its original details preserved.

Use this mapping:

- Safety or observability support item: move to `BACKLOG.md` P1.
- Local maintainability or documentation item: move to `BACKLOG.md` P2.
- Micro-optimization or long-term cleanup: move to `BACKLOG.md` P3.

Expected: no item loses file references, current-state notes, action notes, or rationale while moving.

- [ ] **Step 4: Ensure BACKLOG P1/P2/P3 still follow the same priority meaning**

Edit `docs/todo/BACKLOG.md` only if needed:

- P1: risk reduction and verification foundation.
- P2: maintainability and local regression detection.
- P3: long-term improvement and low urgency.

Expected: moved items land under the bucket matching these meanings.

- [ ] **Step 5: Check for duplicated active item titles**

Run:

```bash
rg -n "^- \\[ \\] " docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected: each active item title appears in only one file and one priority bucket.

- [ ] **Step 6: Commit the priority reclassification**

Run:

```bash
git diff -- docs/todo/TODO.md docs/todo/BACKLOG.md
git add docs/todo/TODO.md docs/todo/BACKLOG.md
git commit -m "docs: TODOをアーキテクチャ負債軸で再分類"
```

Expected: commit succeeds and only `docs/todo/TODO.md` and `docs/todo/BACKLOG.md` are included.

## Task 4: Validate Documentation Consistency

**Files:**
- Read: `docs/todo/TODO.md`
- Read: `docs/todo/BACKLOG.md`
- Read: `docs/superpowers/specs/2026-05-02-todo-backlog-architecture-debt-reprioritization-design.md`
- Modify: none unless validation reveals a docs inconsistency

- [ ] **Step 1: Run targeted consistency checks**

Run:

```bash
rg -n "notify_update|receiver=0|受信者ゼロ|完了根拠" docs/todo/TODO.md docs/todo/BACKLOG.md
rg -n "Host|Origin|sanitize|CSP|path|silent|atomic|watcher|Markdown 方言|HeadingInfo" docs/todo/TODO.md docs/todo/BACKLOG.md
rg -n "TBD|未定|あとで|仮置き|PLACEHOLDER" docs/todo/TODO.md docs/todo/BACKLOG.md docs/superpowers/plans/2026-05-02-todo-backlog-architecture-debt-reprioritization.md
```

Expected: completed `notify_update` work appears only as Done evidence; security-sensitive topics remain visible in active priority buckets; no placeholder language appears.

- [ ] **Step 2: Confirm docs-only scope**

Run:

```bash
git diff --stat HEAD~2..HEAD
git diff --name-only HEAD~2..HEAD
```

Expected: changed files are limited to `docs/todo/TODO.md`, `docs/todo/BACKLOG.md`, and planning/spec docs.

- [ ] **Step 3: Run repository verification**

Run:

```bash
./verify.sh
```

Expected: format, clippy, Rust tests, release tests, and E2E typecheck pass.

- [ ] **Step 4: Commit any validation-only correction if needed**

If Step 1 or Step 2 reveals only a documentation inconsistency, edit the affected docs file and run:

```bash
git add docs/todo/TODO.md docs/todo/BACKLOG.md
git commit -m "docs: TODO整理の整合性を補正"
```

Expected: this step is skipped when no correction is needed.

- [ ] **Step 5: Prepare completion report**

Report:

- Changed files with reason and rough line impact.
- Affected dependent files: `docs/superpowers/specs/2026-05-02-todo-backlog-architecture-debt-reprioritization-design.md`, `docs/superpowers/plans/2026-05-02-todo-backlog-architecture-debt-reprioritization.md`.
- Verification result from `./verify.sh`.
- Residual risk: priority classification is based on current code and docs evidence; ambiguous external-review claims remain active unless verified complete.

## Self-Review

- Spec coverage: Tasks cover inventory, completed-item cleanup, High/Medium/BACKLOG reclassification, security-sensitive priority preservation, duplicate checks, docs-only scope, verification, and rollback-friendly commits.
- Placeholder scan: The plan contains no unresolved placeholder markers. Mentions of `TODO.md` are file names, not unfinished plan content.
- Scope check: This plan is docs-only and does not modify production code, tests, or configuration.
- Ambiguity check: Completion moves require evidence; ambiguous items stay active. Security-sensitive items must not be downgraded without a stated rationale.
