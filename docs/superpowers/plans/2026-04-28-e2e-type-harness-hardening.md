# E2E Type Harness Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** E2E の browser global 型、`updateContent` payload 型、WebSocket テストハーネスを共通化し、追加 strict flag でも `npm run typecheck` が通る状態にする。

**Architecture:** `tests/e2e/globals.d.ts` を ambient 宣言の SSoT にし、spec 内の `declare global` を削除する。WebSocket 差し替えは `tests/e2e/browser/test-websocket.ts` の serializable init function に集約し、spec は `page.addInitScript()` から呼ぶ。production Rust/JS には触れない。

**Tech Stack:** TypeScript, Playwright, NodeNext, npm, Markdown

**Design reference:** `docs/superpowers/specs/2026-04-28-e2e-type-harness-hardening-design.md`

---

## ファイル構成

- Create: `tests/e2e/globals.d.ts` - browser global と bundle 関数の ambient 宣言。
- Create: `tests/e2e/browser/test-websocket.ts` - `page.addInitScript()` に渡す WebSocket 差し替え関数。
- Modify: `tests/e2e/{document_search,text_selection_defer,memo_jump,update_content_exposure,markdown_links}.spec.ts` - 重複宣言削除と strict 対応。
- Modify: `tsconfig.json` - `noUncheckedIndexedAccess` と `exactOptionalPropertyTypes` を追加。
- Modify: `docs/todo/BACKLOG.md` - 対象 5 項目を完了化。

---

### Task 1: Ambient 型宣言と strict flag を追加する

**Files:**
- Create: `tests/e2e/globals.d.ts`
- Modify: `tsconfig.json`

- [ ] **Step 1: baseline typecheck を確認する**

Run: `npm run typecheck`

Expected:

```text
> typecheck
> tsc --noEmit
```

- [ ] **Step 2: `tests/e2e/globals.d.ts` を追加する**

```ts
export {};

declare global {
  type UpdateContentPayload = {
    content: string;
    toc: string;
  };
  type UpdateContentOptions = Record<string, unknown>;
  type ClickObservation = {
    defaultPrevented: boolean;
  };
  type TestWebSocketInstance = WebSocket & {
    onmessage: ((ev: MessageEvent) => void) | null;
  };

  interface Window {
    __MV_E2E__?: boolean;
    __lastWs?: TestWebSocketInstance;
    __realWsOnmessage?: (ev: { data: string }) => void;
    __dispatchWsMessage?: (payload: unknown) => void;
    __markPendingCalls?: number;
    __tocActiveChanges?: string[];
    __stopTocObserver?: () => void;
    __clickObservations?: Record<string, ClickObservation>;
    markPendingTocNavigation?: (id: string) => void;
    updateContent: (data: UpdateContentPayload, opts?: UpdateContentOptions) => void;
    scheduleBufferedLiveUpdate: (data: UpdateContentPayload) => void;
  }

  var isDirMode: boolean;
  var currentFile: string;
  function activateSidebarTab(tab: string): void;
  function applyDocumentSearchQuery(value: string): void;
  function moveDocumentSearch(direction: number): void;
  function selectFile(file: string, pushHistory?: boolean, options?: UpdateContentOptions): void;
  function augmentHashWithTrailingLineHint(link: HTMLAnchorElement, hash: string): string;
}
```

- [ ] **Step 3: `tsconfig.json` に strict flag を追加する**

`compilerOptions` に次を追加する。

```json
"noUncheckedIndexedAccess": true,
"exactOptionalPropertyTypes": true
```

- [ ] **Step 4: duplicate global で失敗することを確認する**

Run: `npm run typecheck`

Expected: 既存 `declare global` と `globals.d.ts` が重複して FAIL。`TS2717` または `Cannot redeclare` 系のエラーが出る。

- [ ] **Step 5: コミットする**

```bash
git add tests/e2e/globals.d.ts tsconfig.json
git commit -m "test: E2Eグローバル型の集約先を追加"
```

---

### Task 2: WebSocket テストハーネスを共通化する

**Files:**
- Create: `tests/e2e/browser/test-websocket.ts`
- Modify: `tests/e2e/document_search.spec.ts`
- Modify: `tests/e2e/text_selection_defer.spec.ts`

- [ ] **Step 1: `tests/e2e/browser/test-websocket.ts` を追加する**

```ts
export type TestWebSocketHarnessOptions = {
  setE2EFlag?: boolean;
  shortenReconnectDelay?: boolean;
};

export function installTestWebSocketHarness(options: TestWebSocketHarnessOptions = {}) {
  if (options.setE2EFlag === true) {
    window.__MV_E2E__ = true;
  }

  const NativeWebSocket = window.WebSocket;
  const nativeSetTimeout = window.setTimeout.bind(window);

  class TestWebSocket extends NativeWebSocket {
    constructor(...args: ConstructorParameters<typeof WebSocket>) {
      super(...args);
      window.__lastWs = this as TestWebSocketInstance;
    }
  }

  TestWebSocket.prototype = NativeWebSocket.prototype;
  Object.setPrototypeOf(TestWebSocket, NativeWebSocket);
  window.WebSocket = TestWebSocket;

  if (options.shortenReconnectDelay === true) {
    window.setTimeout = ((fn: TimerHandler, delay?: number, ...args: unknown[]) => {
      const effectiveDelay = delay === 30000 ? 50 : delay;
      return nativeSetTimeout(fn, effectiveDelay, ...args);
    }) as typeof window.setTimeout;
  }
}
```

- [ ] **Step 2: `document_search.spec.ts` を共通ハーネスへ移行する**

削除: ファイル冒頭の `declare global` と `export {};`、`test.beforeEach` 内の inline `TestWebSocket` 定義。

追加 import:

```ts
import { installTestWebSocketHarness } from './browser/test-websocket';
```

`stabilizeWebSocketHarness` を次に置き換える。

```ts
async function stabilizeWebSocketHarness(page: Page) {
  await page.waitForFunction(() => window.__lastWs && typeof window.__lastWs.onmessage === 'function');
  await page.evaluate(() => {
    const socket = window.__lastWs;
    if (!socket || typeof socket.onmessage !== 'function') {
      throw new Error('WebSocket onmessage is not ready');
    }
    // __dispatchWsMessage は MessageEvent を生成せず { data: string } を直接渡すため、
    // E2E ハーネス内では onmessage の契約をテスト用の狭い型へ bridge する。
    window.__realWsOnmessage = socket.onmessage as unknown as (ev: { data: string }) => void;
    socket.onmessage = function() {};
  });
}
```

`test.beforeEach` は次にする。

```ts
test.beforeEach(async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.goto('/');
  await stabilizeWebSocketHarness(page);
  await loadSearchFixture(page);
});
```

- [ ] **Step 3: `text_selection_defer.spec.ts` を共通ハーネスへ移行する**

削除: ファイル冒頭の `declare global`、`test.beforeEach` 内の inline `TestWebSocket` 定義と `setTimeout` override。

追加 import:

```ts
import { installTestWebSocketHarness } from './browser/test-websocket';
```

`stabilizeWebSocketHarness` を次に置き換える。

```ts
async function stabilizeWebSocketHarness(page: Page) {
  await page.waitForFunction(() => window.__lastWs && typeof window.__lastWs.onmessage === 'function');
  await page.evaluate(() => {
    const socket = window.__lastWs;
    if (!socket || typeof socket.onmessage !== 'function') {
      throw new Error('WebSocket onmessage is not ready');
    }
    // __dispatchWsMessage は MessageEvent を生成せず { data: string } を直接渡すため、
    // E2E ハーネス内では onmessage の契約をテスト用の狭い型へ bridge する。
    window.__realWsOnmessage = socket.onmessage as unknown as (ev: { data: string }) => void;
    socket.onmessage = function() {};
    window.__dispatchWsMessage = (payload) => {
      const onmessage = window.__realWsOnmessage;
      if (!onmessage) {
        throw new Error('WebSocket dispatch harness is not ready');
      }
      onmessage({ data: JSON.stringify(payload) });
    };
  });
}
```

`test.beforeEach` 内の init script は次にする。

```ts
await page.addInitScript(installTestWebSocketHarness, { shortenReconnectDelay: true });
```

- [ ] **Step 4: focused typecheck を実行する**

Run: `npm run typecheck`

Expected: `document_search.spec.ts` と `text_selection_defer.spec.ts` の duplicate global は解消済み。残る失敗は他 spec の global 宣言または strict flag 起因に限定される。

- [ ] **Step 5: コミットする**

```bash
git add tests/e2e/browser/test-websocket.ts tests/e2e/document_search.spec.ts tests/e2e/text_selection_defer.spec.ts
git commit -m "test: E2E WebSocketハーネスを共通化"
```

---

### Task 3: 残り spec の global 宣言と strict flag エラーを解消する

**Files:**
- Modify: `tests/e2e/memo_jump.spec.ts`
- Modify: `tests/e2e/update_content_exposure.spec.ts`
- Modify: `tests/e2e/markdown_links.spec.ts`

- [ ] **Step 1: `memo_jump.spec.ts` の `declare global` を削除する**

削除対象:

```ts
declare global {
  interface Window {
    __MV_E2E__?: boolean;
    updateContent: (data: { content: string; toc: string }, opts?: Record<string, unknown>) => void;
    scheduleBufferedLiveUpdate: (data: { content: string; toc: string }) => void;
  }
  function augmentHashWithTrailingLineHint(link: HTMLAnchorElement, hash: string): string;
}
```

契約違反テストの cast は `UpdateContentPayload` へ置き換える。

```ts
window.updateContent({ toc } as unknown as UpdateContentPayload, {});
window.updateContent({ content } as unknown as UpdateContentPayload, {});
window.updateContent({} as unknown as UpdateContentPayload, {});
window.updateContent({ content: null, toc } as unknown as UpdateContentPayload, {});
window.updateContent({ content, toc: null } as unknown as UpdateContentPayload, {});
window.updateContent({ content: 123, toc } as unknown as UpdateContentPayload, {});
window.updateContent(null as unknown as UpdateContentPayload, {});
window.updateContent(undefined as unknown as UpdateContentPayload, {});
window.scheduleBufferedLiveUpdate({} as unknown as UpdateContentPayload);
```

- [ ] **Step 2: `update_content_exposure.spec.ts` の `declare global` を削除する**

削除対象:

```ts
declare global {
  interface Window {
    __MV_E2E__?: boolean;
    updateContent: (data: { content: string; toc: string }, opts?: Record<string, unknown>) => void;
  }
}

export {};
```

- [ ] **Step 3: `markdown_links.spec.ts` の `declare global` を削除し helper を追加する**

削除対象:

```ts
declare global {
  interface Window {
    __clickObservations: Record<string, { defaultPrevented: boolean }>;
  }
}
```

`installClickObserver(page)` の後に追加:

```ts
function requireClickObservation(
  results: Record<string, ClickObservation> | undefined,
  href: string
): ClickObservation {
  const observation = results?.[href];
  if (!observation) {
    throw new Error(`click observation not found: ${href}`);
  }
  return observation;
}
```

direct indexed assertions を置き換える。

```ts
expect(requireClickObservation(results, 'https://example.com/foo.md').defaultPrevented).toBe(false);
expect(requireClickObservation(results, 'mailto:foo@example.com').defaultPrevented).toBe(false);
expect(requireClickObservation(results, 'report.pdf').defaultPrevented).toBe(false);
```

- [ ] **Step 4: typecheck と対象 E2E を通す**

Run:

```bash
npm run typecheck
npx playwright test tests/e2e/document_search.spec.ts tests/e2e/text_selection_defer.spec.ts tests/e2e/memo_jump.spec.ts tests/e2e/update_content_exposure.spec.ts tests/e2e/markdown_links.spec.ts
```

Expected: `tsc --noEmit` と対象 Playwright tests が pass。

- [ ] **Step 5: コミットする**

```bash
git add tests/e2e/memo_jump.spec.ts tests/e2e/update_content_exposure.spec.ts tests/e2e/markdown_links.spec.ts
git commit -m "test: E2Eグローバル型の重複を解消"
```

---

### Task 4: BACKLOG を更新して全体検証する

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Verify: `tests/e2e/**/*.ts`
- Verify: `tsconfig.json`

- [ ] **Step 1: BACKLOG の対象 5 項目を完了にする**

`docs/todo/BACKLOG.md` で次を `- [x]` にする。

```markdown
- [x] E2E の `declare global` ブロックを `tests/e2e/globals.d.ts` に集約
- [x] `updateContent` 型宣言の統一
- [x] `tsconfig.json` に strict flag 追加
- [x] `TestWebSocket` を `tests/e2e/browser/test-websocket.ts` に抽出
- [x] `as unknown as` double-cast の説明コメント追加
```

- [ ] **Step 2: BACKLOG 件数を確認する**

Run:

```bash
grep -c "^- \[ \]" docs/todo/BACKLOG.md
grep -c "^- \[x\]" docs/todo/BACKLOG.md
```

Expected:

```text
12
6
```

- [ ] **Step 3: 必須検証を再実行する**

Run:

```bash
npm run typecheck
npx playwright test tests/e2e/document_search.spec.ts tests/e2e/text_selection_defer.spec.ts tests/e2e/memo_jump.spec.ts tests/e2e/update_content_exposure.spec.ts tests/e2e/markdown_links.spec.ts
```

Expected: `tsc --noEmit` と対象 Playwright tests が pass。

- [ ] **Step 4: 可能なら full verification を実行する**

Run: `./verify.sh`

Expected: format, clippy, Rust tests, configured checks が pass。

- [ ] **Step 5: 差分が設計範囲内であることを確認する**

Run:

```bash
git diff --stat
git status --short
```

Expected changed files are limited to `docs/todo/BACKLOG.md`, `tests/e2e/browser/test-websocket.ts`, `tests/e2e/globals.d.ts`, 対象 5 spec, and `tsconfig.json`.

- [ ] **Step 6: コミットする**

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: E2E型ハーネス整理のbacklogを完了"
```

---

## セキュリティ確認

- E2E ハーネスは `page.addInitScript()` から注入されるテストコードに閉じる。
- `window.updateContent` の production expose 条件は変更しない。
- Host/Origin 検証、HTML sanitization、CSP、path validation には触れない。
- BACKLOG の外部レビュー由来テキストは、現在の脆弱性の断定として扱わない。

## 完了条件

- `npm run typecheck` が strict flag 追加後に通る。
- 対象 Playwright spec が通る。
- 可能なら `./verify.sh` が通る。
- BACKLOG の対象 5 項目が `[x]` になっている。
- production コードに差分がない。
