import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

const tsAssetDir = path.join(__dirname, '..', '..', 'src', 'template', 'assets', 'ts');
function extractInternalGlobalNames(source: string) {
  return Array.from(source.matchAll(/^(?:(?:async\s+)?function\*?\s+|class\s+|(?:var|let|const)\s+)([A-Za-z_$][\w$]*)/gm))
    .map((match) => match[1])
    .filter((name): name is string => typeof name === 'string');
}

const internalGlobalNames = Array.from(new Set(
  fs.readdirSync(tsAssetDir)
    .filter((fileName) => fileName.endsWith('.ts') && !fileName.endsWith('.d.ts'))
    .flatMap((fileName) => {
      const source = fs.readFileSync(path.join(tsAssetDir, fileName), 'utf8');
      return extractInternalGlobalNames(source);
    })
)).sort();

test('内部グローバル名抽出は実ファイルから十分な宣言数を拾う', async () => {
  expect(internalGlobalNames.length).toBeGreaterThan(20);
  expect(internalGlobalNames).toContain('startMarkdownViewApp');
  expect(internalGlobalNames).toContain('createContentController');
  expect(internalGlobalNames).toContain('createContentEnhancements');
  expect(internalGlobalNames).toContain('createContentNavigation');
  expect(internalGlobalNames).toContain('createDocumentSearchController');
  expect(internalGlobalNames).toContain('createDirectorySearchController');
  expect(internalGlobalNames).toContain('createWebSocketController');
  expect(internalGlobalNames).toContain('validateUpdatePayload');
  expect(internalGlobalNames).toContain('applyValidatedUpdateHtml');
});

async function ownWindowPropertyNames(page: import('@playwright/test').Page) {
  return page.evaluate(() => Object.getOwnPropertyNames(window).sort());
}

test('内部グローバル名抽出は将来のトップレベル構文も対象にする', async () => {
  expect(extractInternalGlobalNames([
    'class FutureController {}',
    'async function loadFuture() {}',
    'function* iterateFuture() {}',
    'const existingConst = 1;',
    'let existingLet = 1;',
    'var existingVar = 1;',
    'function existingFunction() {}'
  ].join('\n'))).toEqual([
    'FutureController',
    'loadFuture',
    'iterateFuture',
    'existingConst',
    'existingLet',
    'existingVar',
    'existingFunction'
  ]);
});

test('production実行では内部APIを公開しない', async ({ page }) => {
  await page.route('**/__window-baseline', async (route) => {
    await route.fulfill({
      contentType: 'text/html',
      body: '<!doctype html><meta charset="utf-8"><title>baseline</title>'
    });
  });
  await page.goto('/__window-baseline');
  const baseline = new Set(await ownWindowPropertyNames(page));
  await page.unroute('**/__window-baseline');

  await page.goto('/');
  const exposed = await page.evaluate((names) => {
    const win = window as unknown as Record<string, unknown>;
    return names.filter((name) => typeof win[name] !== 'undefined');
  }, internalGlobalNames);
  expect(exposed).toEqual([]);
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).createContentController)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).createDocumentSearchController)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).createDirectorySearchController)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).appContext)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).validateUpdatePayload)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).applyValidatedUpdateHtml)).resolves.toBe('undefined');
  const addedWindowProps = (await ownWindowPropertyNames(page)).filter((name) => !baseline.has(name));
  expect(addedWindowProps).toEqual([]);
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
  const hookState = await page.evaluate(() => {
    const hooks = window.markdownViewTestHooks;
    return {
      isDirModeType: typeof hooks.isDirMode,
      currentFileType: typeof hooks.currentFile,
      lastAppliedContentType: hooks.lastAppliedContent === null ? 'null' : typeof hooks.lastAppliedContent
    };
  });
  expect(hookState).toEqual({
    isDirModeType: 'boolean',
    currentFileType: 'string',
    lastAppliedContentType: 'null'
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
