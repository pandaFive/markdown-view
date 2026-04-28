# BACKLOG.md Risk Prioritization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `docs/todo/BACKLOG.md` の未完了 Low 項目を P1/P2/P3 に分類し、完了済み項目を `Done` に分離する。

**Architecture:** docs-only の整理。既存項目本文は保持し、ヘッダと見出しを差し替え、項目ブロック単位で並べ替える。実装コード、テストコード、`docs/todo/TODO.md` は変更しない。

**Tech Stack:** Markdown, Bash (`grep`, `rg`, `awk`), git

**Design reference:** `docs/superpowers/specs/2026-04-28-backlog-risk-prioritization-design.md`

---

## ファイル構成

- Modify: `docs/todo/BACKLOG.md`
  - Low Priority の単一セクションを `P1 / P2 / P3 / Done` に分割する。
  - 既存の各項目本文、チェック状態、由来行は保持する。
- Read only: `docs/superpowers/specs/2026-04-28-backlog-risk-prioritization-design.md`
  - 分類ルールと受け入れ基準の確認に使う。
- Unchanged: `docs/todo/TODO.md`
  - High / Medium 管理ファイルなので、この計画では触らない。

---

### Task 1: `BACKLOG.md` を P1/P2/P3/Done 構造へ並べ替える

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: 現在の件数を確認する**

Run:

```bash
grep -c "^- \[ \]" docs/todo/BACKLOG.md
grep -c "^- \[x\]" docs/todo/BACKLOG.md
grep -c "^  - 由来:" docs/todo/BACKLOG.md
```

Expected output:

```text
17
1
18
```

- [ ] **Step 2: ヘッダと見出し構造を置き換える**

`docs/todo/BACKLOG.md` の冒頭を次の構造にする。

```markdown
# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium が TODO.md から捌けてから着手する候補。
未完了項目はリスク低減効果を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）の発見コンテキスト。

## P1: リスク低減・検証基盤
```

既存の `## Low Priority` 見出しは削除する。

- [ ] **Step 3: P1 に入れる未完了項目を移動する**

`## P1: リスク低減・検証基盤` の下に、次の順で項目ブロックを置く。各ブロックの本文は既存内容をそのまま使う。

1. E2E テストの DOM クリーンアップ戦略見直し
2. E2E の `declare global` ブロックを `tests/e2e/globals.d.ts` に集約
3. `updateContent` 型宣言の統一
4. `tsconfig.json` に strict flag 追加
5. `TestWebSocket` を `tests/e2e/browser/test-websocket.ts` に抽出
6. E2E を `verify.sh` に統合するか検討

P1 の最後に次の見出しを追加する。

```markdown
## P2: 保守性・局所回帰検知
```

- [ ] **Step 4: P2 に入れる未完了項目を移動する**

`## P2: 保守性・局所回帰検知` の下に、次の順で項目ブロックを置く。各ブロックの本文は既存内容をそのまま使う。

1. `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling のユニットテスト
2. 猶予期間中の連続 TOC クリックでの挙動検証
3. `TOC_NAVIGATION_SLACK_PX` 境界の回帰テスト
4. L347 を active 遷移フラッシュ厳密検証に強化
5. E2E 共通ヘルパーを `tests/e2e/helpers.ts` に抽出
6. `as unknown as` double-cast の説明コメント追加
7. `memo_jump.spec.ts:303` の Codex review ID 削除

P2 の最後に次の見出しを追加する。

```markdown
## P3: 長期改善・低緊急
```

- [ ] **Step 5: P3 に入れる未完了項目を移動する**

`## P3: 長期改善・低緊急` の下に、次の順で項目ブロックを置く。各ブロックの本文は既存内容をそのまま使う。

1. インラインブラウザJS の TS 化
2. `render_markdown` の責務分割（大規模）
3. `catalog.rs` のパス構築での Vec アロケーション削減
4. README のアーキテクチャ図を実装構成に揃える

P3 の最後に次の見出しを追加する。

```markdown
## Done
```

- [ ] **Step 6: Done に完了済み項目を移動する**

`## Done` の下に、次の完了済み項目ブロックを置く。本文は既存内容をそのまま使う。

1. `window.updateContent` を E2E モード限定 expose に変更

- [ ] **Step 7: 並べ替え後の差分を確認する**

Run:

```bash
git diff -- docs/todo/BACKLOG.md
```

Expected:

- `docs/todo/BACKLOG.md` だけが変更されている。
- 変更内容はヘッダ変更、`P1 / P2 / P3 / Done` 見出し追加、項目ブロックの移動に限られている。
- 既存項目の `ファイル`、`内容`、`理由`、`由来` が消えていない。

---

### Task 2: 構造検証してコミットする

**Files:**
- Verify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: 全体件数を検証する**

Run:

```bash
grep -c "^- \[ \]" docs/todo/BACKLOG.md
grep -c "^- \[x\]" docs/todo/BACKLOG.md
grep -c "^  - 由来:" docs/todo/BACKLOG.md
```

Expected output:

```text
17
1
18
```

- [ ] **Step 2: 各セクションの未完了件数を検証する**

Run:

```bash
awk '
  /^## P1:/ { section="P1"; next }
  /^## P2:/ { section="P2"; next }
  /^## P3:/ { section="P3"; next }
  /^## Done/ { section="Done"; next }
  /^- \[ \]/ { counts[section]++ }
  /^- \[x\]/ { done[section]++ }
  END {
    printf "P1=%d\nP2=%d\nP3=%d\nDoneChecked=%d\n", counts["P1"], counts["P2"], counts["P3"], done["Done"]
  }
' docs/todo/BACKLOG.md
```

Expected output:

```text
P1=6
P2=7
P3=4
DoneChecked=1
```

- [ ] **Step 3: 完了済み項目が Done にだけ存在することを検証する**

Run:

```bash
rg -n "window.updateContent.*E2E モード限定 expose|^## Done" docs/todo/BACKLOG.md
```

Expected:

- `## Done` の後に `window.updateContent` の項目が 1 件だけ表示される。
- `P1 / P2 / P3` には同じ項目が表示されない。

- [ ] **Step 4: 変更対象が `BACKLOG.md` だけであることを確認する**

Run:

```bash
git status --short
```

Expected output:

```text
 M docs/todo/BACKLOG.md
```

- [ ] **Step 5: セキュリティ観点を確認する**

目視で次を確認する。

- P1 の項目を「実装済みのセキュリティ改善」と表現していない。
- 外部レビュー由来の項目を現在の脆弱性として断定していない。
- `BACKLOG.md` の整理がセキュリティ境界の強化そのものではないことが、設計書に残っている。

- [ ] **Step 6: コミットする**

Run:

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: BACKLOGをリスク順に整理"
```

Expected:

```text
[docs/backlog-risk-prioritization <commit>] docs: BACKLOGをリスク順に整理
```

---

### Task 3: 最終確認を行う

**Files:**
- Verify: `docs/todo/BACKLOG.md`
- Verify: git history

- [ ] **Step 1: ブランチ上のコミットを確認する**

Run:

```bash
git log --oneline develop..HEAD
```

Expected:

```text
<new commit> docs: BACKLOGをリスク順に整理
f0d7fbf docs: BACKLOGリスク順整理の実装計画を追加
e943f63 docs: BACKLOGリスク順整理の設計を追加
```

- [ ] **Step 2: 作業ツリーが clean であることを確認する**

Run:

```bash
git status --short
```

Expected: output is empty.

- [ ] **Step 3: 完了報告に含める内容をまとめる**

報告には次を含める。

- 変更ファイル: `docs/todo/BACKLOG.md`
- 依存影響: `docs/todo/TODO.md` は未変更、今後のバックログ選定運用に影響
- 検証結果: 件数検証、セクション別件数、`Done` 分離、作業ツリー確認
- 残余リスク: 優先分類は実装完了を意味しない。各項目の着手時には現在のコードとテストで再検証する。
- 次の候補: P1 上位から個別に設計する。
