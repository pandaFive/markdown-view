import { test, expect } from '@playwright/test';

test('production実行では内部APIを公開しない', async ({ page }) => {
  await page.goto('/');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).updateContent)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).selectFile)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).appContext)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).startMarkdownViewApp)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).connectWS)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).applyDocumentSearchQuery)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).markPendingTocNavigation)).resolves.toBe('undefined');
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

test('E2Eフラグがtruthy非booleanならテストフックを公開しない', async ({ page }) => {
  await page.addInitScript(() => {
    // expose 条件が __MV_E2E__ === true に固定されていることを確認するため、意図的に型を破る。
    window.__MV_E2E__ = 'true' as unknown as boolean;
  });
  await page.goto('/');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).updateContent)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('undefined');
});
