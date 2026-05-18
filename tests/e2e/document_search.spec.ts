import { test, expect, type Page } from '@playwright/test';
import { installTestWebSocketHarness } from './browser/test-websocket';
import { stabilizeWebSocketHarness, updateContent, updateContentAndActivateToc } from './helpers';

function searchFixtureContent(): string {
  return (
    '<h1 id="readme">README</h1>' +
    '<p>Alpha note appears here.</p>' +
    '<h2 id="details">Details</h2>' +
    '<p>Alpha note appears again in the details section.</p>' +
    '<h3 id="deep-dive">Deep dive</h3>' +
    '<p>alpha note appears a third time in lowercase.</p>'
  );
}

function searchFixtureToc(): string {
  return (
    '<ul>' +
    '<li><a href="#readme">README</a></li>' +
    '<li><a href="#details">Details</a></li>' +
    '<li><a href="#deep-dive">Deep dive</a></li>' +
    '</ul>'
  );
}

async function loadSearchFixture(page: Page) {
  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(false);
  });
  await updateContentAndActivateToc(page, {
    content: searchFixtureContent(),
    toc: searchFixtureToc()
  });
  await expect(page.locator('#document-search-input')).toBeVisible();
  await expect(page.locator('#content')).toContainText('Alpha note appears here.');
}

async function visibleMatchCount(page: Page) {
  return page.evaluate(() => {
    return new Set(
      Array.from(document.querySelectorAll<HTMLElement>('#content mark.document-search-match')).map((mark) => mark.dataset.matchId)
    ).size;
  });
}

async function currentMatchText(page: Page) {
  return page.evaluate(() => {
    return Array.from(document.querySelectorAll('#content mark.document-search-match.current'))
      .map((mark) => mark.textContent || '')
      .join('');
  });
}

async function setDocumentSearchQuery(page: Page, query: string) {
  await page.locator('#document-search-input').fill(query);
}

test.beforeEach(async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.goto('/');
  await stabilizeWebSocketHarness(page);
  await loadSearchFixture(page);
});

test('Ctrl/Cmd+Fで本文検索欄を開いてフォーカスする', async ({ page }) => {
  const modifier = process.platform === 'darwin' ? 'Meta' : 'Control';
  await page.keyboard.press(`${modifier}+f`);
  await expect(page.locator('#document-search-input')).toBeFocused();
});

test('検索語を入力するとヒット件数を表示して本文を強調する', async ({ page }) => {
  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 3 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(3);
  await expect.poll(() => currentMatchText(page)).toContain('Alpha note');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(3);
});

test('見出しテキストも文書内検索の対象に含める', async ({ page }) => {
  await setDocumentSearchQuery(page, 'deep dive');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 1 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(1);
  await expect.poll(() => currentMatchText(page)).toContain('Deep dive');
});

test('リンクやコードブロック内の一致は検索ハイライト対象にしない', async ({ page }) => {
  await updateContentAndActivateToc(page, {
    content:
      '<h1 id="readme">README</h1>' +
      '<p>Alpha note appears here.</p>' +
      '<p><a href="https://example.com">alpha note link</a></p>' +
      '<pre class="code-block"><code>alpha note code</code></pre>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha note link');

  await expect(page.locator('#document-search-summary')).toHaveText('0 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(0);
  await expect(page.locator('#content a mark.document-search-match')).toHaveCount(0);
  await expect(page.locator('#content pre mark.document-search-match')).toHaveCount(0);

  await setDocumentSearchQuery(page, 'alpha note code');
  await expect(page.locator('#document-search-summary')).toHaveText('0 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(0);
  await expect(page.locator('#content pre mark.document-search-match')).toHaveCount(0);
});

test('inline code内の一致も検索対象に含める', async ({ page }) => {
  await updateContentAndActivateToc(page, {
    content:
      '<h1 id="readme">README</h1>' +
      '<p>Run <code>cargo test</code> after editing.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'cargo test');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 1 件');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(1);
  await expect(page.locator('#document-search-results .document-search-result').first())
    .toContainText('Run cargo test after editing.');
});

test('装飾をまたぐ語句も検索できる', async ({ page }) => {
  await updateContentAndActivateToc(page, {
    content:
      '<h1 id="readme">README</h1>' +
      '<p>Alpha <strong>note</strong> appears across formatting.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 1 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(1);
  await expect.poll(() => currentMatchText(page)).toContain('Alpha note');
});

test('EnterとShift+Enterで次前のヒットへ移動する', async ({ page }) => {
  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 3 件');

  await page.locator('#document-search-input').focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#document-search-summary')).toHaveText('2 / 3 件');
  await expect(page.locator('#document-search-results .document-search-result').nth(1)).toHaveClass(/active/);

  await page.keyboard.press('Shift+Enter');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 3 件');
  await expect(page.locator('#document-search-results .document-search-result').nth(0)).toHaveClass(/active/);
});

test('クリアで検索状態と強調が消える', async ({ page }) => {
  await setDocumentSearchQuery(page, 'alpha');
  await expect.poll(() => visibleMatchCount(page)).toBe(3);

  await page.locator('#document-search-clear').click();
  await expect(page.locator('#document-search-summary')).toHaveText('0 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(0);
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(0);
});

test('ファイル切り替え時に検索状態をリセットする', async ({ page }) => {
  await setDocumentSearchQuery(page, 'alpha');
  await expect.poll(() => visibleMatchCount(page)).toBe(3);

  await page.evaluate(() => {
    window.markdownViewTestHooks.selectFile('notes.md');
  });

  await expect(page.locator('#content')).toContainText('Notes body');
  await expect(page.locator('#document-search-input')).toHaveValue('');
  await expect(page.locator('#document-search-summary')).toHaveText('0 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(0);
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(0);
});

test('ディレクトリモードでファイル切り替え時は本文の表示位置を先頭へ戻す', async ({ page }) => {
  await page.route('**/api/content?file=notes.md', async (route) => {
    const paragraphs = Array.from(
      { length: 80 },
      (_, index) => `<p>Notes body line ${index + 1}</p>`
    ).join('');

    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        file: 'notes.md',
        content: '<h1 id="notes">Notes</h1>' + paragraphs,
        toc: '<ul><li><a href="#notes">Notes</a></li></ul>'
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.scrollTo(0, document.documentElement.scrollHeight);
  });

  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0);

  await page.evaluate(() => {
    window.markdownViewTestHooks.selectFile('notes.md');
  });

  await expect(page.locator('#content')).toContainText('Notes body line 80');
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
});

test('ディレクトリモードで同一ファイルを再読込したときは本文スクロール位置を維持する', async ({ page }) => {
  await page.route('**/api/content?file=README.md', async (route) => {
    const paragraphs = Array.from(
      { length: 80 },
      (_, index) => `<p>README body line ${index + 1}</p>`
    ).join('');

    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        file: 'README.md',
        content: '<h1 id="readme">README</h1>' + paragraphs,
        toc: '<ul><li><a href="#readme">README</a></li></ul>'
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
    window.scrollTo(0, document.documentElement.scrollHeight);
  });

  const beforeScrollY = await page.evaluate(() => window.scrollY);
  await expect(beforeScrollY).toBeGreaterThan(0);

  await page.evaluate(() => {
    window.markdownViewTestHooks.selectFile('README.md', false);
  });

  await expect(page.locator('#content')).toContainText('README body line 80');
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(beforeScrollY);
});

test('ファイル切り替え失敗時は元文書の検索状態を維持する', async ({ page }) => {
  await setDocumentSearchQuery(page, 'alpha');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 3 件');

  await page.evaluate(() => {
    window.markdownViewTestHooks.selectFile('missing.md');
  });

  await expect(page.locator('#file-fetch-error-banner')).toContainText('指定したファイルが見つかりません。');
  await expect(page.locator('#content')).toContainText('Alpha note appears here.');
  await expect(page.locator('#document-search-input')).toHaveValue('alpha');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 3 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(3);
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(3);
});

test('古いファイル取得成功は新しい取得エラー表示を消さない', async ({ page }) => {
  let oldRequestStarted = false;
  let releaseOldResponse!: () => void;

  await page.route('**/api/content**', async (route) => {
    const url = new URL(route.request().url());
    const file = url.searchParams.get('file');

    if (file === 'old.md') {
      oldRequestStarted = true;
      await new Promise<void>((resolve) => {
        releaseOldResponse = resolve;
      });
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          file: 'old.md',
          content: '<h1 id="old">Old</h1><p>old response should stay stale</p>',
          toc: '<ul><li><a href="#old">Old</a></li></ul>'
        })
      });
      return;
    }

    if (file === 'missing.md') {
      await route.fulfill({ status: 404, contentType: 'text/plain', body: 'missing' });
      return;
    }

    await route.fallback();
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.selectFile('old.md');
  });
  await expect.poll(() => oldRequestStarted).toBe(true);

  await page.evaluate(() => {
    window.markdownViewTestHooks.selectFile('missing.md');
  });
  await expect(page.locator('#file-fetch-error-banner')).toContainText('指定したファイルが見つかりません。');

  const oldResponse = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return url.pathname === '/api/content' && url.searchParams.get('file') === 'old.md';
  });
  releaseOldResponse();
  await oldResponse;
  await page.evaluate(() => new Promise(requestAnimationFrame));

  await expect(page.locator('#file-fetch-error-banner')).toContainText('指定したファイルが見つかりません。');
  await expect(page.locator('#content')).not.toContainText('old response should stay stale');
});

test('ファイル取得の契約違反はサーバー応答エラーとして表示する', async ({ page }) => {
  await page.route('**/api/content**', async (route) => {
    const url = new URL(route.request().url());
    if (url.searchParams.get('file') === 'broken.md') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          file: 'broken.md',
          content: '<h1 id="broken">Broken</h1>'
        })
      });
      return;
    }
    await route.fallback();
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
    window.markdownViewTestHooks.selectFile('broken.md');
  });

  await expect(page.locator('#file-fetch-error-banner')).toContainText('サーバー応答の解析に失敗しました。');
  await expect(page.locator('#file-fetch-error-banner')).not.toContainText('ネットワークエラー');
  await expect(page.locator('#content')).not.toContainText('Broken');
});

test('live update後も検索結果を再適用する', async ({ page }) => {
  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 3 件');

  await updateContent(page, {
    content:
      '<h1 id="readme">README</h1>' +
      '<p>Alpha note appears here.</p>' +
      '<h2 id="details">Details</h2>' +
      '<p>Alpha note appears again in the details section.</p>' +
      '<h3 id="deep-dive">Deep dive</h3>' +
      '<p>alpha note appears a third time in lowercase.</p>' +
      '<p>Alpha note appears a fourth time after live update.</p>',
    toc:
      '<ul>' +
      '<li><a href="#readme">README</a></li>' +
      '<li><a href="#details">Details</a></li>' +
      '<li><a href="#deep-dive">Deep dive</a></li>' +
      '</ul>'
  });

  await expect(page.locator('#document-search-summary')).toHaveText('1 / 4 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(4);
  await expect.poll(() => currentMatchText(page)).toContain('Alpha note');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(4);
});

test('検索結果一覧に前後文を表示してクリックで該当箇所へ移動する', async ({ page }) => {
  await updateContentAndActivateToc(page, {
    content:
      '<h1 id="readme">README</h1>' +
      '<p>Opening sentence. Alpha note appears here. Closing sentence.</p>' +
      '<p>Another intro. Alpha note appears again in the details section. Another ending.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(2);
  await expect(page.locator('#document-search-results .document-search-result').nth(0))
    .toContainText('Opening sentence. Alpha note appears here. Closing sentence.');
  await expect(page.locator('#document-search-results .document-search-result').nth(1))
    .toContainText('Another intro. Alpha note appears again in the details section. Another ending.');

  const secondResult = page.locator('#document-search-results .document-search-result').nth(1);
  await expect(secondResult).toBeVisible();
  await secondResult.evaluate((element: HTMLElement) => {
    element.click();
  });
  await expect(page.locator('#document-search-summary')).toHaveText('2 / 2 件');
  await expect(page.locator('#document-search-results .document-search-result').nth(1)).toHaveClass(/active/);
});

test('検索queryは検索結果リストでHTMLとして解釈されない', async ({ page }) => {
  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(false);
  });
  await updateContentAndActivateToc(page, {
    content: '<p>literal &lt;img src=x onerror=alert(1)&gt; appears here</p>',
    toc: '<ul></ul>'
  });

  const query = '<img src=x onerror=alert(1)>';
  await setDocumentSearchQuery(page, query);

  await expect(page.locator('#document-search-results')).toContainText(query);
  await expect(page.locator('#document-search-results img')).toHaveCount(0);
  await expect(page.locator('mark.document-search-match')).toHaveCount(1);
});

test('ディレクトリ検索結果はHTMLとして解釈されない', async ({ page }) => {
  const injected = '<img src=x onerror=alert(1)>';
  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: injected,
        results: [
          {
            file: injected,
            file_match_index: 0,
            before: 'before ' + injected,
            current: injected,
            after: injected + ' after'
          }
        ],
        searched_files: 1,
        skipped_files: 0
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>literal marker</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, injected);

  await expect(page.locator('#document-search-results')).toContainText(injected);
  await expect(page.locator('#document-search-results img')).toHaveCount(0);
});

test('検索結果移動時に一覧のスクロール位置を維持する', async ({ page }) => {
  const paragraphs = Array.from(
    { length: 18 },
    (_, index) => `<p>Entry ${index + 1}. Alpha note appears in result ${index + 1}. Tail ${index + 1}.</p>`
  ).join('');
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1>' + paragraphs,
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(18);

  const beforeScrollTop = await page.evaluate(() => {
    const results = document.getElementById('document-search-results')!;
    results.scrollTop = results.scrollHeight;
    return results.scrollTop;
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.moveDocumentSearch(1);
  });

  await expect.poll(() => page.evaluate(() => {
    return document.getElementById('document-search-results')!.scrollTop;
  })).toBe(beforeScrollTop);
});

test('ディレクトリモードでは検索API結果を一覧表示する', async ({ page }) => {
  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'note',
        results: [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'Alpha note appears here.',
            after: ''
          },
          {
            file: 'notes.md',
            file_match_index: 0,
            before: '',
            current: 'Notes body with alpha note.',
            after: ''
          }
        ],
        searched_files: 2,
        skipped_files: 0
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha note appears here.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'note');

  await expect(page.locator('#document-search-summary')).toHaveText('1 / 2 件');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(2);
  await expect(page.locator('#document-search-results .document-search-result').first())
    .toContainText('README.md');
  await expect(page.locator('#document-search-results .document-search-result').nth(1))
    .toContainText('notes.md');
});

test('ディレクトリ検索APIへタブ内クライアントIDを送る', async ({ page }) => {
  const clientIds: string[] = [];
  await page.route('**/api/search**', async (route) => {
    const url = new URL(route.request().url());
    clientIds.push(route.request().headers()['x-markdown-view-search-client'] || '');
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: url.searchParams.get('q') || '',
        results: [],
        searched_files: 0,
        skipped_files: 0,
        truncated: false,
        truncated_reasons: []
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha beta</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha');
  await expect.poll(() => clientIds.length).toBe(1);
  await setDocumentSearchQuery(page, 'beta');
  await expect.poll(() => clientIds.length).toBe(2);

  expect(clientIds[0]).toMatch(/^tab-[a-z0-9]+-[a-z0-9]+$/);
  expect(clientIds[1]).toBe(clientIds[0]);
});

test('ディレクトリ検索の不正な成功応答は検索エラーとして表示する', async ({ page }) => {
  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        results: []
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha note appears here.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha');

  await expect(page.locator('#document-search-summary')).toHaveText('エラー');
  await expect(page.locator('#document-search-results')).toContainText('サーバー応答の解析に失敗しました。');
});

test('ディレクトリ検索の400エラーはAPI本文の固定文言を表示する', async ({ page }) => {
  const query = 'あ'.repeat(257);

  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 400,
      contentType: 'application/json',
      body: JSON.stringify({
        error: '検索クエリが長すぎます'
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha note appears here.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, query);

  await expect(page.locator('#document-search-summary')).toHaveText('エラー');
  await expect(page.locator('#document-search-results')).toContainText('検索クエリが長すぎます');
  await expect(page.locator('#document-search-results')).not.toContainText(query);
});

test('ディレクトリ検索の不正な結果要素は検索エラーとして表示する', async ({ page }) => {
  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'alpha',
        results: [{}]
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha note appears here.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha');

  await expect(page.locator('#document-search-summary')).toHaveText('エラー');
  await expect(page.locator('#document-search-results')).toContainText('サーバー応答の解析に失敗しました。');
});

test('ディレクトリモードでは検索打ち切り警告を結果一覧の先頭に表示する', async ({ page }) => {
  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'alpha',
        results: [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'Alpha result is visible.',
            after: ''
          }
        ],
        searched_files: 1,
        skipped_files: 0,
        truncated: true,
        truncated_reasons: ['result_limit'],
        limits: {
          max_results: 100,
          max_files: 1000,
          max_bytes: 67108864
        },
        searched_bytes: 1024
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha result is visible.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha');

  await expect(page.locator('#document-search-results')).toContainText('上限により一部のみ表示しています。');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(1);
  await expect(page.locator('#document-search-results .document-search-result').first())
    .toContainText('Alpha result is visible.');
});

test('ディレクトリモードでは検索結果0件でも検索打ち切り警告を表示する', async ({ page }) => {
  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'alpha',
        results: [],
        searched_files: 0,
        skipped_files: 0,
        truncated: true,
        truncated_reasons: ['byte_limit'],
        limits: {
          max_results: 100,
          max_files: 1000,
          max_bytes: 67108864
        },
        searched_bytes: 0
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>No matching text here.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha');

  await expect(page.locator('#document-search-results')).toContainText('上限により一部のみ表示しています。');
  await expect(page.locator('#document-search-results')).toContainText('ディレクトリ内に一致が見つかりません。');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(0);
});

test('ディレクトリモードでは現在ファイルの本文ヒットを検索結果選択に反映する', async ({ page }) => {
  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'alpha note',
        results: [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'Alpha note appears here.',
            after: ''
          },
          {
            file: 'README.md',
            file_match_index: 1,
            before: '',
            current: 'Alpha note appears again in the details section.',
            after: ''
          }
        ],
        searched_files: 1,
        skipped_files: 0
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content:
      '<h1 id="readme">README</h1>' +
      '<p>Alpha note appears here.</p>' +
      '<p>Alpha note appears again in the details section.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha note');

  await expect(page.locator('#document-search-summary')).toHaveText('1 / 2 件');
  await expect(page.locator('#document-search-results .document-search-result').first()).toHaveClass(/active/);
});

test('ディレクトリモードでは他ファイルのlive updateでも検索結果一覧を再取得する', async ({ page }) => {
  let searchCallCount = 0;
  await page.route('**/api/search**', async (route) => {
    searchCallCount += 1;
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'alpha note',
        results: searchCallCount === 1 ? [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'Alpha note appears here.',
            after: ''
          }
        ] : [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'Alpha note appears here.',
            after: ''
          },
          {
            file: 'notes.md',
            file_match_index: 0,
            before: '',
            current: 'Notes alpha note appears after update.',
            after: ''
          }
        ],
        searched_files: 2,
        skipped_files: 0
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha note appears here.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(1);

  await page.evaluate(() => {
    const realWsOnmessage = window.__realWsOnmessage;
    if (!realWsOnmessage) {
      throw new Error('WebSocket test harness message handler is not initialized');
    }
    realWsOnmessage({
      data: JSON.stringify({
        file: 'notes.md',
        content: '<h1 id="notes">Notes</h1><p>Notes alpha note appears after update.</p>',
        toc: '<ul><li><a href="#notes">Notes</a></li></ul>'
      })
    });
  });

  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(2);
  await expect(page.locator('#document-search-results .document-search-result').nth(1))
    .toContainText('notes.md');
});

test('ディレクトリモードの初回キーボード移動は先頭の検索結果を開く', async ({ page }) => {
  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'note',
        results: [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'Alpha note appears here.',
            after: ''
          },
          {
            file: 'notes.md',
            file_match_index: 0,
            before: '',
            current: 'Notes body with alpha note.',
            after: ''
          }
        ],
        searched_files: 2,
        skipped_files: 0
      })
    });
  });
  await page.route('**/api/content?file=README.md', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        file: 'README.md',
        content: '<h1 id="readme">README</h1><p>Alpha note appears here.</p>',
        toc: '<ul><li><a href=\"#readme\">README</a></li></ul>'
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('initial.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="initial">Initial</h1><p>Placeholder body.</p>',
    toc: '<ul><li><a href="#initial">Initial</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'note');
  await expect(page.locator('#document-search-summary')).toHaveText('0 / 2 件');

  await page.evaluate(() => {
    window.markdownViewTestHooks.moveDocumentSearch(1);
  });

  await expect(page).toHaveURL(/file=README\.md/);
  await expect(page.locator('#content')).toContainText('Alpha note appears here.');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 2 件');
  await expect(page.locator('#document-search-results .document-search-result').first()).toHaveClass(/active/);
});

test('ディレクトリ検索結果をクリックすると対象ファイルを開いて一致箇所へ移動する', async ({ page }) => {
  let searchCallCount = 0;
  const notesParagraphs = Array.from({ length: 40 }, (_, index) => {
    if (index === 32) {
      return '<p id="target-match">Notes body appears in this document.</p>';
    }
    return `<p>Filler line ${index + 1}</p>`;
  }).join('');
  await page.route('**/api/search**', async (route) => {
    searchCallCount += 1;
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'notes body',
        results: searchCallCount === 1 ? [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'README notes body appears first.',
            after: ''
          },
          {
            file: 'notes.md',
            file_match_index: 0,
            before: '',
            current: 'Notes body appears in this document.',
            after: ''
          }
        ] : [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'README notes body appears first.',
            after: ''
          },
          {
            file: 'notes.md',
            file_match_index: 0,
            before: '',
            current: 'Notes body appears in this document.',
            after: ''
          }
        ],
        searched_files: 2,
        skipped_files: 0
      })
    });
  });
  await page.route('**/api/content?file=notes.md', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        content: '<h1 id="notes">Notes</h1>' + notesParagraphs,
        toc: '<ul><li><a href="#notes">Notes</a></li></ul>',
        file: 'notes.md'
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Initial README content.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'notes body');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(2);

  await page.locator('#document-search-results .document-search-result').nth(1).click();

  await expect(page.locator('#content')).toContainText('Notes body appears in this document.');
  await expect(page.locator('#document-search-summary')).toHaveText('2 / 2 件');
  await expect.poll(() => currentMatchText(page)).toContain('Notes body');
  await expect.poll(() => searchCallCount).toBe(1);
  await expect(page).toHaveURL(/file=notes\.md/);
  await expect(page.locator('#document-search-results .document-search-result').nth(1)).toHaveClass(/active/);
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0);
});

test('ディレクトリ検索結果のオープン失敗時は以前の選択状態を復元する', async ({ page }) => {
  await page.route('**/api/search**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'alpha note',
        results: [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'Alpha note appears here.',
            after: ''
          },
          {
            file: 'missing.md',
            file_match_index: 0,
            before: '',
            current: 'Missing alpha note.',
            after: ''
          }
        ],
        searched_files: 2,
        skipped_files: 0
      })
    });
  });
  await page.route('**/api/content?file=missing.md', async (route) => {
    await route.fulfill({
      status: 404,
      contentType: 'application/json',
      body: JSON.stringify({ error: 'not found' })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha note appears here.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(2);

  await page.locator('#document-search-results .document-search-result').first().click();
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 2 件');

  await page.locator('#document-search-results .document-search-result').nth(1).click();

  await expect(page.locator('#file-fetch-error-banner')).toContainText('指定したファイルが見つかりません。');
  await expect(page).toHaveURL(/file=README\.md/);
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 2 件');
  await expect(page.locator('#document-search-results .document-search-result').first()).toHaveClass(/active/);
  await expect(page.locator('#document-search-results .document-search-result').nth(1)).not.toHaveClass(/active/);
});

test('ディレクトリモードではlive update後に検索結果一覧を再取得する', async ({ page }) => {
  let searchCallCount = 0;
  await page.route('**/api/search**', async (route) => {
    searchCallCount += 1;
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'alpha note',
        results: searchCallCount === 1 ? [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'Alpha note appears here.',
            after: ''
          }
        ] : [
          {
            file: 'README.md',
            file_match_index: 0,
            before: '',
            current: 'Alpha note appears here.',
            after: ''
          },
          {
            file: 'README.md',
            file_match_index: 1,
            before: '',
            current: 'Alpha note appears after update.',
            after: ''
          }
        ],
        searched_files: 1,
        skipped_files: 0
      })
    });
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha note appears here.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(1);

  await updateContent(page, {
    content:
      '<h1 id="readme">README</h1>' +
      '<p>Alpha note appears here.</p>' +
      '<p>Alpha note appears after update.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(2);
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 2 件');
});

test('ディレクトリモードでは古い検索失敗で新しいクエリのエラー表示に切り替わらない', async ({ page }) => {
  let firstRequestStarted = false;
  await page.route('**/api/search**', async (route) => {
    const url = new URL(route.request().url());
    const query = url.searchParams.get('q');

    if (query === 'alpha') {
      firstRequestStarted = true;
      await new Promise((resolve) => setTimeout(resolve, 150));
      await route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'stale failure' })
      });
      return;
    }

    if (query === 'beta') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          query: 'beta',
          results: [
            {
              file: 'README.md',
              file_match_index: 0,
              before: '',
              current: 'Beta result is visible.',
              after: ''
            }
          ],
          searched_files: 1,
          skipped_files: 0
        })
      });
      return;
    }

    await route.fallback();
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>Alpha result is visible.</p><p>Beta result is visible.</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha');
  await expect.poll(() => firstRequestStarted).toBe(true);

  await setDocumentSearchQuery(page, 'beta');

  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(1);
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 1 件');
  await expect(page.locator('#document-search-results .document-search-result').first())
    .toContainText('Beta result is visible.');
  await expect(page.locator('#document-search-results')).not.toContainText('サーバー内部エラーが発生しました。');
});

test('ディレクトリ検索の古い応答は現在queryへ適用されない', async ({ page }) => {
  let firstRequestStarted = false;
  let releaseFirstResponse!: () => void;

  await page.route('**/api/search**', async (route) => {
    const url = new URL(route.request().url());
    const query = url.searchParams.get('q');

    if (query === 'alpha') {
      firstRequestStarted = true;
      await new Promise<void>((resolve) => {
        releaseFirstResponse = resolve;
      });
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          query: 'alpha',
          results: [{ file: 'notes.md', line: 1, before: '', current: 'alpha old', after: '', file_match_index: 0 }],
          skipped_files: 0,
          truncated: false,
          truncated_reasons: []
        })
      });
      return;
    }

    if (query === 'beta') {
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          query: 'beta',
          results: [{ file: 'README.md', line: 1, before: '', current: 'beta current', after: '', file_match_index: 0 }],
          skipped_files: 0,
          truncated: false,
          truncated_reasons: []
        })
      });
      return;
    }

    await route.fallback();
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>alpha text</p><p>beta text</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha');
  await expect.poll(() => firstRequestStarted).toBe(true);

  await setDocumentSearchQuery(page, 'beta');
  await expect(page.locator('#document-search-results')).toContainText('beta current');

  const alphaResponse = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return url.pathname === '/api/search' && url.searchParams.get('q') === 'alpha';
  });
  releaseFirstResponse();
  await alphaResponse;
  await page.evaluate(() => new Promise(requestAnimationFrame));
  await expect(page.locator('#document-search-results')).toContainText('beta current');
  await expect(page.locator('#document-search-results')).not.toContainText('alpha old');
});
