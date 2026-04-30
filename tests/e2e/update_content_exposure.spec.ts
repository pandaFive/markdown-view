import { test, expect } from '@playwright/test';

const internalGlobalNames = [
  'appContext',
  'startMarkdownViewApp',
  'createAppContext',
  'connectWS',
  'createWebSocketController',
  'selectFile',
  'updateContent',
  'setLiveStatus',
  'updateDocumentStats',
  'updateReadingProgress',
  'syncDocumentChrome',
  'applyDocumentSearchQuery',
  'moveDocumentSearch',
  'markPendingTocNavigation',
  'augmentHashWithTrailingLineHint',
  'scheduleBufferedLiveUpdate',
  'setupSelectionDeferral',
  'setupDocumentSearch',
  'setupContentLinkNavigation',
  'setupMemoLinkNavigation',
  'setupMemoInteractions',
  'setupSidebarInteractions',
  'setupThemeToggle',
  'setupTocTracking'
];

test('production実行では内部APIを公開しない', async ({ page }) => {
  await page.goto('/');
  const exposed = await page.evaluate((names) => {
    const win = window as unknown as Record<string, unknown>;
    return names.filter((name) => typeof win[name] !== 'undefined');
  }, internalGlobalNames);
  expect(exposed).toEqual([]);
  await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('undefined');
});

test('E2Eフラグがtrueなら単一テストフックだけ公開する', async ({ page }) => {
  await page.addInitScript(() => {
    window.__MV_E2E__ = true;
  });
  await page.goto('/');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).updateContent)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('object');
  await expect(page.evaluate(() => typeof window.markdownViewTestHooks.selectFile)).resolves.toBe('function');
  await expect(page.evaluate(() => typeof window.markdownViewTestHooks.updateContent)).resolves.toBe('function');
});

for (const flagValue of ['true', 1, {}, [], '1']) {
  test(`E2Eフラグがboolean true以外ならテストフックを公開しない: ${JSON.stringify(flagValue)}`, async ({ page }) => {
    await page.addInitScript((value) => {
      window.__MV_E2E__ = value as unknown as boolean;
    }, flagValue);
    await page.goto('/');
    await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).updateContent)).resolves.toBe('undefined');
    await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('undefined');
  });
}

test('E2Eフラグがfalseならテストフックを公開しない', async ({ page }) => {
  await page.addInitScript(() => {
    window.__MV_E2E__ = false;
  });
  await page.goto('/');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).updateContent)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('undefined');
});
