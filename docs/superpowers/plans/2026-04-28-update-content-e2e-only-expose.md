# updateContent E2E 限定 expose Implementation Plan
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** production HTML では `window.updateContent` を公開せず、E2E 実行時だけ `window.__MV_E2E__ === true` で公開する。

**Architecture:** `content.js` の `function updateContent(...)` を `let updateContent = function updateContent(...) { ... };` に変え、classic script の top-level function 宣言が `window` property を作る経路を閉じる。direct-call E2E だけが `page.goto()` 前に `window.__MV_E2E__ = true` を注入する。production 非露出は独立 E2E spec で固定する。

**Tech Stack:** JavaScript inline asset（`let` 使用）、TypeScript strict、Playwright `page.addInitScript` / `page.evaluate`

**設計書:** `docs/superpowers/specs/2026-04-28-update-content-e2e-only-expose-design.md`

---

## File Map
- Modify: `src/template/assets/js/content.js` - `updateContent` を lexical binding 化し、E2E フラグ時だけ `window` へ expose
- Modify: `tests/e2e/memo_jump.spec.ts` - direct-call 用に `__MV_E2E__` を注入
- Modify: `tests/e2e/document_search.spec.ts` - direct-call 用に `__MV_E2E__` を注入し、呼び出しを `window.updateContent` へ寄せる
- Create: `tests/e2e/update_content_exposure.spec.ts` - production 非露出と E2E expose を検証
- Modify: `docs/todo/BACKLOG.md` - 該当 backlog を完了化

### Task 1: production 非露出の red test を追加する
**Files:**
- Create: `tests/e2e/update_content_exposure.spec.ts`

- [ ] **Step 1: E2E spec を作成する**
```typescript
import { test, expect } from '@playwright/test';

declare global {
  interface Window {
    __MV_E2E__?: boolean;
    updateContent: (data: { content: string; toc: string }, opts?: Record<string, unknown>) => void;
  }
}

export {};

test('production実行ではwindow.updateContentを公開しない', async ({ page }) => {
  await page.goto('/');
  const exposedType = await page.evaluate(() => typeof window.updateContent);
  expect(exposedType).toBe('undefined');
});

test('E2Eフラグがtrueならwindow.updateContentを公開する', async ({ page }) => {
  await page.addInitScript(() => {
    window.__MV_E2E__ = true;
  });
  await page.goto('/');
  const exposedType = await page.evaluate(() => typeof window.updateContent);
  expect(exposedType).toBe('function');
});
```
- [ ] **Step 2: red test を確認する**
Run:
```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts --grep "production実行ではwindow.updateContentを公開しない"
```
Expected: FAIL。現状は `window.updateContent` が無条件に公開されるため `Received: "function"` になる。

- [ ] **Step 3: positive test の現状を確認する**
Run:
```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts --grep "E2Eフラグがtrueならwindow.updateContentを公開する"
```
Expected: PASS。Task 2 後は gate が閉じすぎていないことを検証する。

### Task 2: `content.js` の expose を閉じる
**Files:**
- Modify: `src/template/assets/js/content.js`
- Test: `tests/e2e/update_content_exposure.spec.ts`

- [ ] **Step 1: 対象箇所を確認する**
Run:
```bash
rg -n "function updateContent|window\\.updateContent = updateContent|テスト専用 expose" src/template/assets/js/content.js
```
Expected: `function updateContent(data, options) {` と末尾の `window.updateContent = updateContent;` が見つかる。

- [ ] **Step 2: function 宣言を lexical binding に変える**
変更前:
```javascript
function updateContent(data, options) {
  options = options || {};
```
変更後:
```javascript
let updateContent = function updateContent(data, options) {
  options = options || {};
```
関数末尾も変更する。
```javascript
  if (!hasContractViolation && typeof rememberAppliedLiveUpdate === 'function') {
    rememberAppliedLiveUpdate(safeData);
  }
};
```

- [ ] **Step 3: 無条件 expose を E2E gate に変える**
変更後:
```javascript
// テスト専用 expose。production では window に公開しない。
// E2E は page.addInitScript で window.__MV_E2E__ = true を事前注入する。
if (window.__MV_E2E__ === true) {
  window.updateContent = updateContent;
}
```

- [ ] **Step 4: exposure spec と型チェックを実行する**
Run:
```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts
npm run typecheck
```
Expected: 2 tests PASS、TypeScript PASS。

- [ ] **Step 5: コミットする**
Run:
```bash
git add src/template/assets/js/content.js tests/e2e/update_content_exposure.spec.ts
git commit -m "test: updateContentのE2E限定exposeを固定"
```
Expected: コミット成功。

### Task 3: 既存 direct-call E2E を E2E フラグ対応にする
**Files:**
- Modify: `tests/e2e/memo_jump.spec.ts`
- Modify: `tests/e2e/document_search.spec.ts`

- [ ] **Step 1: `memo_jump.spec.ts` の型と beforeEach を変更する**
`Window` 宣言:
```typescript
declare global {
  interface Window {
    __MV_E2E__?: boolean;
    updateContent: (data: { content: string; toc: string }, opts?: Record<string, unknown>) => void;
    scheduleBufferedLiveUpdate: (data: { content: string; toc: string }) => void;
  }
```
`beforeEach`:
```typescript
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.__MV_E2E__ = true;
  });
  await resetLongFixture();
  await page.goto('/?file=long.md');
  await expect(page.locator('#content')).toContainText('TARGET BLOCK');
});
```

- [ ] **Step 2: `document_search.spec.ts` の型を変更する**
変更後:
```typescript
declare global {
  interface Window {
    __MV_E2E__?: boolean;
    __lastWs: WebSocket & { onmessage: ((ev: MessageEvent) => void) | null };
    __realWsOnmessage: (ev: { data: string }) => void;
    updateContent: (data: { content: string; toc: string }, opts?: Record<string, unknown>) => void;
  }
  var isDirMode: boolean;
  var currentFile: string;
```
既存の `function updateContent(data: { content: string; toc: string }, opts?: Record<string, unknown>): void;` 宣言は削除する。

- [ ] **Step 3: `document_search.spec.ts` の呼び出しと beforeEach を変更する**
Run:
```bash
rg -n "([^.]|^)updateContent\\(" tests/e2e/document_search.spec.ts
```
Expected: unqualified `updateContent(` が見つかる。

各呼び出しを `window.updateContent(...)` に変える。例:
```typescript
window.updateContent({ content, toc });
```
`beforeEach` の `page.addInitScript` callback 冒頭:
```typescript
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.__MV_E2E__ = true;
    const NativeWebSocket = window.WebSocket;
```
置換後:
```bash
rg -n "([^.]|^)updateContent\\(" tests/e2e/document_search.spec.ts
```
Expected: match なし。

- [ ] **Step 4: 対象 E2E と型チェックを実行する**
Run:
```bash
npm run test:e2e -- tests/e2e/memo_jump.spec.ts tests/e2e/document_search.spec.ts tests/e2e/update_content_exposure.spec.ts
npm run typecheck
```
Expected: E2E 全件 PASS、TypeScript PASS。

- [ ] **Step 5: コミットする**
Run:
```bash
git add tests/e2e/memo_jump.spec.ts tests/e2e/document_search.spec.ts
git commit -m "test: updateContent direct-call E2Eに明示フラグを注入"
```
Expected: コミット成功。

### Task 4: backlog 完了化と最終検証
**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: backlog を完了にする**
変更後:
```markdown
- [x] `window.updateContent` を E2E モード限定 expose に変更
```

- [ ] **Step 2: 最終検証を実行する**
Run:
```bash
npm run typecheck
npm run test:e2e -- tests/e2e/memo_jump.spec.ts tests/e2e/document_search.spec.ts tests/e2e/update_content_exposure.spec.ts
./verify.sh
```
Expected: すべて PASS。`./verify.sh` が Playwright E2E を含まない場合は、E2E 結果を completion report に別記する。

- [ ] **Step 3: セキュリティ観点を grep で確認する**
Run:
```bash
rg -n "let updateContent = function updateContent|window\\.updateContent = updateContent|__MV_E2E__|updateContent\\(" src/template/assets/js tests/e2e
```
Expected:
- `window.updateContent = updateContent` は `if (window.__MV_E2E__ === true)` の内側だけ
- `memo_jump.spec.ts` と `document_search.spec.ts` は `page.goto()` 前に `window.__MV_E2E__ = true` を注入
- `websocket.js` と `fetch.js` の `updateContent(data)` 呼び出しは残る

- [ ] **Step 4: backlog 変更をコミットする**
Run:
```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: updateContent E2E限定exposeのbacklogを完了"
```
Expected: コミット成功。

## Self-Review
- Spec coverage: production 非露出は Task 1-2、E2E expose は Task 1-2、既存 direct-call 維持は Task 3、正規更新経路維持とセキュリティ確認は Task 4。
- Placeholder scan: 未決定の実装項目なし。変更 step は具体的な置換コードを含む。検証コマンドと期待結果を明記済み。
- Type consistency: `Window.__MV_E2E__?: boolean` は全 spec で同じ型。`Window.updateContent` は required property、`{ content: string; toc: string }` と optional `opts` で統一。`content.js` は `let updateContent = function updateContent` と `window.__MV_E2E__ === true` gate で統一。
