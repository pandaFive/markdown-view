import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

const jsAssetDir = path.join(__dirname, '..', '..', 'src', 'template', 'assets', 'js');
const internalGlobalNames = Array.from(new Set(
  fs.readdirSync(jsAssetDir)
    .filter((fileName) => fileName.endsWith('.js'))
    .flatMap((fileName) => {
      const source = fs.readFileSync(path.join(jsAssetDir, fileName), 'utf8');
      return Array.from(source.matchAll(/^(?:function|var|let|const)\s+([A-Za-z_$][\w$]*)/gm))
        .map((match) => match[1])
        .filter((name): name is string => typeof name === 'string');
    })
)).sort();

async function ownWindowPropertyNames(page: import('@playwright/test').Page) {
  return page.evaluate(() => Object.getOwnPropertyNames(window).sort());
}

test('production実行では内部APIを公開しない', async ({ page }) => {
  await page.goto('about:blank');
  const baseline = new Set(await ownWindowPropertyNames(page));
  await page.goto('/');
  const exposed = await page.evaluate((names) => {
    const win = window as unknown as Record<string, unknown>;
    return names.filter((name) => typeof win[name] !== 'undefined');
  }, internalGlobalNames);
  expect(exposed).toEqual([]);
  const addedWindowProps = (await ownWindowPropertyNames(page)).filter((name) => !baseline.has(name));
  const leakedInternalProps = addedWindowProps.filter((name) => internalGlobalNames.includes(name));
  expect(leakedInternalProps).toEqual([]);
  await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('undefined');
});

test('E2Eフラグがtrueなら単一テストフックだけ公開する', async ({ page }) => {
  await page.addInitScript(() => {
    window.__MV_E2E__ = true;
  });
  await page.goto('/');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).updateContent)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('object');
  const hookTypes = await page.evaluate(() => {
    const hooks = window.markdownViewTestHooks;
    return Object.fromEntries(
      [
        'activateSidebarTab',
        'applyDocumentSearchQuery',
        'augmentHashWithTrailingLineHint',
        'markPendingTocNavigation',
        'moveDocumentSearch',
        'scheduleBufferedLiveUpdate',
        'selectFile',
        'setCurrentFileForTest',
        'setDirModeForTest',
        'setMarkPendingTocNavigationObserverForTest',
        'updateContent'
      ].map((name) => [name, typeof hooks[name as keyof typeof hooks]])
    );
  });
  expect(hookTypes).toEqual({
    activateSidebarTab: 'function',
    applyDocumentSearchQuery: 'function',
    augmentHashWithTrailingLineHint: 'function',
    markPendingTocNavigation: 'function',
    moveDocumentSearch: 'function',
    scheduleBufferedLiveUpdate: 'function',
    selectFile: 'function',
    setCurrentFileForTest: 'function',
    setDirModeForTest: 'function',
    setMarkPendingTocNavigationObserverForTest: 'function',
    updateContent: 'function'
  });
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
