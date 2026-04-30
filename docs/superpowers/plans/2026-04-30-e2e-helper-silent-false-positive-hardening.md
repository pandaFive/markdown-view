# E2E Helper Silent False-Positive Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Narrow silent false-positive paths in shared Playwright E2E helpers for text selection, fixture cleanup, and WebSocket dispatch.

**Architecture:** Keep the existing flat `tests/e2e/helpers.ts` module. Add contract-focused E2E coverage in `tests/e2e/helpers.spec.ts`, then harden helper defaults and adjust only existing specs that need explicit looser matching. Production Rust and browser bundle code stay unchanged.

**Tech Stack:** TypeScript, Playwright, Node `fs/promises`, existing E2E globals in `tests/e2e/globals.d.ts`.

---

## File Structure

- Create `tests/e2e/helpers.spec.ts`: contract tests for shared helper behavior. It owns helper-only regression coverage and should not test production feature behavior beyond what is needed to exercise helpers.
- Modify `tests/e2e/helpers.ts`: add selection match options, cleanup residual checks, stale WebSocket bridge detection, and shared dispatch precondition checks.
- Modify `tests/e2e/globals.d.ts`: add `window.__bridgedWs` for the WebSocket bridge freshness contract.
- Modify `tests/e2e/memo_jump.spec.ts`: switch the intentional substring selection to `match: 'contains'`.
- Modify `docs/todo/BACKLOG.md`: move the completed backlog item to Done after implementation and verification.
- Inspect but avoid changing unless tests require it: `tests/e2e/memo_quote.spec.ts`, `tests/e2e/text_selection_defer.spec.ts`, `tests/e2e/document_search.spec.ts`, `tests/e2e/browser/test-websocket.ts`.

## Task 0: Prepare a Non-Develop Work Branch

**Files:**
- Inspect: git branch state

- [ ] **Step 1: Confirm the current branch**

Run:

```bash
git status --short --branch
```

Expected: working tree is clean except for this plan if it has not been committed yet. Do not start implementation commits directly on `develop` or `main`.

- [ ] **Step 2: Create the implementation branch**

If the current branch is `develop`, run:

```bash
git checkout -b fix/e2e-helper-silent-false-positive-hardening
```

Expected: `git status --short --branch` shows `## fix/e2e-helper-silent-false-positive-hardening`.

## Task 1: Add Failing Helper Contract Tests

**Files:**
- Create: `tests/e2e/helpers.spec.ts`
- Test: `tests/e2e/helpers.spec.ts`

- [ ] **Step 1: Create `tests/e2e/helpers.spec.ts` with contract tests**

Create the file with this content:

```ts
import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect } from '@playwright/test';
import { installTestWebSocketHarness } from './browser/test-websocket';
import {
  dispatchWsMessage,
  resetStandardFixtures,
  selectParagraphText,
  stabilizeWebSocketHarness,
  updateContent
} from './helpers';

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');
const readmePath = path.join(fixtureDir, 'README.md');

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.__MV_E2E__ = true;
  });
  await resetStandardFixtures();
  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
});

test.afterEach(async () => {
  await resetStandardFixtures();
});

test('selectParagraphTextは完全一致の一意候補だけを選択する', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');

  const selectedText = await page.evaluate(() => window.getSelection()?.toString() ?? '');
  expect(selectedText).toBe('Initial README content');
});

test('selectParagraphTextは既定で部分一致を採用しない', async ({ page }) => {
  await expect(selectParagraphText(page, 'README content')).rejects.toThrow(/text not found: README content/);
});

test('selectParagraphTextは明示された部分一致なら選択できる', async ({ page }) => {
  await selectParagraphText(page, 'README content', { match: 'contains' });

  const selectedText = await page.evaluate(() => window.getSelection()?.toString() ?? '');
  expect(selectedText).toBe('Initial README content');
});

test('selectParagraphTextは複数候補の部分一致を曖昧として失敗させる', async ({ page }) => {
  await updateContent(page, {
    content: '<h1 id="readme">README</h1><p>duplicate target alpha</p><p>duplicate target beta</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await expect(selectParagraphText(page, 'duplicate target', { match: 'contains' }))
    .rejects.toThrow(/ambiguous text match: duplicate target \(2 matches\)/);
});

test('resetStandardFixturesはmemo artifactと設定ディレクトリを削除する', async () => {
  await fs.writeFile(path.join(fixtureDir, '.README.md.memo.md'), 'stale memo');
  await fs.mkdir(path.join(fixtureDir, '.markdown-view'), { recursive: true });
  await fs.writeFile(path.join(fixtureDir, '.markdown-view', 'state.json'), '{}');

  await resetStandardFixtures();

  await expect(fs.access(path.join(fixtureDir, '.README.md.memo.md'))).rejects.toThrow();
  await expect(fs.access(path.join(fixtureDir, '.markdown-view'))).rejects.toThrow();
});

test('WebSocket dispatchはstale bridgeを失敗させる', async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.reload();
  await stabilizeWebSocketHarness(page);

  await page.evaluate(() => {
    const NativeWebSocket = Object.getPrototypeOf(window.__lastWs!).constructor as typeof WebSocket;
    window.__lastWs = {
      onmessage: function() {},
      close: function() {},
      send: function() {},
      readyState: NativeWebSocket.OPEN
    } as unknown as MvE2E.TestWebSocketInstance;
  });

  await expect(dispatchWsMessage(page, {
    content: '<h1 id="readme">README</h1><p>stale update</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  })).rejects.toThrow(/WebSocket test harness bridge is stale/);
});
```

- [ ] **Step 2: Run the new spec and verify the helper contract tests fail**

Run:

```bash
npx playwright test tests/e2e/helpers.spec.ts
```

Expected: FAIL. The failures should include TypeScript/runtime issues showing `selectParagraphText` does not accept the third argument yet and stale WebSocket bridge is not detected yet.

- [ ] **Step 3: Commit the failing tests**

```bash
git add tests/e2e/helpers.spec.ts
git commit -m "test: E2E helper契約の回帰テストを追加"
```

## Task 2: Harden `selectParagraphText`

**Files:**
- Modify: `tests/e2e/helpers.ts`
- Modify: `tests/e2e/memo_jump.spec.ts`
- Test: `tests/e2e/helpers.spec.ts`, `tests/e2e/memo_quote.spec.ts`, `tests/e2e/memo_jump.spec.ts`

- [ ] **Step 1: Add selection match options and fail-fast candidate handling**

In `tests/e2e/helpers.ts`, replace the current `selectParagraphText` function with this code and add the exported type just above it:

```ts
export type SelectParagraphTextOptions = {
  match?: 'exact' | 'contains';
};

export async function selectParagraphText(
  page: Page,
  text: string,
  options: SelectParagraphTextOptions = {}
) {
  await page.evaluate(({ targetText, match }) => {
    const content = document.getElementById('content');
    if (!content) {
      throw new Error('content root not found');
    }

    const walker = document.createTreeWalker(content, NodeFilter.SHOW_TEXT);
    const matches: Text[] = [];
    let node: Node | null = null;
    while ((node = walker.nextNode())) {
      const nodeText = node.textContent;
      if (!nodeText) {
        continue;
      }
      const isMatch = match === 'contains' ? nodeText.includes(targetText) : nodeText === targetText;
      if (isMatch) {
        matches.push(node as Text);
      }
    }

    if (matches.length === 0) {
      throw new Error(`text not found: ${targetText}`);
    }
    if (matches.length > 1) {
      throw new Error(`ambiguous text match: ${targetText} (${matches.length} matches)`);
    }

    const matchedNode = matches[0];
    if (!matchedNode) {
      throw new Error(`text not found: ${targetText}`);
    }
    const parent = matchedNode.parentElement;
    if (!parent) {
      throw new Error(`text match has no parent element: ${targetText}`);
    }

    const selection = window.getSelection();
    if (!selection) {
      throw new Error('window selection is not available');
    }
    const range = document.createRange();
    range.selectNodeContents(parent);
    selection.removeAllRanges();
    selection.addRange(range);
  }, { targetText: text, match: options.match ?? 'exact' });
}
```

- [ ] **Step 2: Make the intentional substring selection explicit**

In `tests/e2e/memo_jump.spec.ts`, replace:

```ts
  await selectParagraphText(page, 'TARGET BLOCK');
```

with:

```ts
  await selectParagraphText(page, 'TARGET BLOCK', { match: 'contains' });
```

- [ ] **Step 3: Run selection-focused tests**

Run:

```bash
npx playwright test tests/e2e/helpers.spec.ts tests/e2e/memo_quote.spec.ts tests/e2e/memo_jump.spec.ts
```

Expected: `selectParagraphText` tests pass. The WebSocket stale bridge test may still fail until Task 4; if Playwright stops at that failure, rerun after Task 4 before committing the full helper hardening.

- [ ] **Step 4: Commit selection hardening**

```bash
git add tests/e2e/helpers.ts tests/e2e/memo_jump.spec.ts
git commit -m "test: E2E本文選択helperを一意一致にする"
```

## Task 3: Harden Fixture Cleanup

**Files:**
- Modify: `tests/e2e/helpers.ts`
- Test: `tests/e2e/helpers.spec.ts`, `tests/e2e/memo_sync.spec.ts`, `tests/e2e/markdown_links.spec.ts`

- [ ] **Step 1: Add cleanup residual assertion helpers**

In `tests/e2e/helpers.ts`, add these helper functions after `notesPath`:

```ts
async function memoArtifactPaths() {
  const entries = await fs.readdir(fixtureDir, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && entry.name.endsWith('.memo.md'))
    .map((entry) => path.join(fixtureDir, entry.name));
}

async function assertMemoArtifactsRemoved() {
  const leftovers = await memoArtifactPaths();
  const markdownViewPath = path.join(fixtureDir, '.markdown-view');
  try {
    await fs.access(markdownViewPath);
    leftovers.push(markdownViewPath);
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code !== 'ENOENT') {
      throw error;
    }
  }

  if (leftovers.length > 0) {
    const relativeLeftovers = leftovers.map((entry) => path.relative(fixtureDir, entry)).join(', ');
    throw new Error(`fixture cleanup left stale artifacts: ${relativeLeftovers}`);
  }
}
```

- [ ] **Step 2: Use the helper functions inside `resetStandardFixtures`**

In `tests/e2e/helpers.ts`, replace the cleanup block in `resetStandardFixtures`:

```ts
    const entries = await fs.readdir(fixtureDir, { withFileTypes: true });
    await Promise.all(entries
      .filter((entry) => entry.isFile() && entry.name.endsWith('.memo.md'))
      .map((entry) => fs.rm(path.join(fixtureDir, entry.name), { force: true })));
    await fs.rm(path.join(fixtureDir, '.markdown-view'), { recursive: true, force: true });
```

with:

```ts
    await Promise.all(
      (await memoArtifactPaths()).map((memoPath) => fs.rm(memoPath, { force: true }))
    );
    await fs.rm(path.join(fixtureDir, '.markdown-view'), { recursive: true, force: true });
    await assertMemoArtifactsRemoved();
```

- [ ] **Step 3: Run fixture cleanup tests**

Run:

```bash
npx playwright test tests/e2e/helpers.spec.ts tests/e2e/memo_sync.spec.ts tests/e2e/markdown_links.spec.ts
```

Expected: fixture cleanup test passes. Existing memo sync and markdown links tests pass and do not inherit stale memo artifacts.

- [ ] **Step 4: Commit fixture cleanup hardening**

```bash
git add tests/e2e/helpers.ts
git commit -m "test: E2E fixture cleanupの残存検知を追加"
```

## Task 4: Harden WebSocket Bridge Freshness

**Files:**
- Modify: `tests/e2e/globals.d.ts`
- Modify: `tests/e2e/helpers.ts`
- Test: `tests/e2e/helpers.spec.ts`, `tests/e2e/text_selection_defer.spec.ts`, `tests/e2e/document_search.spec.ts`

- [ ] **Step 1: Add the bridged WebSocket global type**

In `tests/e2e/globals.d.ts`, add this property to `interface Window` beside `__lastWs` and `__realWsOnmessage`:

```ts
    __bridgedWs?: MvE2E.TestWebSocketInstance;
```

- [ ] **Step 2: Record and validate the bridge target during stabilize**

In `stabilizeWebSocketHarness`, replace the `page.evaluate` body with this version:

```ts
  await page.evaluate(() => {
    const lastWs = window.__lastWs;
    if (!lastWs || typeof lastWs.onmessage !== 'function') {
      throw new Error('WebSocket test harness is not initialized');
    }
    if (window.__bridgedWs && window.__bridgedWs !== lastWs) {
      throw new Error('WebSocket test harness bridge is stale; call stabilizeWebSocketHarness after reconnect');
    }
    // __dispatchWsMessage は MessageEvent を生成せず { data: string } を直接渡すため、
    // E2E ハーネス内では onmessage の契約をテスト用の狭い型へ bridge する。
    const assertFreshBridge = () => {
      if (window.__bridgedWs !== window.__lastWs) {
        throw new Error('WebSocket test harness bridge is stale; call stabilizeWebSocketHarness after reconnect');
      }
    };
    window.__bridgedWs = lastWs;
    window.__realWsOnmessage = lastWs.onmessage as unknown as (ev: { data: string }) => void;
    lastWs.onmessage = function() {};
    window.__dispatchWsMessage = (payload) => {
      assertFreshBridge();
      const realWsOnmessage = window.__realWsOnmessage;
      if (!realWsOnmessage) {
        throw new Error('WebSocket test harness message handler is not initialized');
      }
      realWsOnmessage({ data: JSON.stringify(payload) });
    };
  });
```

- [ ] **Step 3: Guard single-message dispatch**

In `dispatchWsMessage`, replace the `page.evaluate` body with:

```ts
  await page.evaluate((messagePayload) => {
    if (window.__bridgedWs !== window.__lastWs) {
      throw new Error('WebSocket test harness bridge is stale; call stabilizeWebSocketHarness after reconnect');
    }
    const dispatchMessage = window.__dispatchWsMessage;
    if (!dispatchMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchMessage(messagePayload);
  }, payload);
```

- [ ] **Step 4: Guard disable-real-handler dispatch**

In `dispatchWsMessageAndDisableRealHandler`, replace the `page.evaluate` body with:

```ts
  await page.evaluate((messagePayload) => {
    if (window.__bridgedWs !== window.__lastWs) {
      throw new Error('WebSocket test harness bridge is stale; call stabilizeWebSocketHarness after reconnect');
    }
    const dispatchMessage = window.__dispatchWsMessage;
    if (!dispatchMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    const lastWs = window.__lastWs;
    if (!lastWs) {
      throw new Error('WebSocket test harness is not initialized');
    }
    dispatchMessage(messagePayload);
    // watcher経由の実WSメッセージがpendingUpdateを上書きしないよう、
    // 偽メッセージ送信後にonmessageを無効化する。
    lastWs.onmessage = function() {};
  }, payload);
```

- [ ] **Step 5: Guard multi-message dispatch**

In `dispatchWsMessages`, replace the `page.evaluate` body with:

```ts
  await page.evaluate((messagePayloads) => {
    if (window.__bridgedWs !== window.__lastWs) {
      throw new Error('WebSocket test harness bridge is stale; call stabilizeWebSocketHarness after reconnect');
    }
    const dispatchMessage = window.__dispatchWsMessage;
    if (!dispatchMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    for (const [index, payload] of messagePayloads.entries()) {
      try {
        dispatchMessage(payload);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        throw new Error(`WebSocket test harness dispatcher failed at payload ${index}: ${message}`);
      }
    }
  }, payloads);
```

- [ ] **Step 6: Run WebSocket-focused tests**

Run:

```bash
npx playwright test tests/e2e/helpers.spec.ts tests/e2e/text_selection_defer.spec.ts tests/e2e/document_search.spec.ts
```

Expected: all tests pass.

- [ ] **Step 7: Commit WebSocket bridge hardening**

```bash
git add tests/e2e/helpers.ts tests/e2e/globals.d.ts
git commit -m "test: E2E WebSocket helperのstale bridgeを検知"
```

## Task 5: Update Backlog and Run Full Verification

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Test: `npm run typecheck`, targeted Playwright specs, `./verify.sh`

- [ ] **Step 1: Move the backlog item to Done**

In `docs/todo/BACKLOG.md`, remove the unchecked P2 item whose heading is:

```md
- [ ] E2E 共通ヘルパーの silent false-positive 経路を狭める
```

Add this item near the top of the `Done` section:

```md
- [x] E2E 共通ヘルパーの silent false-positive 経路を狭める
  - ファイル: `tests/e2e/helpers.ts`, `tests/e2e/helpers.spec.ts`, `tests/e2e/globals.d.ts`, `tests/e2e/memo_jump.spec.ts`
  - 内容: `selectParagraphText` を既定で完全一致かつ一意一致にし、部分一致を明示オプションへ移した。fixture cleanup は memo artifact と `.markdown-view` の削除後残存を検知し、WebSocket dispatch helper は stale bridge を fail-fast にした
  - 完了根拠: `npm run typecheck`、`npx playwright test tests/e2e/helpers.spec.ts tests/e2e/memo_quote.spec.ts tests/e2e/memo_jump.spec.ts tests/e2e/text_selection_defer.spec.ts tests/e2e/document_search.spec.ts tests/e2e/memo_sync.spec.ts tests/e2e/markdown_links.spec.ts`、`./verify.sh` が pass
  - 由来: E2E 共通ヘルパー抽出 PR レビュー (2026-04-29、pre-existing)
```

- [ ] **Step 2: Run TypeScript typecheck**

Run:

```bash
npm run typecheck
```

Expected: PASS.

- [ ] **Step 3: Run targeted Playwright coverage**

Run:

```bash
npx playwright test tests/e2e/helpers.spec.ts tests/e2e/memo_quote.spec.ts tests/e2e/memo_jump.spec.ts tests/e2e/text_selection_defer.spec.ts tests/e2e/document_search.spec.ts tests/e2e/memo_sync.spec.ts tests/e2e/markdown_links.spec.ts
```

Expected: PASS.

- [ ] **Step 4: Run repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 5: Commit backlog update**

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: E2E helper hardening完了を記録"
```

## Task 6: Final Review Before PR

**Files:**
- Inspect: `tests/e2e/helpers.ts`
- Inspect: `tests/e2e/helpers.spec.ts`
- Inspect: `tests/e2e/globals.d.ts`
- Inspect: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Review the final diff**

Run:

```bash
git diff origin/develop...HEAD -- tests/e2e/helpers.ts tests/e2e/helpers.spec.ts tests/e2e/globals.d.ts tests/e2e/memo_jump.spec.ts docs/todo/BACKLOG.md
```

Expected: diff only contains E2E helper hardening, new helper contract tests, the one explicit substring selection call, and backlog completion.

- [ ] **Step 2: Confirm no production files changed**

Run:

```bash
git diff --name-only origin/develop...HEAD
```

Expected: output is limited to docs and `tests/e2e` files. No `src/` files should appear.

- [ ] **Step 3: Record verification commands for the completion report**

Use this verification summary in the final report:

```md
Verification:
- `npm run typecheck`: PASS
- `npx playwright test tests/e2e/helpers.spec.ts tests/e2e/memo_quote.spec.ts tests/e2e/memo_jump.spec.ts tests/e2e/text_selection_defer.spec.ts tests/e2e/document_search.spec.ts tests/e2e/memo_sync.spec.ts tests/e2e/markdown_links.spec.ts`: PASS
- `./verify.sh`: PASS
```

- [ ] **Step 4: Report residual risk**

Use this residual risk statement unless verification finds a more specific issue:

```md
Residual risk: WebSocket reconnect behavior is guarded at the test harness bridge boundary, not by simulating every browser reconnect timing. Production WebSocket behavior is unchanged.
```
