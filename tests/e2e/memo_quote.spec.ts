import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect, type Page } from '@playwright/test';

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');
const readmePath = path.join(fixtureDir, 'README.md');
const notesPath = path.join(fixtureDir, 'notes.md');
async function resetFixtures() {
  const entries = await fs.readdir(fixtureDir, { withFileTypes: true });
  await Promise.all(entries
    .filter((entry) => entry.isFile() && entry.name.endsWith('.memo.md'))
    .map((entry) => fs.rm(path.join(fixtureDir, entry.name), { force: true })));
  await fs.rm(path.join(fixtureDir, '.markdown-view'), { recursive: true, force: true });
  await fs.writeFile(readmePath, '# README\n\nInitial README content\n');
  await fs.writeFile(notesPath, '# Notes\n\nNotes body\n');
}

async function selectParagraphText(page: Page, text: string) {
  await page.evaluate((targetText) => {
    const walker = document.createTreeWalker(document.getElementById('content')!, NodeFilter.SHOW_TEXT);
    let node = null;
    while ((node = walker.nextNode())) {
      if (node.textContent && node.textContent.includes(targetText)) {
        const selection = window.getSelection()!;
        const range = document.createRange();
        range.selectNodeContents(node.parentElement!);
        selection.removeAllRanges();
        selection.addRange(range);
        return;
      }
    }
    throw new Error(`text not found: ${targetText}`);
  }, text);
}

test.beforeEach(async ({ page }) => {
  await resetFixtures();
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
