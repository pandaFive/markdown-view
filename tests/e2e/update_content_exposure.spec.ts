import { test, expect } from '@playwright/test';

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

test('E2Eフラグがtruthy非booleanならwindow.updateContentを公開しない', async ({ page }) => {
  await page.addInitScript(() => {
    window.__MV_E2E__ = 'true' as unknown as boolean;
  });
  await page.goto('/');
  const exposedType = await page.evaluate(() => typeof window.updateContent);
  expect(exposedType).toBe('undefined');
});
