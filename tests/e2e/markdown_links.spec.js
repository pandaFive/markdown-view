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

test('別ファイルのフラグメント履歴は戻る進むでも見出し位置を復元する', async ({ page }) => {
  await fs.writeFile(
    readmePath,
    '# README\n\n[Beta section](notes.md#beta)\n\nBack target\n'
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
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0);

  await page.goBack();
  await expect(page).toHaveURL(/file=README\.md$/);
  await expect(page.locator('#content')).toContainText('Back target');
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);

  await page.goForward();
  await expect(page).toHaveURL(/file=notes\.md#beta/);
  await expect(page.locator('#content')).toContainText('Target section body');
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#beta');
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0);
});

test('同一ファイル内フラグメント履歴は戻る進むでも見出し位置を復元する', async ({ page }) => {
  await fs.writeFile(
    readmePath,
    '# README\n\n[Alpha](README.md#alpha)\n\n[Beta](README.md#beta)\n\n' +
    Array.from({ length: 30 }, (_, index) => `Paragraph ${index + 1}`).join('\n\n') +
    '\n\n## Alpha\n\nAlpha section body\n\n' +
    Array.from({ length: 20 }, (_, index) => `Tail ${index + 1}`).join('\n\n') +
    '\n\n## Beta\n\nBeta section body\n'
  );

  await page.reload();
  await page.locator('#content a[href="README.md#alpha"]').click();
  await expect(page).toHaveURL(/file=README\.md#alpha/);
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#alpha');
  await expect.poll(async () => page.locator('#toc a.active').innerText()).toBe('Alpha');

  await page.locator('#content a[href="README.md#beta"]').click();
  await expect(page).toHaveURL(/file=README\.md#beta/);
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#beta');
  await expect.poll(async () => page.locator('#toc a.active').innerText()).toBe('Beta');

  await page.goBack();
  await expect(page).toHaveURL(/file=README\.md#alpha/);
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#alpha');
  await expect.poll(async () => page.locator('#toc a.active').innerText()).toBe('Alpha');

  await page.goForward();
  await expect(page).toHaveURL(/file=README\.md#beta/);
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#beta');
  await expect.poll(async () => page.locator('#toc a.active').innerText()).toBe('Beta');
});

test('同一ファイルの壊れたフラグメントリンクではURLのhashをクリアし警告を出す', async ({ page }) => {
  var warnings = [];
  page.on('console', function(message) {
    if (message.type() === 'warning') {
      warnings.push(message.text());
    }
  });

  await fs.writeFile(
    readmePath,
    '# README\n\n[Missing](README.md#missing)\n\n' +
    Array.from({ length: 30 }, (_, index) => `Intro ${index + 1}`).join('\n\n') +
    '\n\n## Alpha\n\nAlpha body\n'
  );

  await page.reload();
  await page.evaluate(() => {
    var alpha = document.getElementById('alpha');
    var offset = parseFloat(window.getComputedStyle(alpha).scrollMarginTop) || 112;
    window.scrollTo(0, alpha.getBoundingClientRect().top + window.scrollY - offset + 8);
  });
  await expect.poll(async () => page.locator('#toc a.active').innerText()).toBe('Alpha');
  await page.locator('#content a[href="README.md#missing"]').click();

  await expect(page).toHaveURL(/file=README\.md$/);
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('');
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  await expect(page.locator('#toc a.active')).toHaveCount(0);
  expect(warnings.some((msg) => msg.indexOf('見出しが見つかりません') !== -1)).toBe(true);
});

test('同一ファイルの自己リンクもSPA内で処理され先頭へ戻る', async ({ page }) => {
  await fs.writeFile(
    readmePath,
    '# README\n\n[Self](README.md)\n\n' +
    Array.from({ length: 40 }, (_, index) => `README line ${index + 1}`).join('\n\n')
  );

  await page.reload();
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0);

  await page.locator('#content a[href="README.md"]').click();

  await expect(page).toHaveURL(/file=README\.md$/);
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('');
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
});

test('別ファイルの壊れたフラグメントリンクでは対象文書を先頭から表示しURLのhashを消す', async ({ page }) => {
  var warnings = [];
  page.on('console', function(message) {
    if (message.type() === 'warning') {
      warnings.push(message.text());
    }
  });

  await fs.writeFile(
    readmePath,
    '# README\n\n[Missing notes](notes.md#missing)\n\n' +
    Array.from({ length: 40 }, (_, index) => `README line ${index + 1}`).join('\n\n')
  );
  await fs.writeFile(
    notesPath,
    '# Notes\n\n' +
    Array.from({ length: 60 }, (_, index) => `Notes line ${index + 1}`).join('\n\n')
  );

  await page.reload();
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0);

  await page.locator('#content a[href="notes.md#missing"]').click();

  await expect(page).toHaveURL(/file=notes\.md$/);
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('');
  await expect(page.locator('#content')).toContainText('Notes line 60');
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  expect(warnings.some((msg) => msg.indexOf('見出しが見つかりません') !== -1)).toBe(true);
});

async function installClickObserver(page) {
  await page.evaluate(() => {
    window.__clickObservations = {};
    window.addEventListener('click', function(event) {
      var link = event.target.closest('a[href]');
      if (!link) return;
      window.__clickObservations[link.getAttribute('href')] = {
        defaultPrevented: event.defaultPrevented
      };
      event.preventDefault();
    });
  });
}

test('外部スキームのリンクはSPA内遷移されない', async ({ page }) => {
  await fs.writeFile(
    readmePath,
    '# README\n\n[external](https://example.com/foo.md)\n\n[mail](mailto:foo@example.com)\n'
  );

  await page.reload();
  await installClickObserver(page);

  await page.locator('#content a[href="https://example.com/foo.md"]').click();
  await page.locator('#content a[href="mailto:foo@example.com"]').click();

  var results = await page.evaluate(() => window.__clickObservations);
  expect(results['https://example.com/foo.md'].defaultPrevented).toBe(false);
  expect(results['mailto:foo@example.com'].defaultPrevented).toBe(false);
  await expect(page).toHaveURL(/file=README\.md$/);
});

test('非Markdown拡張子の相対リンクはSPA内遷移されない', async ({ page }) => {
  await fs.writeFile(
    readmePath,
    '# README\n\n[report](report.pdf)\n'
  );

  await page.reload();
  await installClickObserver(page);

  await page.locator('#content a[href="report.pdf"]').click();

  var results = await page.evaluate(() => window.__clickObservations);
  expect(results['report.pdf'].defaultPrevented).toBe(false);
  await expect(page).toHaveURL(/file=README\.md$/);
});

test('日本語見出しへのフラグメントリンクでも対象見出しへ遷移する', async ({ page }) => {
  await fs.writeFile(
    readmePath,
    '# README\n\n[見出しへ](notes.md#日本語見出し)\n'
  );
  await fs.writeFile(
    notesPath,
    '# Notes\n\n' +
    Array.from({ length: 40 }, (_, index) => `Paragraph ${index + 1}`).join('\n\n') +
    '\n\n## 日本語見出し\n\n日本語見出しの本文\n'
  );

  await page.reload();
  await page.locator('#content a[href$="日本語見出し"]').click();

  await expect(page).toHaveURL(/file=notes\.md/);
  await expect(page.locator('#content')).toContainText('日本語見出しの本文');
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0);
  await expect.poll(() => page.evaluate(() => Boolean(document.getElementById('日本語見出し')))).toBe(true);
});
