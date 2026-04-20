# E2EテストのTypeScript移行 実装プラン

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `tests/e2e/*.spec.js` (6ファイル) と `playwright.config.js` を TypeScript strict へ移行し、`tsc --noEmit` を `verify.sh` に組み込む。

**Architecture:** Playwrightの組み込みTSローダーを利用し、`.ts`ファイルはトランスパイルなしで実行される。`tsc --noEmit` は純粋な型検査として `verify.sh` に追加。ファイル単位で段階的にリネーム＋ESM化し、各コミットで `verify.sh` がpassする状態を維持する。

**Tech Stack:** TypeScript 5.6+、@types/node 22+、Playwright 1.58、Node 20.11+

**設計書:** `docs/superpowers/specs/2026-04-20-e2e-typescript-migration-design.md`

---

## 前提

- 作業ブランチ: `feat/e2e-typescript-migration`（developから作成 ※プラン本体は `docs/e2e-typescript-migration-spec` で先行）
- 作業ディレクトリ: `/home/propan/personal_dev/markdown-view`
- ブランチ・コミットルール: `~/.claude/CLAUDE.md` に準拠（日本語コミット、developへ直接コミット禁止）

### 型定義のパターン（全spec共通で参照）

strictモードでは以下の対処を各ファイルで行う:

- **ブラウザグローバル関数** (`augmentHashWithTrailingLineHint`, `updateContent`, `activateSidebarTab`, `applyDocumentSearchQuery`, `isDirMode`): 必要なファイルの冒頭で `declare global` / `declare const` / `declare function` で宣言
- **`window` の拡張プロパティ** (`__lastWs`, `__realWsOnmessage`, `__dispatchWsMessage`, `__clickObservations`): `declare global { interface Window { ... } }` で宣言
- **`document.getElementById(...)` の `HTMLElement | null`**: 非null断言 `!` か早期throwで処理
- **`HTMLInputElement.value` 等のタグ固有プロパティ**: `instanceof HTMLInputElement` で型narrowing
- **`NodeFilter`, `Node`, `Range`, `Selection` 等のDOM型**: `lib` にDOMが含まれる（`target: ES2022` の既定で有効）ため追加設定不要

---

## Task 1: TS基盤整備

**Files:**
- Create: `tsconfig.json`
- Modify: `package.json`
- Modify: `verify.sh`
- Rename: `playwright.config.js` → `playwright.config.ts`

---

- [ ] **Step 1.1: 作業ブランチ作成**

```bash
git checkout develop
git pull --ff-only
git checkout -b feat/e2e-typescript-migration
```

Expected: `Switched to a new branch 'feat/e2e-typescript-migration'`

- [ ] **Step 1.2: `tsconfig.json` 新規作成**

ファイル: `tsconfig.json`（リポジトリルート）

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noEmit": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "resolveJsonModule": true,
    "types": ["node"]
  },
  "include": ["tests/e2e/**/*.ts", "playwright.config.ts"]
}
```

- [ ] **Step 1.3: `package.json` 更新**

ファイル: `package.json`。以下の内容で上書き保存:

```json
{
  "private": true,
  "engines": {
    "node": ">=20.11"
  },
  "scripts": {
    "test:e2e": "playwright test",
    "typecheck": "tsc --noEmit"
  },
  "devDependencies": {
    "@playwright/test": "^1.58.2",
    "@types/node": "^22.0.0",
    "typescript": "^5.6.0"
  }
}
```

- [ ] **Step 1.4: npm install で lockfile 更新**

Run: `npm install`
Expected: `added N packages, ...` と `package-lock.json` が更新される

- [ ] **Step 1.5: `verify.sh` 更新**

ファイル: `verify.sh`。以下の2箇所を追加する。

関数定義（`cleanup_on_error` と `run_step` の直後、`run_step "フォーマットチェック" ...` の直前）:

```bash
typecheck_e2e() {
  if [[ ! -d node_modules ]]; then
    echo "エラー: node_modules が存在しません。'npm ci' を先に実行してください。" >&2
    return 1
  fi
  npx --no-install tsc --noEmit
}
```

実行行の追加（`run_step "テスト実行" ...` の直後、`echo "==> 検証が正常に完了しました。"` の直前）:

```bash
run_step "E2E型チェック (tsc)" typecheck_e2e
```

- [ ] **Step 1.6: `playwright.config.js` を `.ts` にリネーム＋ESM化**

```bash
git mv playwright.config.js playwright.config.ts
```

ファイル: `playwright.config.ts`。全体を以下で置換:

```ts
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/e2e',
  timeout: 30000,
  // E2E は共通 fixture (tests/fixtures/e2e/{README,notes}.md) を読み書きするため
  // ワーカー並列実行で別 spec が同 fixture を上書きし test pollution を起こす。
  // 直列化で決定論性を確保する。
  workers: 1,
  use: {
    baseURL: 'http://localhost:4173',
    headless: true
  },
  projects: [
    {
      name: 'chromium',
      use: { browserName: 'chromium' }
    }
  ],
  webServer: {
    command: 'cargo run -- tests/fixtures/e2e --port 4173 --no-open',
    url: 'http://localhost:4173',
    reuseExistingServer: true,
    timeout: 120000
  }
});
```

- [ ] **Step 1.7: 型チェックとverify実行**

Run: `./verify.sh`
Expected: 全ステップ（フォーマット・clippy・cargo test・E2E型チェック）がpass

- [ ] **Step 1.8: E2Eテスト動作確認**

Run: `npm run test:e2e`
Expected: 全E2Eテストがpass（既存の`.js` specsはCommonJSのまま動作）

- [ ] **Step 1.9: コミット**

```bash
git add tsconfig.json package.json package-lock.json verify.sh playwright.config.ts
git commit -F - <<'EOF'
chore: E2E TypeScript移行の基盤を整備

変更内容:
- tsconfig.json を新規作成（strict, noEmit, NodeNext）
- package.json に typescript / @types/node 追加、typecheck スクリプトと engines.node>=20.11 を追加
- verify.sh に tsc --noEmit ステップを追加（node_modules 不在時は明示エラー）
- playwright.config.js を playwright.config.ts にリネームし ESM化

変更理由:
- E2EテストのTypeScript strict移行に必要な型検査基盤を確立する
- verify.sh を E2E型検査の Single Source of Truth に保つ

影響範囲:
- 既存の .js spec は CommonJS のまま動作（段階的移行）
- verify.sh 実行には node_modules が必要になる

テスト結果: ./verify.sh pass、npm run test:e2e pass
EOF
```

---

## Task 2: memo_quote.spec を TypeScript 化

**Files:**
- Rename: `tests/e2e/memo_quote.spec.js` → `tests/e2e/memo_quote.spec.ts`

**ブラウザグローバル:** なし
**`window` 拡張:** なし

---

- [ ] **Step 2.1: リネーム**

```bash
git mv tests/e2e/memo_quote.spec.js tests/e2e/memo_quote.spec.ts
```

- [ ] **Step 2.2: 冒頭の import と型注釈を適用**

ファイル: `tests/e2e/memo_quote.spec.ts`。冒頭3行を置換し、`selectParagraphText` に型注釈を追加。

Before (1-7行目):
```js
const fs = require('node:fs/promises');
const path = require('node:path');
const { test, expect } = require('@playwright/test');

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');
const readmePath = path.join(fixtureDir, 'README.md');
const notesPath = path.join(fixtureDir, 'notes.md');
```

After:
```ts
import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect, type Page } from '@playwright/test';

const fixtureDir = path.join(import.meta.dirname, '..', 'fixtures', 'e2e');
const readmePath = path.join(fixtureDir, 'README.md');
const notesPath = path.join(fixtureDir, 'notes.md');
```

`selectParagraphText` シグネチャ変更（`async function selectParagraphText(page, text) {` → 下記）:

```ts
async function selectParagraphText(page: Page, text: string) {
```

`page.evaluate` コールバック内の `document.getElementById('content')` が `HTMLElement | null` を返すため、非null断言を追加:

```ts
const walker = document.createTreeWalker(document.getElementById('content')!, NodeFilter.SHOW_TEXT);
```

また `range.selectNodeContents(node.parentElement)` の `node.parentElement` も null可能性があるため非null断言:

```ts
range.selectNodeContents(node.parentElement!);
```

`const selection = window.getSelection();` の戻り値は `Selection | null`。後続で `selection.removeAllRanges()` を呼ぶため、非null断言または早期return:

```ts
const selection = window.getSelection()!;
```

テスト本体（`test.beforeEach` 以降）は変更不要。

- [ ] **Step 2.3: 型チェック**

Run: `npm run typecheck`
Expected: PASS (エラーなし)

- [ ] **Step 2.4: E2E動作確認**

Run: `npx playwright test memo_quote`
Expected: 2 passed

- [ ] **Step 2.5: verify 実行**

Run: `./verify.sh`
Expected: 全ステップpass

- [ ] **Step 2.6: コミット**

```bash
git add tests/e2e/memo_quote.spec.ts
git commit -F - <<'EOF'
refactor: memo_quote.spec を TypeScript strict へ移行

変更内容:
- CommonJS require を ESM import へ置換
- __dirname を import.meta.dirname へ置換
- selectParagraphText に Page / string 型注釈を追加
- document.getElementById / window.getSelection の null可能性へ非null断言で対処

変更理由:
- E2EテストのTS化段階的移行 (Task 2/7)

影響範囲:
- memo_quote.spec のみ。他 .js spec は未変更

テスト結果: ./verify.sh pass、npx playwright test memo_quote pass (2 passed)
EOF
```

---

## Task 3: memo_sync.spec を TypeScript 化

**Files:**
- Rename: `tests/e2e/memo_sync.spec.js` → `tests/e2e/memo_sync.spec.ts`

**ブラウザグローバル:** なし
**`window` 拡張:** なし

---

- [ ] **Step 3.1: リネーム**

```bash
git mv tests/e2e/memo_sync.spec.js tests/e2e/memo_sync.spec.ts
```

- [ ] **Step 3.2: import と型注釈を適用**

ファイル: `tests/e2e/memo_sync.spec.ts`。

冒頭3行のrequire → import（Task 2.2 と同一パターン）:

```ts
import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect, type Page } from '@playwright/test';
```

`__dirname` → `import.meta.dirname`。

ヘルパー関数4つに型注釈:

```ts
async function openMemoTab(page: Page) {
async function openFileTab(page: Page) {
async function selectFile(page: Page, file: string) {
async function saveMemo(page: Page, text: string) {
```

`resetFixtures` は引数なし、`fs.readdir` の戻りは配列なので型注釈不要（TS推論で十分）。

テスト本体は変更不要（`{ page, context }` はPlaywright型から推論される）。

- [ ] **Step 3.3: 型チェック**

Run: `npm run typecheck`
Expected: PASS

- [ ] **Step 3.4: E2E動作確認**

Run: `npx playwright test memo_sync`
Expected: 6 passed

- [ ] **Step 3.5: verify 実行**

Run: `./verify.sh`
Expected: 全ステップpass

- [ ] **Step 3.6: コミット**

```bash
git add tests/e2e/memo_sync.spec.ts
git commit -F - <<'EOF'
refactor: memo_sync.spec を TypeScript strict へ移行

変更内容:
- CommonJS require を ESM import へ置換
- __dirname を import.meta.dirname へ置換
- openMemoTab / openFileTab / selectFile / saveMemo に Page / string 型注釈を追加

変更理由:
- E2EテストのTS化段階的移行 (Task 3/7)

影響範囲:
- memo_sync.spec のみ

テスト結果: ./verify.sh pass、npx playwright test memo_sync pass (6 passed)
EOF
```

---

## Task 4: markdown_links.spec を TypeScript 化

**Files:**
- Rename: `tests/e2e/markdown_links.spec.js` → `tests/e2e/markdown_links.spec.ts`

**ブラウザグローバル:** なし
**`window` 拡張:** `__clickObservations`

---

- [ ] **Step 4.1: リネーム**

```bash
git mv tests/e2e/markdown_links.spec.js tests/e2e/markdown_links.spec.ts
```

- [ ] **Step 4.2: import、declare global、型注釈を適用**

ファイル: `tests/e2e/markdown_links.spec.ts`。冒頭3行を置換:

```ts
import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect, type Page } from '@playwright/test';

declare global {
  interface Window {
    __clickObservations: Record<string, { defaultPrevented: boolean }>;
  }
}
```

`__dirname` → `import.meta.dirname`。

`installClickObserver` に型注釈:

```ts
async function installClickObserver(page: Page) {
```

`installClickObserver` 内の `event.target.closest(...)` は `event.target` が `EventTarget | null`。`closest` を呼ぶには `Element` が必要。`HTMLElement` にキャストして対処:

```ts
await page.evaluate(() => {
  window.__clickObservations = {};
  window.addEventListener('click', function(event) {
    const link = (event.target as HTMLElement | null)?.closest('a[href]');
    if (!link) return;
    window.__clickObservations[link.getAttribute('href')!] = {
      defaultPrevented: event.defaultPrevented
    };
    event.preventDefault();
  });
});
```

- [ ] **Step 4.3: 型チェック**

Run: `npm run typecheck`
Expected: PASS

- [ ] **Step 4.4: E2E動作確認**

Run: `npx playwright test markdown_links`
Expected: 13 passed

- [ ] **Step 4.5: verify 実行**

Run: `./verify.sh`
Expected: 全ステップpass

- [ ] **Step 4.6: コミット**

```bash
git add tests/e2e/markdown_links.spec.ts
git commit -F - <<'EOF'
refactor: markdown_links.spec を TypeScript strict へ移行

変更内容:
- CommonJS require を ESM import へ置換、__dirname を import.meta.dirname へ置換
- window.__clickObservations を declare global で型宣言
- installClickObserver に Page 型注釈、event.target の null可能性へキャストで対処

変更理由:
- E2EテストのTS化段階的移行 (Task 4/7)

影響範囲:
- markdown_links.spec のみ

テスト結果: ./verify.sh pass、npx playwright test markdown_links pass (13 passed)
EOF
```

---

## Task 5: memo_jump.spec を TypeScript 化

**Files:**
- Rename: `tests/e2e/memo_jump.spec.js` → `tests/e2e/memo_jump.spec.ts`

**ブラウザグローバル:** `augmentHashWithTrailingLineHint`
**`window` 拡張:** `updateContent`

---

- [ ] **Step 5.1: リネーム**

```bash
git mv tests/e2e/memo_jump.spec.js tests/e2e/memo_jump.spec.ts
```

- [ ] **Step 5.2: import、declare global、型注釈を適用**

ファイル: `tests/e2e/memo_jump.spec.ts`。冒頭3行を置換＋宣言追加:

```ts
import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect, type Page } from '@playwright/test';

declare global {
  interface Window {
    updateContent: (data: { content: string; toc?: string }, opts: Record<string, unknown>) => void;
  }
  // ブラウザ側バンドルで定義されるグローバル関数（page.evaluate内で参照）
  function augmentHashWithTrailingLineHint(link: HTMLAnchorElement, hash: string): string;
}
```

`__dirname` → `import.meta.dirname`。

ヘルパーに型注釈:

```ts
async function selectParagraphText(page: Page, text: string) {
async function lineBlockStartOf(page: Page, needle: string) {
```

`selectParagraphText` 内の null可能性対処は Task 2 と同一パターン（`document.getElementById('content')!`、`window.getSelection()!`、`node.parentElement!`）。

`lineBlockStartOf` 内の `el.getAttribute(...)` は `string | null` を返すが `parseInt` は第1引数に `string | null` を取れない。`|| ''` でfallback:

```ts
const start = parseInt(el.getAttribute('data-line-block-start') || el.getAttribute('data-source-start-line') || '', 10);
const end = parseInt(el.getAttribute('data-line-block-end') || el.getAttribute('data-source-end-line') || '', 10);
```

`document.querySelector('#content h2')` は `Element | null` だが、`classList.add` を呼ぶには `Element` で十分。非null断言を付ける:

```ts
const h = document.querySelector('#content h2')!;
h.classList.add('jump-highlight');
```

検証系の `h && h.classList.contains(...)` 箇所は元のまま（`h` が `Element | null` を短絡評価する慣用句）。

- [ ] **Step 5.3: 型チェック**

Run: `npm run typecheck`
Expected: PASS

- [ ] **Step 5.4: E2E動作確認**

Run: `npx playwright test memo_jump`
Expected: 15 passed

- [ ] **Step 5.5: verify 実行**

Run: `./verify.sh`
Expected: 全ステップpass

- [ ] **Step 5.6: コミット**

```bash
git add tests/e2e/memo_jump.spec.ts
git commit -F - <<'EOF'
refactor: memo_jump.spec を TypeScript strict へ移行

変更内容:
- CommonJS require を ESM import へ置換、__dirname を import.meta.dirname へ置換
- augmentHashWithTrailingLineHint / Window.updateContent を declare で型宣言
- selectParagraphText / lineBlockStartOf に型注釈追加
- getAttribute の null可能性は || '' fallback、DOM検索結果は非null断言で対処

変更理由:
- E2EテストのTS化段階的移行 (Task 5/7)

影響範囲:
- memo_jump.spec のみ

テスト結果: ./verify.sh pass、npx playwright test memo_jump pass (15 passed)
EOF
```

---

## Task 6: text_selection_defer.spec を TypeScript 化

**Files:**
- Rename: `tests/e2e/text_selection_defer.spec.js` → `tests/e2e/text_selection_defer.spec.ts`

**ブラウザグローバル:** なし
**`window` 拡張:** `__lastWs`, `__realWsOnmessage`, `__dispatchWsMessage`

---

- [ ] **Step 6.1: リネーム**

```bash
git mv tests/e2e/text_selection_defer.spec.js tests/e2e/text_selection_defer.spec.ts
```

- [ ] **Step 6.2: import、declare global、型注釈を適用**

ファイル: `tests/e2e/text_selection_defer.spec.ts`。冒頭3行を置換＋宣言追加:

```ts
import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect, type Page } from '@playwright/test';

declare global {
  interface Window {
    __lastWs: WebSocket & { onmessage: ((ev: MessageEvent) => void) | null };
    __realWsOnmessage: (ev: { data: string }) => void;
    __dispatchWsMessage: (payload: unknown) => void;
  }
}
```

`__dirname` → `import.meta.dirname`。

ヘルパーに型注釈:

```ts
async function selectParagraphText(page: Page, text: string) {
async function clearSelection(page: Page) {
async function activeTocLabel(page: Page) {
async function activeTocLabelOrEmpty(page: Page) {
async function clickTocLink(page: Page, id: string) {
async function stabilizeWebSocketHarness(page: Page) {
async function loadDenseHeadingFixture(page: Page) {
async function loadBottomHeadingFixture(page: Page) {
```

`selectParagraphText` 内の null可能性対処は以下のパターン（Task 2 と同じ）:

```ts
const walker = document.createTreeWalker(document.getElementById('content')!, NodeFilter.SHOW_TEXT);
// ...
const selection = window.getSelection()!;
// ...
range.selectNodeContents(node.parentElement!);
```

`clickTocLink` 内の `link.click()` は `link` が `Element | null` だが、直前のthrowでnullを除外している。strictでは narrowing が効くので変更不要。ただし `link` が `Element` 型だと `.click()` がないので `HTMLElement` へキャスト:

```ts
const link = document.querySelector(`#toc a[href="#${targetId}"]`) as HTMLAnchorElement | null;
if (!link) {
  throw new Error(`toc link not found: ${targetId}`);
}
link.click();
```

`loadDenseHeadingFixture` / `loadBottomHeadingFixture` 内の `document.getElementById(...)` は null可能性あり。`parseFloat(window.getComputedStyle(alpha).scrollMarginTop)` の `alpha` 等は非null断言で対処（ヘルパーは fixture ロード直後に呼ばれるため存在を前提としている）:

```ts
const alpha = document.getElementById('alpha')!;
const beta = document.getElementById('beta')!;
```

`stabilizeWebSocketHarness` 内の `window.__lastWs.onmessage = function() {};` のような代入は上記 declare で解決される。

- [ ] **Step 6.3: 型チェック**

Run: `npm run typecheck`
Expected: PASS

- [ ] **Step 6.4: E2E動作確認**

Run: `npx playwright test text_selection_defer`
Expected: 20 passed

- [ ] **Step 6.5: verify 実行**

Run: `./verify.sh`
Expected: 全ステップpass

- [ ] **Step 6.6: コミット**

```bash
git add tests/e2e/text_selection_defer.spec.ts
git commit -F - <<'EOF'
refactor: text_selection_defer.spec を TypeScript strict へ移行

変更内容:
- CommonJS require を ESM import へ置換、__dirname を import.meta.dirname へ置換
- window.__lastWs / __realWsOnmessage / __dispatchWsMessage を declare global で型宣言
- 全ヘルパー関数 (8個) に Page / string 型注釈を追加
- DOM検索結果の null可能性は非null断言とキャストで対処

変更理由:
- E2EテストのTS化段階的移行 (Task 6/7)

影響範囲:
- text_selection_defer.spec のみ

テスト結果: ./verify.sh pass、npx playwright test text_selection_defer pass (20 passed)
EOF
```

---

## Task 7: document_search.spec を TypeScript 化

**Files:**
- Rename: `tests/e2e/document_search.spec.js` → `tests/e2e/document_search.spec.ts`

**ブラウザグローバル:** `isDirMode`, `updateContent`, `activateSidebarTab`, `applyDocumentSearchQuery`
**`window` 拡張:** `__lastWs`, `__realWsOnmessage`, `WebSocket` の上書き

---

- [ ] **Step 7.1: リネーム**

```bash
git mv tests/e2e/document_search.spec.js tests/e2e/document_search.spec.ts
```

- [ ] **Step 7.2: import、declare global、型注釈を適用**

ファイル: `tests/e2e/document_search.spec.ts`。冒頭1行を置換＋宣言追加:

```ts
import { test, expect, type Page } from '@playwright/test';

declare global {
  interface Window {
    __lastWs: WebSocket & { onmessage: ((ev: MessageEvent) => void) | null };
    __realWsOnmessage: (ev: { data: string }) => void;
  }
  // ブラウザ側バンドルで定義される変数・関数
  var isDirMode: boolean;
  function updateContent(data: { content: string; toc?: string }, opts?: Record<string, unknown>): void;
  function activateSidebarTab(tab: string): void;
  function applyDocumentSearchQuery(value: string): void;
}

export {}; // ファイルをモジュールとして扱わせる（declare global の要件）
```

注: `isDirMode` はブラウザバンドル側で `var` 宣言されたグローバル変数のため、TS側も `var` で宣言する必要がある（`let`/`const` では `globalThis.isDirMode` 扱いにならない）。

ヘルパー型注釈:

```ts
function searchFixtureContent(): string {
function searchFixtureToc(): string {
async function loadSearchFixture(page: Page) {
async function visibleMatchCount(page: Page) {
async function currentMatchText(page: Page) {
async function stabilizeWebSocketHarness(page: Page) {
async function setDocumentSearchQuery(page: Page, query: string) {
```

`setDocumentSearchQuery` 内で `input.value = value;` を使うため、`HTMLInputElement` へ narrowing:

```ts
await page.evaluate((value) => {
  const input = document.getElementById('document-search-input');
  if (!(input instanceof HTMLInputElement)) {
    throw new Error('document search input not found');
  }
  input.value = value;
  if (typeof applyDocumentSearchQuery === 'function') {
    applyDocumentSearchQuery(value);
    return;
  }
  input.dispatchEvent(new Event('input', { bubbles: true }));
}, query);
```

`visibleMatchCount` 内の `mark.dataset.matchId` は `string | undefined`。`Set` に入る分には問題なし。`Array.from(...).map((mark) => mark.dataset.matchId)` の `mark` は `Element` だが `dataset` は `HTMLElement` のプロパティ。`querySelectorAll` で `HTMLElement` を得るためジェネリクス指定:

```ts
return page.evaluate(() => {
  return new Set(
    Array.from(document.querySelectorAll<HTMLElement>('#content mark.document-search-match')).map((mark) => mark.dataset.matchId)
  ).size;
});
```

TestWebSocketの上書き箇所（79-89行目あたり）では `this` が `any` 相当になる。クラス定義で置換:

```ts
const NativeWebSocket = window.WebSocket;
class TestWebSocket extends NativeWebSocket {
  constructor(...args: ConstructorParameters<typeof WebSocket>) {
    super(...args);
    (window as unknown as { __lastWs: WebSocket }).__lastWs = this;
  }
}
window.WebSocket = TestWebSocket;
```

注: 既存コードが `function` ベースで記述されている場合、その意味を保つ最小限の型付けで済ませる。実装詳細は移行時のコードを見て判断する。

- [ ] **Step 7.3: 型チェック**

Run: `npm run typecheck`
Expected: PASS

- [ ] **Step 7.4: E2E動作確認**

Run: `npx playwright test document_search`
Expected: 23 passed

- [ ] **Step 7.5: verify 実行**

Run: `./verify.sh`
Expected: 全ステップpass

- [ ] **Step 7.6: コミット**

```bash
git add tests/e2e/document_search.spec.ts
git commit -F - <<'EOF'
refactor: document_search.spec を TypeScript strict へ移行

変更内容:
- CommonJS require を ESM import へ置換
- ブラウザグローバル (isDirMode / updateContent / activateSidebarTab / applyDocumentSearchQuery) を declare で型宣言
- window.__lastWs / __realWsOnmessage を declare global で型宣言
- 全ヘルパー関数 (7個) に型注釈追加
- HTMLInputElement narrowing、querySelectorAll ジェネリクス、WebSocket 上書き箇所の型対応

変更理由:
- E2EテストのTS化段階的移行 (Task 7/7) — 全 spec の移行完了

影響範囲:
- document_search.spec のみ。全E2E spec が .ts 化完了

テスト結果: ./verify.sh pass、npx playwright test document_search pass (23 passed)
EOF
```

---

## Task 8: CLAUDE.md 更新

**Files:**
- Modify: `CLAUDE.md`

---

- [ ] **Step 8.1: `CLAUDE.md` のビルドコマンド節を更新**

ファイル: `CLAUDE.md`。「ビルド・テスト・検証コマンド」セクションの該当行を更新する。

既存のコードブロック内の末尾付近（`cargo test` 行の下）に、以下を追記:

```bash
npm run typecheck                        # E2Eテストの型チェック
npm run test:e2e                          # E2Eテスト実行（Playwright）
```

- [ ] **Step 8.2: `CLAUDE.md` のテスト構成節を更新**

「テスト構成」セクションの `tests/e2e/*.spec.js` への言及を `.spec.ts` に置換。該当行が無ければスキップ（既存の記述に応じて調整）。

具体的には、「テスト構成」セクション末尾に以下を追記:

```markdown
- `tests/e2e/*.spec.ts` — Playwright E2Eテスト（TypeScript strict、`npm run test:e2e`で実行）
```

「コード規約」セクション末尾にも追記:

```markdown
- E2Eテストは TypeScript strict で記述し、`tsc --noEmit`（`verify.sh`内）で型検査する
```

- [ ] **Step 8.3: verify 実行**

Run: `./verify.sh`
Expected: 全ステップpass（CLAUDE.md変更は検証対象外だが念のため確認）

- [ ] **Step 8.4: コミット**

```bash
git add CLAUDE.md
git commit -F - <<'EOF'
docs: CLAUDE.md を E2E TypeScript化に追従して更新

変更内容:
- ビルド・テスト・検証コマンド節に npm run typecheck / test:e2e を追記
- テスト構成節に tests/e2e/*.spec.ts の記載を追加
- コード規約節に E2E は TypeScript strict で記述する方針を明記

変更理由:
- E2E全specのTS化完了に伴うドキュメント整合性確保

影響範囲:
- ドキュメントのみ。コード変更なし

テスト結果: ./verify.sh pass
EOF
```

---

## PR作成

- [ ] **Step 9.1: リモートへpush**

```bash
git push -u origin feat/e2e-typescript-migration
```

- [ ] **Step 9.2: PR作成**

```bash
gh pr create --base develop --title "feat: E2EテストをTypeScript strictへ移行" --body "$(cat <<'EOF'
## Summary
- `tests/e2e/*.spec.js` (6ファイル) と `playwright.config.js` を TypeScript strict へ移行
- `tsc --noEmit` を `verify.sh` に追加、`node_modules` 不在時は明示エラー
- ブラウザグローバルと `window` 拡張は `declare global` で型付け

## Spec / Plan
- 設計書: `docs/superpowers/specs/2026-04-20-e2e-typescript-migration-design.md`
- 実装プラン: `docs/superpowers/plans/2026-04-20-e2e-typescript-migration.md`

## Test plan
- [x] `./verify.sh` 全ステップpass（各コミットで検証済み）
- [x] `npm run test:e2e` 全testpass（各specコミットで検証済み）
- [ ] PR branchで再度verify.sh実行
EOF
)"
```

---

## 検証サマリー

各Task完了時に以下が満たされていること:

| 項目 | 検証方法 |
|------|----------|
| 型検査pass | `npm run typecheck` → エラーなし |
| E2E動作不変 | `npm run test:e2e` 全passまたは対象spec単独pass |
| Rust検証pass | `./verify.sh` 全ステップpass |
| コミット単位 | 1Task=1コミット、日本語メッセージ、小変更 |
| スコープ逸脱なし | ヘルパーDRY化 / インラインJS は触らない |

## リスクと早期検出

- **ビルド前提の崩れ**: `package.json` に `"type": "module"` を追加していない（`.ts` は Playwright組み込みTSローダーで処理、`.js` は CommonJS のまま）
- **型エラーの見逃し**: 各 Task の 型チェック ステップで `npm run typecheck` を必ず実行
- **E2E動作リグレッション**: 各 Task で個別spec実行＋ `./verify.sh`（cargo testも含む）
- **node_modules 欠落**: `verify.sh` の `typecheck_e2e` 関数が親切なエラーで停止

## スコープ外（別タスク候補）

- E2EヘルパーのDRY化 (`tests/e2e/helpers.ts` 抽出)
- E2Eの`verify.sh`統合（`npm run test:e2e` の追加）
- インラインブラウザJS (`src/template/assets/js/*.js`) のTS化
