const fs = require('node:fs/promises');
const path = require('node:path');
const { test, expect } = require('@playwright/test');

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');
const readmePath = path.join(fixtureDir, 'README.md');
const notesPath = path.join(fixtureDir, 'notes.md');

async function resetFixtures() {
  await fs.writeFile(readmePath, '# README\n\nInitial README content\n');
  await fs.writeFile(notesPath, '# Notes\n\nNotes body\n');
}

test.beforeEach(async ({ page }) => {
  await resetFixtures();
  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
});

test.afterEach(async () => {
  await resetFixtures();
});

test('ディレクトリモードでMarkdown相対リンクをクリックすると同一アプリ内で対象文書へ遷移する', async ({ page }) => {
  await fs.writeFile(
    readmePath,
    '# README\n\n[Notes](notes.md)\n'
  );
  await fs.writeFile(
    notesPath,
    '# Notes\n\nNotes body from markdown link\n'
  );

  await page.reload();
  await page.locator('#content a[href="notes.md"]').click();

  await expect(page).toHaveURL(/file=notes\.md/);
  await expect(page.locator('#content')).toContainText('Notes body from markdown link');
});

test('ディレクトリモードでMarkdown相対リンクのフラグメントをクリックすると遷移先見出しへ移動する', async ({ page }) => {
  await fs.writeFile(
    readmePath,
    '# README\n\n[Beta section](notes.md#beta)\n'
  );
  await fs.writeFile(
    notesPath,
    '# Notes\n\n' +
    Array.from({ length: 40 }, (_, index) => `Paragraph ${index + 1}`).join('\n\n') +
    '\n\n## Beta\n\nTarget section body\n'
  );

  await page.reload();
  await page.locator('#content a[href="notes.md#beta"]').click();

  await expect(page).toHaveURL(/file=notes\.md#beta/);
  await expect(page.locator('#content')).toContainText('Target section body');
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#beta');
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0);
});
