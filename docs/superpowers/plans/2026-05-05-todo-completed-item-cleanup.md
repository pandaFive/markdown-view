# TODO Completed Item Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `docs/todo/TODO.md` から完了済みの `AppState` 項目を取り除き、Done Summary に完了根拠を残す。

**Architecture:** docs-only の整理として `TODO.md` だけを編集する。完了判定は既存設計書と直近コミットで確認し、未確認の Medium 項目や `BACKLOG.md` は動かさない。

**Tech Stack:** Markdown documentation, `rg`, `git diff`

---

## File Structure

- Modify: `docs/todo/TODO.md`
  - Medium Priority から完了済みの `AppState` 項目を削除する。
  - Done Summary に完了根拠つきの短い項目を追加する。
- Reference: `docs/superpowers/specs/2026-05-05-todo-completed-item-cleanup-design.md`
  - 実装範囲、非目的、検証条件の根拠として読む。
- Reference: `docs/superpowers/specs/2026-05-05-appstate-arc-lifecycle-design.md`
  - 完了根拠の記述内容を確認する。
- No changes: `docs/todo/BACKLOG.md`, `src/**`, `tests/**`, config files

## Task 1: 完了根拠を確認する

**Files:**
- Read: `docs/todo/TODO.md`
- Read: `docs/superpowers/specs/2026-05-05-appstate-arc-lifecycle-design.md`
- Read: `src/server/state.rs`

- [ ] **Step 1: 現在の TODO 項目を確認する**

Run:

```bash
rg -n "AppState|with_memo_fs|Arc<AppState>" docs/todo/TODO.md
```

Expected:

```text
Medium Priority 内に AppState 項目が 1 件見つかる。
```

- [ ] **Step 2: 設計書上の完了条件を確認する**

Run:

```bash
rg -n "AppState|with_memo_fs|Arc<AppState>|Acceptance Criteria" docs/superpowers/specs/2026-05-05-appstate-arc-lifecycle-design.md
```

Expected:

```text
AppState の共有を Arc<AppState> に統一すること、with_memo_fs を削除すること、MemoFs を生成時注入することが確認できる。
```

- [ ] **Step 3: 現行コードが完了条件に合っていることを確認する**

Run:

```bash
rg -n "derive\\(Clone\\)|with_memo_fs|new_with_tokio_memo_fs|Arc<AppState>|memo_fs" src/server/state.rs src/main.rs
```

Expected:

```text
with_memo_fs は見つからない。
new_with_tokio_memo_fs と MemoFs 注入 constructor が見つかる。
AppState 自体の derive(Clone) は見つからない。
main.rs 側で Arc::new(AppState::new_with_tokio_memo_fs(...)) 相当の生成が確認できる。
```

## Task 2: TODO.md を最小編集する

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Medium Priority から AppState 項目を削除する**

Edit `docs/todo/TODO.md` and remove this entire bullet block from Medium Priority:

```markdown
- [ ] `AppState` の Arc 二重ラップと `with_memo_fs` の API 整合を解消する
  - ファイル: `src/server/state.rs` L202-258, `src/main.rs` L105
  - 現状: `AppState: Clone` でフィールド単位に `Arc` を持つにも関わらず、`main.rs:105` が `Arc::new(AppState::new(...))` で外側でも Arc 化している。`with_memo_fs` は `cfg(test)` で `mut self` を取るが、`Arc<AppState>` をテスト本体で扱う API（`api_files_handler` 等）と相互運用できないハーフサポート状態
  - 対応: `AppState: !Clone` にして `Arc<AppState>` 一本に統一。`MemoFs` を `AppState::new` の引数にする builder パターンに変更し、`with_memo_fs` を削除。テストは `Arc::clone` に書き換え
  - 理由: 「Clone とフィールド Arc の二重投資」を解消し、ライフサイクルを単一化する
```

- [ ] **Step 2: Done Summary に完了根拠を追加する**

Add this bullet near the top of `## Done Summary`, before older Done items:

```markdown
- [x] `AppState` の Arc 二重ラップと `with_memo_fs` の API 整合を解消する
  - 完了根拠: `AppState` の共有単位を外側の `Arc<AppState>` に統一し、`with_memo_fs` を削除した。`MemoFs` は生成時注入の constructor へ寄せ、production 経路は Tokio 実装を使う構成になっている
```

- [ ] **Step 3: 差分を確認する**

Run:

```bash
git diff -- docs/todo/TODO.md
```

Expected:

```text
docs/todo/TODO.md だけが変更されている。
Medium Priority から AppState 項目が削除されている。
Done Summary に AppState 項目の完了根拠が追加されている。
他の TODO 項目や BACKLOG 項目は変更されていない。
```

## Task 3: 文書検証を実行する

**Files:**
- Verify: `docs/todo/TODO.md`
- Verify: `docs/superpowers/specs/2026-05-05-todo-completed-item-cleanup-design.md`

- [ ] **Step 1: AppState 項目の配置を確認する**

Run:

```bash
rg -n "AppState|with_memo_fs|Arc<AppState>" docs/todo/TODO.md docs/superpowers/specs/2026-05-05-todo-completed-item-cleanup-design.md
```

Expected:

```text
docs/todo/TODO.md では Done Summary 側の完了済み項目だけが見つかる。
docs/superpowers/specs/2026-05-05-todo-completed-item-cleanup-design.md では設計上の対象説明が見つかる。
```

- [ ] **Step 2: Medium Priority に AppState 項目が残っていないことを確認する**

Run:

```bash
sed -n '/## Medium Priority/,/## Done Summary/p' docs/todo/TODO.md
```

Expected:

```text
Medium Priority の範囲に `AppState`、`with_memo_fs`、`Arc<AppState>` の項目が存在しない。
```

- [ ] **Step 3: 変更ファイルが範囲内であることを確認する**

Run:

```bash
git status --short
```

Expected:

```text
 M docs/todo/TODO.md
```

If the implementation plan file is still uncommitted in the worktree, the status may also include:

```text
?? docs/superpowers/plans/2026-05-05-todo-completed-item-cleanup.md
```

No `src/**`, `tests/**`, `docs/todo/BACKLOG.md`, or config file changes should appear.

## Task 4: 最終確認とコミット

**Files:**
- Modify: `docs/todo/TODO.md`
- Commit: `docs/todo/TODO.md`

- [ ] **Step 1: docs-only 最終 diff を確認する**

Run:

```bash
git diff -- docs/todo/TODO.md
```

Expected:

```text
変更内容は Task 2 の削除 1 件と Done Summary 追加 1 件だけである。
```

- [ ] **Step 2: TODO.md 変更をコミットする**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: TODOの完了済みAppState項目を整理"
```

Expected:

```text
1 file changed
```

- [ ] **Step 3: 残作業を確認する**

Run:

```bash
git status --short --branch
```

Expected:

```text
## docs/todo-completed-item-cleanup
```

If the plan file has not been committed separately, commit it before Task 4 or include it in a separate docs commit. Do not mix the plan commit and TODO.md implementation commit unless the user explicitly asks for a single commit.

## Residual Risk

- `TODO.md` の他 Medium 項目の行番号や現状説明は再監査しないため、別の stale 記述が残る可能性はある。
- `BACKLOG.md` は対象外のため、完了済み混入があっても今回の計画では検出しない。
- 実装コードは変更しないため、セキュリティ境界そのもののリスク増減はない。
