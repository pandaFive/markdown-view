# BACKLOG Done Monthly Archive Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `docs/todo/BACKLOG.md` の長い Done 履歴を `docs/done/DONE-2026-05.md` へ移し、BACKLOG を未完了候補中心に軽量化する。

**Architecture:** docs-only の移動作業として扱う。`BACKLOG.md` は未完了 P1 / P2 / P3 とアーカイブ参照だけを持ち、`DONE-2026-05.md` が BACKLOG 由来の 2026年5月完了履歴を保持する。

**Tech Stack:** Markdown, ripgrep, git diff, repository docs conventions.

---

## Files

- Modify: `docs/todo/BACKLOG.md`
  - 責務: 未完了の低優先・長期改善候補を P1 / P2 / P3 で保持する。
  - 今回の変更: 冒頭説明から「完了済みの履歴を置く」を外し、`## Done` を短いアーカイブ参照へ置き換える。
- Create: `docs/done/DONE-2026-05.md`
  - 責務: 2026年5月に `docs/todo/BACKLOG.md` から移動した完了済みタスクを保存する。
  - 今回の変更: 移動前の `BACKLOG.md` の `## Done` 配下を原文ほぼそのまま保持する。
- Reference only: `docs/superpowers/specs/2026-05-16-backlog-done-monthly-archive-design.md`
  - 責務: 承認済み設計判断。実装中に変更しない。

## Task 1: 移動前の構造を確認する

**Files:**
- Read: `docs/todo/BACKLOG.md`
- Read: `docs/superpowers/specs/2026-05-16-backlog-done-monthly-archive-design.md`

- [ ] **Step 1: 現在のブランチと作業ツリーを確認する**

Run:

```bash
git status --short --branch
```

Expected: branch は `docs/backlog-done-monthly-archive`。未コミット差分がある場合は、今回の plan 追加だけか確認してから進める。

- [ ] **Step 2: `BACKLOG.md` の見出しとチェック項目を確認する**

Run:

```bash
rg -n "^## |^- \\[ \\]|^- \\[x\\]" docs/todo/BACKLOG.md
```

Expected:

```text
docs/todo/BACKLOG.md:9:## P1: リスク低減・契約明文化
docs/todo/BACKLOG.md:11:## P2: 保守性・局所回帰検知
docs/todo/BACKLOG.md:13:- [ ] ディレクトリ検索のキャンセル境界と allocation 削減を検討する
docs/todo/BACKLOG.md:20:- [ ] `AppMode` 構築時の `is_file()`/`is_dir()` 判定の TOCTOU を緩和する
docs/todo/BACKLOG.md:27:- [ ] `log_path::canonicalize_status` の毎回 syscall を削減する
docs/todo/BACKLOG.md:34:## P3: 長期改善・低緊急
docs/todo/BACKLOG.md:36:- [ ] WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する
docs/todo/BACKLOG.md:43:- [ ] サイドバーの "Documents" 文字列を i18n または日本語化
docs/todo/BACKLOG.md:50:- [ ] インラインブラウザJS の TS 化
docs/todo/BACKLOG.md:57:- [ ] `catalog.rs` のパス構築での Vec アロケーション削減
docs/todo/BACKLOG.md:64:## Done
```

Line numbers may shift if the plan file has already been committed or docs changed, but the unchecked items and `## Done` section must exist before the move.

- [ ] **Step 3: 移動対象の完了項目数を記録する**

Run:

```bash
rg -n "^- \\[x\\]" docs/todo/BACKLOG.md
```

Expected: `BACKLOG.md` の `## Done` 配下に複数の完了項目が表示される。実装後は同じタイトル群が `docs/done/DONE-2026-05.md` に移っていることを確認する。

## Task 2: 月別 Done アーカイブを作成する

**Files:**
- Create: `docs/done/DONE-2026-05.md`
- Read: `docs/todo/BACKLOG.md`

- [ ] **Step 1: 新規アーカイブファイルを作成する**

Create `docs/done/DONE-2026-05.md` with this exact header:

```markdown
# 完了タスク (2026-05)

このファイルには 2026-05 に `docs/todo/BACKLOG.md` から移動した完了済みタスクを記録します。

---

## BACKLOG 完了履歴
```

- [ ] **Step 2: `BACKLOG.md` の Done 項目を原文のまま移す**

Read `docs/todo/BACKLOG.md` from the first completed item immediately after `## Done` through the end of file. Append that block after `## BACKLOG 完了履歴` in `docs/done/DONE-2026-05.md`.

The first moved item must start with:

```markdown
- [x] `data-memo-file` 属性を None 時にスキップする
```

The moved block must preserve item bodies, including `ファイル:`, `内容:`, `完了根拠:`, `由来:`, and multi-line bullet details. Do not summarize long entries.

- [ ] **Step 3: アーカイブ内の先頭と末尾を確認する**

Run:

```bash
sed -n '1,40p' docs/done/DONE-2026-05.md
```

Expected: header, `## BACKLOG 完了履歴`, and the first completed item `data-memo-file` are visible.

Run:

```bash
tail -40 docs/done/DONE-2026-05.md
```

Expected: the file ends with the last moved completed item from the original `BACKLOG.md`, without unrelated P2 / P3 unchecked items appended after it.

## Task 3: BACKLOG を未完了中心に軽量化する

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Read: `docs/done/DONE-2026-05.md`

- [ ] **Step 1: 冒頭説明を更新する**

In `docs/todo/BACKLOG.md`, replace:

```markdown
低優先度で蓄積している項目。High/Medium は [`TODO.md`](./TODO.md) に置き、ここには低優先・長期改善・完了済みの履歴を置く。
```

with:

```markdown
低優先度で蓄積している項目。High/Medium は [`TODO.md`](./TODO.md) に置き、ここには低優先・長期改善の未完了候補を置く。
```

- [ ] **Step 2: 最終整理文にアーカイブ導線を追加する**

In `docs/todo/BACKLOG.md`, replace:

```markdown
最終整理: 2026-05-09。セキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目は `TODO.md` へ昇格した。ここには昇格しないが文脈を残すべき候補を置く。
```

with:

```markdown
最終整理: 2026-05-09。セキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目は `TODO.md` へ昇格した。ここには昇格しないが文脈を残すべき候補を置く。
完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) に移動した。
```

- [ ] **Step 3: `## Done` セクションを短い参照へ置き換える**

In `docs/todo/BACKLOG.md`, replace the entire `## Done` section with:

```markdown
## Done

完了済み履歴は [`DONE-2026-05.md`](../done/DONE-2026-05.md) へ移動した。
```

Do not remove or reorder the P1 / P2 / P3 sections above `## Done`.

- [ ] **Step 4: `BACKLOG.md` の未完了項目が残っていることを確認する**

Run:

```bash
sed -n '1,90p' docs/todo/BACKLOG.md
```

Expected: P1 / P2 / P3 sections remain. The unchecked items are unchanged. `## Done` contains only the archive reference.

## Task 4: 文書整合性を検証する

**Files:**
- Verify: `docs/todo/BACKLOG.md`
- Verify: `docs/done/DONE-2026-05.md`
- Verify: `docs/superpowers/specs/2026-05-16-backlog-done-monthly-archive-design.md`

- [ ] **Step 1: 見出しとチェック項目の配置を確認する**

Run:

```bash
rg -n "^## |^- \\[ \\]|^- \\[x\\]|DONE-2026-05" docs/todo/BACKLOG.md docs/done/DONE-2026-05.md
```

Expected:

- `docs/todo/BACKLOG.md` contains P1 / P2 / P3 unchecked items and a short `## Done`.
- `docs/done/DONE-2026-05.md` contains `## BACKLOG 完了履歴` and the moved `- [x]` items.
- `DONE-2026-05` appears in the BACKLOG archive reference.

- [ ] **Step 2: 先送り表現が増えていないことを確認する**

Run:

```bash
rg -n "T[B]D|未[定]|あと[で]" docs/todo/BACKLOG.md docs/done/DONE-2026-05.md docs/superpowers/specs/2026-05-16-backlog-done-monthly-archive-design.md
```

Expected: no matches.

- [ ] **Step 3: 未完了項目が `BACKLOG.md` に残り、完了項目がアーカイブへ移ったことを確認する**

Run:

```bash
rg -n "^- \\[ \\]" docs/todo/BACKLOG.md
```

Expected: seven unchecked items remain in `BACKLOG.md`.

Run:

```bash
rg -n "^- \\[x\\]" docs/todo/BACKLOG.md docs/done/DONE-2026-05.md
```

Expected: `docs/todo/BACKLOG.md` has no `- [x]` items. `docs/done/DONE-2026-05.md` has the moved completed items.

- [ ] **Step 4: 差分が設計どおり docs-only であることを確認する**

Run:

```bash
git diff -- docs/todo/BACKLOG.md docs/done/DONE-2026-05.md docs/superpowers/specs/2026-05-16-backlog-done-monthly-archive-design.md
```

Expected: only `BACKLOG.md` and `DONE-2026-05.md` changed during implementation. The spec file should not change.

## Task 5: コミットする

**Files:**
- Commit: `docs/todo/BACKLOG.md`
- Commit: `docs/done/DONE-2026-05.md`

- [ ] **Step 1: ステージ対象を確認する**

Run:

```bash
git status --short --branch
```

Expected: modified `docs/todo/BACKLOG.md` and new `docs/done/DONE-2026-05.md` are visible. No code files are changed.

- [ ] **Step 2: docs 変更だけをステージする**

Run:

```bash
git add docs/todo/BACKLOG.md docs/done/DONE-2026-05.md
```

Expected: command succeeds with no output.

- [ ] **Step 3: ステージ差分を確認する**

Run:

```bash
git diff --cached --stat
```

Expected: only `docs/todo/BACKLOG.md` and `docs/done/DONE-2026-05.md` appear.

- [ ] **Step 4: コミットする**

Run:

```bash
git commit -m "docs: BACKLOG完了履歴を月別アーカイブへ移動"
```

Expected: commit succeeds.

## Self-Review

- Spec coverage: 目的、非目的、月別アーカイブ、BACKLOG 軽量化、セキュリティ、受け入れ条件、検証、ロールバックは Tasks 1-5 で対応している。
- Placeholder scan: plan contains no placeholder or deferral requirements.
- Type consistency: docs-only 変更のため型や関数名はない。ファイルパスは spec と一致している。
