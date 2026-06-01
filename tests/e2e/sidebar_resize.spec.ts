import { test, expect } from '@playwright/test';
import { resetStandardFixtures } from './helpers';

test.beforeEach(async ({ page }) => {
  await resetStandardFixtures();
  await page.goto('/');
  await expect(page.locator('#sidebar')).toBeVisible();
});

test.afterEach(async () => {
  await resetStandardFixtures();
});

test('サイドバー幅はドラッグで伸び縮みできる', async ({ page }) => {
  const sidebar = page.locator('#sidebar');
  const handle = page.locator('#sidebar-width-resizer');
  const initialBox = await sidebar.boundingBox();
  const handleBox = await handle.boundingBox();

  expect(initialBox).not.toBeNull();
  expect(handleBox).not.toBeNull();

  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y + 80);
  await page.mouse.down();
  await page.mouse.move(initialBox!.width + 90, handleBox!.y + 80);
  await page.mouse.up();

  await expect.poll(async () => (await sidebar.boundingBox())?.width ?? 0).toBeGreaterThan(initialBox!.width + 60);

  const expandedBox = await sidebar.boundingBox();
  const expandedHandleBox = await handle.boundingBox();
  expect(expandedHandleBox).not.toBeNull();
  await page.mouse.move(expandedHandleBox!.x + expandedHandleBox!.width / 2, expandedHandleBox!.y + 80);
  await page.mouse.down();
  await page.mouse.move(initialBox!.width - 40, handleBox!.y + 80);
  await page.mouse.up();

  await expect.poll(async () => (await sidebar.boundingBox())?.width ?? 0).toBeLessThan(expandedBox!.width - 60);
});

test('サイドバー内部コンテンツエリアはドラッグで伸び縮みできる', async ({ page }) => {
  const panel = page.locator('#panel-files');
  const content = panel.locator('.sidebar-resizable-content');
  const handle = panel.locator('.sidebar-content-resizer');
  const panelBox = await panel.boundingBox();
  const handleBox = await handle.boundingBox();

  expect(panelBox).not.toBeNull();
  expect(handleBox).not.toBeNull();

  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y + handleBox!.height / 2);
  await page.mouse.down();
  await page.mouse.move(handleBox!.x + handleBox!.width / 2, panelBox!.y + 180);
  await page.mouse.up();

  await expect.poll(async () => (await content.boundingBox())?.height ?? 0).toBeLessThan(220);

  const shrunkenBox = await content.boundingBox();
  const shrunkenHandleBox = await handle.boundingBox();
  expect(shrunkenHandleBox).not.toBeNull();
  await page.mouse.move(shrunkenHandleBox!.x + shrunkenHandleBox!.width / 2, shrunkenHandleBox!.y + shrunkenHandleBox!.height / 2);
  await page.mouse.down();
  await page.mouse.move(shrunkenHandleBox!.x + shrunkenHandleBox!.width / 2, panelBox!.y + 260);
  await page.mouse.up();

  await expect.poll(async () => (await content.boundingBox())?.height ?? 0).toBeGreaterThan((shrunkenBox?.height ?? 0) + 50);
});
