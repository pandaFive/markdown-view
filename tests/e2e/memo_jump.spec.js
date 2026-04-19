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
  '',
  '## Section Blockquote',
  '',
  '> Outer quote opening line.',
  '>',
  '> > Nested blockquote inner TARGET line.',
  '>',
  '> Outer quote closing line.',
  '',
  '## Section Multiline',
  '',
  'Multi line one content.',
  'Multi line two MULTILINE TARGET.',
  'Multi line three content.',
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

/// 指定したテキストを含む block の data-line-block-start を返す。
/// 複数行段落・blockquote内paragraph どちらにも対応する。
async function lineBlockStartOf(page, needle) {
  return page.evaluate((text) => {
    const walker = document.createTreeWalker(document.getElementById('content'), NodeFilter.SHOW_TEXT);
    let node;
    while ((node = walker.nextNode())) {
      if (!node.textContent || !node.textContent.includes(text)) continue;
      let el = node.parentElement;
      while (el && !el.hasAttribute('data-line-block-start') && !el.hasAttribute('data-source-start-line')) {
        el = el.parentElement;
      }
      if (!el) return { start: -1, end: -1 };
      const start = parseInt(el.getAttribute('data-line-block-start') || el.getAttribute('data-source-start-line'), 10);
      const end = parseInt(el.getAttribute('data-line-block-end') || el.getAttribute('data-source-end-line'), 10);
      return { start, end };
    }
    return { start: -1, end: -1 };
  }, needle);
}

test('不正な行番号 #L0 はジャンプせずスクロール位置を維持する', async ({ page }) => {
  // renderer は 1-indexed。`scrollToLineRange` は targetLine < 1 で早期return する
  await page.goto('/?file=long.md#L0');
  await expect(page.locator('#content')).toContainText('Long Document');
  // rAFによるscroll復元が走る時間を与える
  await page.waitForTimeout(200);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
  await expect(page.locator('#content .jump-highlight')).toHaveCount(0);
});

test('負の行番号 #L-5 は heading にマッチせずジャンプしない', async ({ page }) => {
  // `^L(\d+)` は負数にマッチしないため headingId="L-5" として扱われ、該当idは存在しない
  await page.goto('/?file=long.md#L-5');
  await expect(page.locator('#content')).toContainText('Long Document');
  await page.waitForTimeout(200);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
  await expect(page.locator('#content .jump-highlight')).toHaveCount(0);
});

test('行範囲が範囲外の場合は heading fallback へスクロールする', async ({ page }) => {
  // 行L99999は存在しないが heading `section-c` は存在する。applyContentAnchorNavigation が
  // 行範囲スクロールに失敗したあと heading fallback に切り替わることを検証する
  await page.goto('/?file=long.md#section-c:L99999');
  await expect(page.locator('#content')).toContainText('Long Document');

  await expect.poll(() => page.evaluate(() => window.scrollY), { timeout: 3000 })
    .toBeGreaterThan(0);

  // heading fallback 経路は jump-highlight を付与しない（scrollToLineRange のみトリガする）
  await expect(page.locator('#content .jump-highlight')).toHaveCount(0);

  // section-c が viewport 内に表示されていることを確認（固定ヘッダー等のoffsetで完全に上端ではない）
  const sectionCInView = await page.evaluate(() => {
    const el = document.getElementById('section-c');
    if (!el) return false;
    const rect = el.getBoundingClientRect();
    return rect.top >= 0 && rect.top < window.innerHeight;
  });
  expect(sectionCInView).toBe(true);
});

test('ネストしたblockquote内の行へジャンプすると最内段落がハイライトされる', async ({ page }) => {
  // 外側blockquoteと内側blockquoteの双方がlineを含むため、最狭マッチの内側<p>が選ばれる必要がある
  await page.goto('/?file=long.md');
  await expect(page.locator('#content')).toContainText('Nested blockquote inner TARGET');

  const range = await lineBlockStartOf(page, 'Nested blockquote inner TARGET');
  expect(range.start).toBeGreaterThan(0);

  await page.goto(`/?file=long.md#L${range.start}`);
  const highlighted = page.locator('#content .jump-highlight');
  await expect(highlighted).toBeVisible();
  await expect(highlighted).toContainText('Nested blockquote inner TARGET');

  // 外側のblockquote全体ではなく innermost <p> がハイライト対象
  const tagName = await highlighted.evaluate((el) => el.tagName.toLowerCase());
  expect(tagName).toBe('p');
});

test('旧形式メモ（リンク外L15）の出典クリックでも行範囲ジャンプできる', async ({ page }) => {
  // PR #73 以前に生成されたメモは `出典: [...](...#heading) L15` のように
  // 行範囲がリンク外テキストとして並ぶ。このレガシー形式でも fine-grained ジャンプできることを検証する。
  // long.md L15 は `Paragraph B2 content TARGET BLOCK.` に対応する
  const memoPath = path.join(fixtureDir, '.long.md.memo.md');
  const legacyMemo = [
    '> Paragraph B2 content TARGET BLOCK.',
    '',
    '出典: [long.md > Section B](?file=long.md#section-b) L15',
    ''
  ].join('\n');
  await fs.writeFile(memoPath, legacyMemo);

  // サーバー側で初期描画にメモを反映させるためリロード
  await page.reload();
  await page.locator('.sidebar-tab[data-tab="memo"]').click();
  await expect(page.locator('#panel-memo.active')).toBeVisible();

  const sourceLink = page.locator('#memo-preview a[href="?file=long.md#section-b"]').first();
  await expect(sourceLink).toBeVisible();
  // 隣接ノード (text/span) の textContent に `L15` が存在することを確認（旧形式の identifying 条件）
  const tail = await sourceLink.evaluate((link) => (link.nextSibling ? link.nextSibling.textContent : ''));
  expect(tail).toMatch(/^\s*L15\b/);

  await page.evaluate(() => window.scrollTo(0, 0));
  await sourceLink.click();

  await expect.poll(() => page.evaluate(() => window.scrollY), { timeout: 3000 }).toBeGreaterThan(0);

  // heading (section-b) ではなく L15 の paragraph に着地
  const highlighted = page.locator('#content .jump-highlight');
  await expect(highlighted).toBeVisible();
  await expect(highlighted).toContainText('TARGET BLOCK');

  await expect(page.locator('#content .jump-highlight')).toHaveCount(0, { timeout: 5000 });
});

test('複数行にまたがる段落の中間行へのジャンプは段落全体を最狭マッチとして選ぶ', async ({ page }) => {
  // 複数行で1つの<p>になる段落。中間行を指定しても同じ段落がハイライトされる
  await page.goto('/?file=long.md');
  await expect(page.locator('#content')).toContainText('MULTILINE TARGET');

  const range = await lineBlockStartOf(page, 'MULTILINE TARGET');
  expect(range.start).toBeGreaterThan(0);
  expect(range.end).toBeGreaterThanOrEqual(range.start);

  const midLine = Math.floor((range.start + range.end) / 2);
  expect(midLine).toBeGreaterThanOrEqual(range.start);
  expect(midLine).toBeLessThanOrEqual(range.end);

  await page.goto(`/?file=long.md#L${midLine}`);
  const highlighted = page.locator('#content .jump-highlight');
  await expect(highlighted).toBeVisible();
  await expect(highlighted).toContainText('MULTILINE TARGET');
});
