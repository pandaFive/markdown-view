import { test, expect } from '@playwright/test';
import { resetStandardFixtures, selectParagraphText } from './helpers';

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
