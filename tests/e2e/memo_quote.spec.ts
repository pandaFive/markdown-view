import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect } from '@playwright/test';
import { openMemoTab, resetStandardFixtures, selectParagraphText } from './helpers';

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');

test.beforeEach(async ({ page }) => {
  await resetStandardFixtures();
  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
});

test('本文選択から引用をメモへ追加できる', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');

  const quoteButton = page.locator('#quote-selection-action');
  await expect(quoteButton).toBeVisible();
  await quoteButton.click();

  await expect(page.locator('#panel-memo.active')).toBeVisible();
  const memoEditor = page.locator('#memo-editor');
  await expect(memoEditor).toHaveValue(/> Initial README content/);
  await expect(memoEditor).toHaveValue(/出典: \[README\.md > README \(L3\)\]\(\?file=README\.md#readme:L3\)/);
  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');
  await expect(page.locator('#memo-preview')).toContainText('Initial README content');
  await expect(page.locator('#memo-preview')).toContainText('README.md > README (L3)');
});

test('ファイルごとに別メモが読み込まれる', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');
  await page.locator('#quote-selection-action').click();
  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');

  await page.locator('.sidebar-tab[data-tab="files"]').click();
  await page.locator('[data-file="notes.md"]').click();
  await expect(page.locator('#content')).toContainText('Notes body');

  await page.locator('.sidebar-tab[data-tab="memo"]').click();
  await expect(page.locator('#memo-editor')).toHaveValue('');

  await selectParagraphText(page, 'Notes body');
  await page.locator('#quote-selection-action').click();
  await expect(page.locator('#memo-editor')).toHaveValue(/> Notes body/);
  await expect(page.locator('#memo-editor')).toHaveValue(/出典: \[notes\.md > Notes \(L3\)\]\(\?file=notes\.md#notes:L3\)/);
});

test('メモ読み込み失敗中は引用挿入から保存しない', async ({ page }) => {
  await resetStandardFixtures();
  const memoPath = path.join(fixtureDir, '.README.md.memo.md');
  const unreadableMemo = Buffer.from([0xff, 0xfe, 0xfd]);
  await fs.writeFile(memoPath, unreadableMemo);
  let putCount = 0;
  await page.route('**/api/memo', async (route) => {
    if (route.request().method() === 'PUT') {
      putCount += 1;
      await route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'unexpected memo save' })
      });
      return;
    }
    await route.continue();
  });

  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
  await expect(page.locator('#memo-editor')).toBeDisabled();
  await expect(page.locator('#memo-save-status')).toContainText('編集を無効化');

  await selectParagraphText(page, 'Initial README content');
  const quoteButton = page.locator('#quote-selection-action');
  await expect(quoteButton).toBeVisible();
  await quoteButton.click();

  await expect(page.locator('#panel-memo.active')).toBeVisible();
  await expect(page.locator('#memo-editor')).toHaveValue('');
  await expect.poll(() => putCount, { timeout: 500 }).toBe(0);
  await expect(await fs.readFile(memoPath)).toEqual(unreadableMemo);
});

test('メモ保存応答がload_errorを含んでも編集中の内容を消さない', async ({ page }) => {
  await page.route('**/api/memo', async (route) => {
    if (route.request().method() === 'PUT') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          raw: '',
          html: '<p>broken memo</p>',
          load_error: 'メモを読み込めませんでした。編集を無効化しました。'
        })
      });
      return;
    }
    await route.continue();
  });

  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
  await openMemoTab(page);

  const memoEditor = page.locator('#memo-editor');
  await memoEditor.fill('local draft that must remain');

  await expect(page.locator('#memo-save-status')).toContainText('編集を無効化');
  await expect(memoEditor).toHaveValue('local draft that must remain');
  await expect(memoEditor).toBeDisabled();
});

test('メモ保存応答のraw欠落では編集中の内容を消さない', async ({ page }) => {
  const warnings: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'warning') {
      warnings.push(message.text());
    }
  });
  await page.route('**/api/memo', async (route) => {
    if (route.request().method() === 'PUT') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          html: '<p>server preview without raw</p>'
        })
      });
      return;
    }
    await route.continue();
  });

  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
  await openMemoTab(page);

  const memoEditor = page.locator('#memo-editor');
  await memoEditor.fill('local draft that must remain');

  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');
  await expect(memoEditor).toHaveValue('local draft that must remain');
  await expect.poll(() => warnings.some((text) => text.includes('raw を含まないメモ応答'))).toBe(true);
});
