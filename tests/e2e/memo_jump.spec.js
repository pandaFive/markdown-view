const fs = require('node:fs/promises');
const path = require('node:path');
const { test, expect } = require('@playwright/test');

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');

const longContent = [
  '# Long Document',
  '',
  '## Section A',
  '',
  'Paragraph A1 content.',
  '',
  'Paragraph A2 content.',
  '',
  'Paragraph A3 content.',
  '',
  '## Section B',
  '',
  'Paragraph B1 content.',
  '',
  'Paragraph B2 content TARGET BLOCK.',
  '',
  'Paragraph B3 content.',
  '',
  '## Section C',
  '',
  'Paragraph C1 content.',
  '',
  'Paragraph C2 content.',
  '',
  'Paragraph C3 content.',
  '',
  '## Section D',
  '',
  'Paragraph D1 content.',
  '',
  'Paragraph D2 content.',
  '',
  'Paragraph D3 content.',
  ''
].join('\n');

async function resetLongFixture() {
  const longPath = path.join(fixtureDir, 'long.md');
  const entries = await fs.readdir(fixtureDir, { withFileTypes: true });
  await Promise.all(entries
    .filter((entry) => entry.isFile() && entry.name.endsWith('.memo.md'))
    .map((entry) => fs.rm(path.join(fixtureDir, entry.name), { force: true })));
  await fs.rm(path.join(fixtureDir, '.markdown-view'), { recursive: true, force: true });
  await fs.writeFile(longPath, longContent);
}

async function selectParagraphText(page, text) {
  await page.evaluate((targetText) => {
    const walker = document.createTreeWalker(document.getElementById('content'), NodeFilter.SHOW_TEXT);
    let node = null;
    while ((node = walker.nextNode())) {
      if (node.textContent && node.textContent.includes(targetText)) {
        const selection = window.getSelection();
        const range = document.createRange();
        range.selectNodeContents(node.parentElement);
        selection.removeAllRanges();
        selection.addRange(range);
        return;
      }
    }
    throw new Error(`text not found: ${targetText}`);
  }, text);
}

test.beforeEach(async ({ page }) => {
  await resetLongFixture();
  await page.goto('/?file=long.md');
  await expect(page.locator('#content')).toContainText('TARGET BLOCK');
});

test('メモ出典クリックで本文の対応ブロックへスクロールしハイライトされる', async ({ page }) => {
  // 1. 中盤の段落を選択して引用追加 → メモタブが activate される
  await selectParagraphText(page, 'TARGET BLOCK');
  const quoteButton = page.locator('#quote-selection-action');
  await expect(quoteButton).toBeVisible();
  await quoteButton.click();
  await expect(page.locator('#panel-memo.active')).toBeVisible();
  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');

  // 2. メモプレビューの出典リンク href が `:L<n>` フラグメントを含むことを確認
  const sourceLink = page.locator('#memo-preview a[href*="long.md"]').first();
  await expect(sourceLink).toBeVisible();
  const href = await sourceLink.getAttribute('href');
  expect(href).toMatch(/long\.md#section-b:L\d+/);

  // 3. 本文を一度トップへスクロールしてから出典リンクをクリック
  await page.evaluate(() => window.scrollTo(0, 0));
  await sourceLink.click();

  // 4. スクロール位置が変化したことを確認（同じファイル内ジャンプ）
  await expect.poll(() => page.evaluate(() => window.scrollY), {
    timeout: 3000
  }).toBeGreaterThan(0);

  // 5. 対応ブロックに .jump-highlight クラスが一時付与される
  const highlighted = page.locator('#content .jump-highlight');
  await expect(highlighted).toBeVisible();
  await expect(highlighted).toContainText('TARGET BLOCK');

  // 6. アニメーション完了後 (~2.5秒) にクラスが除去される
  await expect(page.locator('#content .jump-highlight')).toHaveCount(0, { timeout: 5000 });
});
