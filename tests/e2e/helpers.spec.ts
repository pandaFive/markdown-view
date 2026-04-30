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
