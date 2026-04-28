# E2E DOM Cleanup Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `tests/e2e/memo_jump.spec.ts` の `augmentHashWithTrailingLineHint` 系テストで、末尾ノード推測 cleanup をやめ、追加した DOM ノードだけを明示的に削除する。

**Architecture:** プロダクトコードは変更しない。各 `page.evaluate` 内で作成した `Text` ノード参照を保持し、`finally` で `link` とその `Text` ノードを直接 `remove()` する。共通 helper 抽出は同ファイル外へ広げず、P2 backlog の `tests/e2e/helpers.ts` 抽出とは分離する。

**Tech Stack:** TypeScript, Playwright, DOM API, npm scripts

**Design reference:** `docs/superpowers/specs/2026-04-28-backlog-next-task-selection-design.md`

---

## ファイル構成

- Modify: `tests/e2e/memo_jump.spec.ts`
  - `augmentHashWithTrailingLineHint` 系 7 テストの cleanup を `container.lastChild` 推測から `lineHint.remove()` へ置き換える。
  - 既存テスト名、期待値、`augmentHashWithTrailingLineHint` 呼び出し引数は変えない。
- Read only: `docs/superpowers/specs/2026-04-28-backlog-next-task-selection-design.md`
  - スコープ、非ゴール、検証方針の確認に使う。
- Unchanged: `verify.sh`, `playwright.config.ts`, `tests/e2e/helpers.ts`, `src/**`
  - 今回は E2E 実行統合、Playwright 設定変更、共通 helper 抽出、プロダクトコード変更をしない。

---

### Task 1: 既存の危険 cleanup パターンを検出する

**Files:**
- Inspect: `tests/e2e/memo_jump.spec.ts:258-417`

- [ ] **Step 1: 危険パターンの現在件数を確認する**

Run:

```bash
rg -n "container\\.lastChild|lastChild\\.remove\\(\\)" tests/e2e/memo_jump.spec.ts
```

Expected: `augmentHashWithTrailingLineHint` 系テスト内に `container.lastChild` と `lastChild.remove()` が 7 組表示される。

- [ ] **Step 2: 変更前の対象 E2E が通ることを確認する**

Run:

```bash
npm run test:e2e -- memo_jump.spec.ts
```

Expected: `memo_jump.spec.ts` の Playwright tests が PASS する。環境に Playwright browser が未導入の場合は、失敗理由が browser install 不足か実テスト失敗かを記録してから止める。

---

### Task 2: `memo_jump.spec.ts` の cleanup を明示ノード削除へ置き換える

**Files:**
- Modify: `tests/e2e/memo_jump.spec.ts:258-417`

- [ ] **Step 1: `content` コンテナのスコープガードテストを置き換える**

Replace the `page.evaluate` body in `augmentHashWithTrailingLineHint は memo-preview 外のリンクでは hash を変えない` with:

```ts
const result = await page.evaluate(() => {
  const container = document.getElementById('content')!;
  const link = document.createElement('a');
  const lineHint = document.createTextNode(' L10 onwards');
  link.href = 'other.md';
  link.textContent = 'other';
  container.appendChild(link);
  container.appendChild(lineHint);
  try {
    return augmentHashWithTrailingLineHint(link, '');
  } finally {
    link.remove();
    lineHint.remove();
  }
});
```

- [ ] **Step 2: `L5abc trailing` テストを置き換える**

Replace the `page.evaluate` body in `augmentHashWithTrailingLineHint は \`L5abc\` など英数字が続く場合は augment しない` with:

```ts
const result = await page.evaluate(() => {
  const container = document.getElementById('memo-preview')!;
  const link = document.createElement('a');
  const lineHint = document.createTextNode(' L5abc trailing');
  link.href = '?file=long.md#section-b';
  link.textContent = 'dummy';
  container.appendChild(link);
  container.appendChild(lineHint);
  try {
    return augmentHashWithTrailingLineHint(link, '#section-b');
  } finally {
    link.remove();
    lineHint.remove();
  }
});
```

- [ ] **Step 3: `L10 onwards` 散文テストを置き換える**

Replace the `page.evaluate` body in `augmentHashWithTrailingLineHint は \`L10 onwards\` のような散文では augment しない` with:

```ts
const result = await page.evaluate(() => {
  const container = document.getElementById('memo-preview')!;
  const link = document.createElement('a');
  const lineHint = document.createTextNode(' L10 onwards は詳しい説明');
  link.href = '?file=spec.md#intro';
  link.textContent = 'spec';
  container.appendChild(link);
  container.appendChild(lineHint);
  try {
    return augmentHashWithTrailingLineHint(link, '#intro');
  } finally {
    link.remove();
    lineHint.remove();
  }
});
```

- [ ] **Step 4: `L15-L17` 範囲形式テストを置き換える**

Replace the `page.evaluate` body in `augmentHashWithTrailingLineHint は \`L15-L17\` 範囲形式を正しく hash 末尾に合成する` with:

```ts
const result = await page.evaluate(() => {
  const container = document.getElementById('memo-preview')!;
  const link = document.createElement('a');
  const lineHint = document.createTextNode(' L15-L17');
  link.href = '?file=long.md#section-b';
  link.textContent = 'dummy';
  container.appendChild(link);
  container.appendChild(lineHint);
  try {
    return augmentHashWithTrailingLineHint(link, '#section-b');
  } finally {
    link.remove();
    lineHint.remove();
  }
});
```

- [ ] **Step 5: `L17-L15` 逆転範囲テストを置き換える**

Replace the `page.evaluate` body in `augmentHashWithTrailingLineHint は \`L17-L15\` 逆転範囲では start のみ採用` with:

```ts
const result = await page.evaluate(() => {
  const container = document.getElementById('memo-preview')!;
  const link = document.createElement('a');
  const lineHint = document.createTextNode(' L17-L15');
  link.href = '?file=long.md#section-b';
  link.textContent = 'dummy';
  container.appendChild(link);
  container.appendChild(lineHint);
  try {
    return augmentHashWithTrailingLineHint(link, '#section-b');
  } finally {
    link.remove();
    lineHint.remove();
  }
});
```

- [ ] **Step 6: 既存行範囲優先テストを置き換える**

Replace the `page.evaluate` body in `augmentHashWithTrailingLineHint は hash に行範囲が既にあれば link.nextSibling の L<n> で上書きしない` with:

```ts
const result = await page.evaluate(() => {
  const container = document.getElementById('memo-preview')!;
  const link = document.createElement('a');
  const lineHint = document.createTextNode(' L20');
  link.href = '?file=long.md#section-b:L15';
  link.textContent = 'dummy';
  container.appendChild(link);
  container.appendChild(lineHint);
  try {
    return augmentHashWithTrailingLineHint(link, '#section-b:L15');
  } finally {
    link.remove();
    lineHint.remove();
  }
});
```

- [ ] **Step 7: 空 hash 合成形テストを置き換える**

Replace the `page.evaluate` body in `augmentHashWithTrailingLineHint は空 hash の合成形は #L<n>（#:L<n> にはしない）` with:

```ts
const results = await page.evaluate(() => {
  const container = document.getElementById('memo-preview')!;
  const link = document.createElement('a');
  const lineHint = document.createTextNode(' L42');
  link.href = '?file=long.md';
  link.textContent = 'dummy';
  container.appendChild(link);
  container.appendChild(lineHint);
  try {
    return {
      empty: augmentHashWithTrailingLineHint(link, ''),
      hashOnly: augmentHashWithTrailingLineHint(link, '#')
    };
  } finally {
    link.remove();
    lineHint.remove();
  }
});
```

---

### Task 3: 検証してコミットする

**Files:**
- Verify: `tests/e2e/memo_jump.spec.ts`

- [ ] **Step 1: 危険パターンが消えたことを確認する**

Run:

```bash
rg -n "container\\.lastChild|lastChild\\.remove\\(\\)" tests/e2e/memo_jump.spec.ts
```

Expected: no output, exit code 1.

- [ ] **Step 2: 追加した明示 cleanup が 7 件あることを確認する**

Run:

```bash
rg -n "const lineHint = document\\.createTextNode|lineHint\\.remove\\(\\)" tests/e2e/memo_jump.spec.ts
```

Expected: `const lineHint = document.createTextNode(...)` が 7 件、`lineHint.remove()` が 7 件表示される。

- [ ] **Step 3: TypeScript 型チェックを実行する**

Run:

```bash
npm run typecheck
```

Expected: `tsc --noEmit` が PASS する。

- [ ] **Step 4: 対象 E2E を実行する**

Run:

```bash
npm run test:e2e -- memo_jump.spec.ts
```

Expected: `memo_jump.spec.ts` の Playwright tests が PASS する。

- [ ] **Step 5: 差分がテスト cleanup だけに閉じていることを確認する**

Run:

```bash
git diff -- tests/e2e/memo_jump.spec.ts
```

Expected:

- `document.createTextNode(...)` の戻り値を `lineHint` に保持している。
- `finally` は `link.remove(); lineHint.remove();` になっている。
- 既存の `expect(...)`、テスト名、`augmentHashWithTrailingLineHint(...)` の引数は変わっていない。
- `verify.sh`, `playwright.config.ts`, `tests/e2e/helpers.ts`, `src/**` に差分がない。

- [ ] **Step 6: コミットする**

Run:

```bash
git add tests/e2e/memo_jump.spec.ts
git commit -m "test: E2E DOM cleanupを明示ノード削除に変更"
```

Expected: one commit is created with only `tests/e2e/memo_jump.spec.ts` changed.

## セキュリティ確認

この変更は直接のセキュリティ修正ではない。テスト cleanup の誤削除リスクを下げることで、将来の回帰検知の信頼性を上げる。外部レビュー由来の backlog 記述は未検証入力として扱い、実装時には現行テストコードと実行結果で確認する。

## ロールバック

実装コミットを revert すれば、`tests/e2e/memo_jump.spec.ts` の cleanup 変更だけが戻る。プロダクトコード、`verify.sh`、Playwright 設定には触れないため、ロールバックの影響範囲は対象 E2E spec に限定される。
