# augmentHashWithTrailingLineHint ELEMENT_NODE sibling Test Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `augmentHashWithTrailingLineHint` の `ELEMENT_NODE` sibling 対応を直接テストで固定し、外部 review ID コメントを仕様説明へ置き換える。

**Architecture:** 変更は `tests/e2e/memo_jump.spec.ts` に閉じる。既存の `augmentHashWithTrailingLineHint` 直接テスト群へ `span` sibling の positive / negative test を1件ずつ追加し、既存コメントは外部IDではなく false-positive 防止仕様を説明する形へ置き換える。

**Tech Stack:** Playwright Test, TypeScript, browser-side DOM evaluation, existing `window.__MV_E2E__` test exposure.

---

## File Structure

- Modify: `tests/e2e/memo_jump.spec.ts`
  - Responsibility: `augmentHashWithTrailingLineHint` の既存ブラウザ関数を Playwright から直接呼び、memo preview 内リンクの hash 補完仕様を固定する。
- Reference: `src/template/assets/js/content.js`
  - Responsibility: 実装済みの `augmentHashWithTrailingLineHint`。今回の計画では変更しない。
- Reference: `docs/superpowers/specs/2026-04-28-augment-hash-element-sibling-test-design.md`
  - Responsibility: 承認済み設計。

## Scope Notes

この計画は1つの test-only タスクで完結する。既存実装は `ELEMENT_NODE` sibling をすでに扱うため、新規テストは red にならない可能性が高い。今回の目的は既存仕様の欠落カバレッジを埋めることなので、プロダクションコードを意図的に壊して red を作る手順は含めない。

### Task 1: `memo_jump.spec.ts` に ELEMENT_NODE sibling 回帰テストを追加する

**Files:**
- Modify: `tests/e2e/memo_jump.spec.ts`
- Test: `tests/e2e/memo_jump.spec.ts`

- [ ] **Step 1: 現在の対象箇所を確認する**

Run:

```bash
rg -n "augmentHashWithTrailingLineHint は `L10 onwards`|augmentHashWithTrailingLineHint は `L15-L17`|Codex review" tests/e2e/memo_jump.spec.ts
```

Expected:

```text
`L10 onwards` の散文テスト、`L15-L17` の positive test、Codex review ID を含むコメントが見つかる
```

- [ ] **Step 2: `L10 onwards` テストのコメントから外部IDを消す**

In `tests/e2e/memo_jump.spec.ts`, replace the comment block inside:

```ts
test('augmentHashWithTrailingLineHint は `L10 onwards` のような散文では augment しない', async ({ page }) => {
```

Use this exact comment:

```ts
  // ユーザー自作メモでリンク直後に行番号から始まる散文（`L10 onwards は詳しい` 等）が続く場合、
  // sibling textContent 全体が行番号トークンのみで占められないため augment しない。
  // 旧 regex（末尾アンカーなし）は先頭 `L10` だけを拾って `#intro:L10` に誤書換していたため、
  // sibling 全体が行番号トークンだけで構成されることを固定する。
```

- [ ] **Step 3: ELEMENT_NODE sibling の直接テストを追加する**

Insert these tests immediately after the `L10 onwards` test and before the `L15-L17` range test:

```ts
test('augmentHashWithTrailingLineHint は ELEMENT_NODE sibling の textContent から行番号を補完する', async ({ page }) => {
  // renderer が旧形式メモの行番号テキストを span 等でラップしても、link.nextSibling の
  // textContent から `L<n>` を読み、TEXT_NODE と同じ hash 補完を行うことを固定する。
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview')!;
    const link = document.createElement('a');
    link.href = '?file=long.md#section-b';
    link.textContent = 'dummy';
    const lineHint = document.createElement('span');
    lineHint.textContent = ' L15';
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return augmentHashWithTrailingLineHint(link, '#section-b');
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  expect(result).toBe('#section-b:L15');
});
```

```ts
test('augmentHashWithTrailingLineHint は ELEMENT_NODE sibling の散文を行番号扱いしない', async ({ page }) => {
  // ELEMENT_NODE 経路でも TEXT_NODE と同じく、sibling textContent 全体が行番号トークン
  // のみで構成されない散文は augment 対象にしないことを固定する。
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview')!;
    const link = document.createElement('a');
    link.href = '?file=spec.md#intro';
    link.textContent = 'spec';
    const lineHint = document.createElement('span');
    lineHint.textContent = ' L10 onwards は詳しい説明';
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return augmentHashWithTrailingLineHint(link, '#intro');
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  expect(result).toBe('#intro');
});
```

- [ ] **Step 4: 外部IDコメントが残っていないことを確認する**

Run:

```bash
rg -n "Codex review|#[0-9]{6,}" tests/e2e/memo_jump.spec.ts
```

Expected:

```text
no matches
```

- [ ] **Step 5: 対象 E2E を実行する**

Run:

```bash
npm run test:e2e -- memo_jump.spec.ts
```

Expected:

```text
memo_jump.spec.ts の全テストが pass する
```

- [ ] **Step 6: 通常検証を実行する**

Run:

```bash
./verify.sh
```

Expected:

```text
format, lint, tests が pass する
```

- [ ] **Step 7: 変更差分を確認する**

Run:

```bash
git diff -- tests/e2e/memo_jump.spec.ts
```

Expected:

```text
差分は `tests/e2e/memo_jump.spec.ts` のテスト2件追加とコメント置換だけ
```

- [ ] **Step 8: 実装変更をコミットする**

Run:

```bash
git add tests/e2e/memo_jump.spec.ts
git commit -m "test: ELEMENT_NODE siblingのhash補完を固定"
```

Expected:

```text
1 file changed
```

## Self-Review

- Spec coverage: 設計書の受け入れ基準は Task 1 の Step 2, Step 3, Step 4, Step 7 で満たす。レビュー対応後は Step 3 に ELEMENT_NODE negative test も含める。
- Placeholder scan: この計画に未確定の作業指示や空の実装手順はない。
- Type consistency: 追加コードは既存 spec と同じ `page.evaluate`、DOM API、`expect(result).toBe(...)` の形を使う。新しい helper や型宣言は追加しない。
- Security: URL hash 補完経路の test-only 変更であり、`#memo-preview` スコープガード、既存 hash 優先、散文除外の既存テストを維持する。
