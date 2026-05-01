import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect } from '@playwright/test';
import { installTestWebSocketHarness } from './browser/test-websocket';
import { openMemoTab, resetStandardFixtures, selectFile, selectParagraphText, stabilizeWebSocketHarness } from './helpers';

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

test('メモload_error後もファイル切替で編集を再開できる', async ({ page }) => {
  await page.route('**/api/memo*', async (route) => {
    const request = route.request();
    if (request.method() !== 'GET') {
      await route.continue();
      return;
    }
    const url = new URL(request.url());
    if (url.searchParams.get('file') === 'notes.md') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          raw: '',
          html: '',
          file: 'notes.md',
          load_error: 'メモを読み込めませんでした。編集を無効化しました。'
        })
      });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        raw: 'recovered readme memo',
        html: '<p>recovered readme memo</p>',
        file: 'README.md'
      })
    });
  });

  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
  await openMemoTab(page);
  await expect(page.locator('#memo-editor')).toBeEnabled();

  await selectFile(page, 'notes.md');
  await openMemoTab(page);
  await expect(page.locator('#memo-editor')).toBeDisabled();

  await selectFile(page, 'README.md');
  await openMemoTab(page);

  await expect(page.locator('#memo-editor')).toBeEnabled();
  await expect(page.locator('#memo-editor')).toHaveValue('recovered readme memo');
  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');
});

test('メモ取得失敗時は古い本文を新ファイルへ保存できないようエディタを無効化する', async ({ page }) => {
  let putCount = 0;
  await page.route('**/api/memo*', async (route) => {
    const request = route.request();
    if (request.method() === 'PUT') {
      putCount += 1;
      const body = request.postDataJSON() as { raw?: string; file?: string | null };
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          raw: body.raw || '',
          html: `<p>${body.raw || ''}</p>`,
          file: body.file || 'README.md'
        })
      });
      return;
    }
    const url = new URL(request.url());
    if (url.searchParams.get('file') === 'notes.md') {
      await route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'memo load failed' })
      });
      return;
    }
    await route.continue();
  });

  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
  await openMemoTab(page);
  await page.locator('#memo-editor').fill('readme memo must not be saved as notes');
  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');
  putCount = 0;
  await expect(page.locator('#memo-editor')).toHaveValue('readme memo must not be saved as notes');

  await selectFile(page, 'notes.md');
  await openMemoTab(page);

  await expect(page.locator('#memo-editor')).toBeDisabled();
  await expect(page.locator('#memo-save-status')).toContainText('失敗');
  await page.waitForTimeout(700);
  await expect.poll(() => putCount).toBe(0);
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

test('メモ保存のネットワーク失敗は接続系エラーとして表示する', async ({ page }) => {
  await page.route('**/api/memo', async (route) => {
    if (route.request().method() === 'PUT') {
      await route.abort('failed');
      return;
    }
    await route.continue();
  });

  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
  await openMemoTab(page);

  await page.locator('#memo-editor').fill('network failed draft');

  await expect(page.locator('#memo-save-status')).toContainText('サーバー接続');
});

test('メモ保存応答のhtml欠落ではプレビューを空にしない', async ({ page }) => {
  const warnings: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'warning') {
      warnings.push(message.text());
    }
  });
  let putCount = 0;
  await page.route('**/api/memo', async (route) => {
    if (route.request().method() === 'PUT') {
      putCount += 1;
      if (putCount === 1) {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            raw: 'stable preview',
            html: '<p>stable preview</p>'
          })
        });
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          raw: 'server raw that must not overwrite'
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
  await memoEditor.fill('stable preview');
  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');
  await expect(page.locator('#memo-preview')).toContainText('stable preview');

  await memoEditor.fill('new draft');

  await expect(page.locator('#memo-save-status')).toContainText('メモ応答が不正');
  await expect(memoEditor).toHaveValue('new draft');
  await expect(page.locator('#memo-preview')).toContainText('stable preview');
  await expect.poll(() => warnings.some((text) => text.includes('html を含まないメモ応答'))).toBe(true);
});

test('古いメモ保存エラーが永続エラーなら後続保存後も表示する', async ({ page }) => {
  let putCount = 0;
  let releaseFirstFailure!: () => void;
  const firstFailureReady = new Promise<void>((resolve) => {
    releaseFirstFailure = resolve;
  });
  let firstRequestSeen!: () => void;
  const firstRequestReady = new Promise<void>((resolve) => {
    firstRequestSeen = resolve;
  });
  let secondRequestSeen!: () => void;
  const secondRequestReady = new Promise<void>((resolve) => {
    secondRequestSeen = resolve;
  });

  await page.route('**/api/memo', async (route) => {
    if (route.request().method() !== 'PUT') {
      await route.continue();
      return;
    }
    putCount += 1;
    if (putCount === 1) {
      firstRequestSeen();
      await firstFailureReady;
      await route.fulfill({
        status: 413,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'too large' })
      });
      return;
    }
    secondRequestSeen();
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        raw: 'second draft',
        html: '<p>second draft</p>'
      })
    });
  });

  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
  await openMemoTab(page);

  const memoEditor = page.locator('#memo-editor');
  await memoEditor.fill('oversized draft');
  await firstRequestReady;

  await memoEditor.fill('second draft');
  await secondRequestReady;
  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');

  releaseFirstFailure();

  await expect(page.locator('#memo-save-status')).toContainText('以前のメモ保存に失敗');
  await expect(memoEditor).toHaveValue('second draft');
});

test('WebSocket切断中のメモ保存は同期待ちとして表示する', async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.reload();
  await expect(page.locator('#content')).toContainText('Initial README content');
  await openMemoTab(page);

  let releaseSave!: () => void;
  const saveReleaseReady = new Promise<void>((resolve) => {
    releaseSave = resolve;
  });
  let saveRequestSeen!: () => void;
  const saveRequestReady = new Promise<void>((resolve) => {
    saveRequestSeen = resolve;
  });
  await page.route('**/api/memo', async (route) => {
    if (route.request().method() !== 'PUT') {
      await route.continue();
      return;
    }
    saveRequestSeen();
    await saveReleaseReady;
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        raw: 'saved while websocket is reconnecting',
        html: '<p>saved while websocket is reconnecting</p>'
      })
    });
  });

  await page.locator('#memo-editor').fill('saved while websocket is reconnecting');
  await saveRequestReady;
  await page.evaluate(() => {
    const liveStatus = document.getElementById('live-status');
    if (!liveStatus) {
      throw new Error('live status is not initialized');
    }
    liveStatus.dataset.state = 'retry';
  });
  releaseSave();

  await expect(page.locator('#memo-save-status')).toContainText('同期待ち');
});

test('WebSocket再接続後に同期待ちのメモ保存表示は保存済みへ戻る', async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.reload();
  await expect(page.locator('#content')).toContainText('Initial README content');
  await stabilizeWebSocketHarness(page);
  await openMemoTab(page);

  await page.evaluate(() => {
    const lastWs = window.__lastWs;
    if (!lastWs) {
      throw new Error('WebSocket test harness is not initialized');
    }
    window.__serverErrorWs = lastWs;
    lastWs.close();
  });
  await expect.poll(() => page.locator('#live-status').getAttribute('data-state')).toBe('retry');

  await page.locator('#memo-editor').fill('saved while websocket reconnects');
  await expect(page.locator('#memo-save-status')).toContainText('同期待ち');

  await page.waitForFunction(() => {
    return Boolean(window.__lastWs && window.__serverErrorWs && window.__lastWs !== window.__serverErrorWs);
  });
  await expect(page.locator('#live-status')).toHaveAttribute('data-state', 'live');
  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');
});
