const { test, expect } = require('@playwright/test');

function searchFixtureContent() {
  return (
    '<h1 id="readme">README</h1>' +
    '<p>Alpha note appears here.</p>' +
    '<h2 id="details">Details</h2>' +
    '<p>Alpha note appears again in the details section.</p>' +
    '<h3 id="deep-dive">Deep dive</h3>' +
    '<p>alpha note appears a third time in lowercase.</p>'
  );
}

function searchFixtureToc() {
  return (
    '<ul>' +
    '<li><a href="#readme">README</a></li>' +
    '<li><a href="#details">Details</a></li>' +
    '<li><a href="#deep-dive">Deep dive</a></li>' +
    '</ul>'
  );
}

async function loadSearchFixture(page) {
  await page.evaluate(({ content, toc }) => {
    updateContent({ content, toc });
    activateSidebarTab('toc');
  }, {
    content: searchFixtureContent(),
    toc: searchFixtureToc()
  });
  await expect(page.locator('#document-search-input')).toBeVisible();
  await expect(page.locator('#content')).toContainText('Alpha note appears here.');
}

async function visibleMatchCount(page) {
  return page.evaluate(() => {
    return new Set(
      Array.from(document.querySelectorAll('#content mark.document-search-match')).map((mark) => mark.dataset.matchId)
    ).size;
  });
}

async function currentMatchText(page) {
  return page.evaluate(() => {
    return Array.from(document.querySelectorAll('#content mark.document-search-match.current'))
      .map((mark) => mark.textContent || '')
      .join('');
  });
}

async function stabilizeWebSocketHarness(page) {
  await page.waitForFunction(() => window.__lastWs && typeof window.__lastWs.onmessage === 'function');
  await page.evaluate(() => {
    window.__realWsOnmessage = window.__lastWs.onmessage;
    window.__lastWs.onmessage = function() {};
  });
}

async function setDocumentSearchQuery(page, query) {
  await page.evaluate((value) => {
    const input = document.getElementById('document-search-input');
    if (!input) {
      throw new Error('document search input not found');
    }
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
  }, query);
}

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;

    class TestWebSocket extends NativeWebSocket {
      constructor(...args) {
        super(...args);
        window.__lastWs = this;
      }
    }

    TestWebSocket.prototype = NativeWebSocket.prototype;
    Object.setPrototypeOf(TestWebSocket, NativeWebSocket);
    window.WebSocket = TestWebSocket;
  });
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
  await page.evaluate(() => {
    updateContent({
      content:
        '<h1 id="readme">README</h1>' +
        '<p>Alpha note appears here.</p>' +
        '<p><a href="https://example.com">alpha note link</a></p>' +
        '<pre class="code-block"><code>alpha note code</code></pre>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>'
    });
    activateSidebarTab('toc');
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
  await page.evaluate(() => {
    updateContent({
      content:
        '<h1 id="readme">README</h1>' +
        '<p>Run <code>cargo test</code> after editing.</p>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>'
    });
    activateSidebarTab('toc');
  });

  await setDocumentSearchQuery(page, 'cargo test');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 1 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(1);
  await expect.poll(() => currentMatchText(page)).toContain('cargo test');
});

test('装飾をまたぐ語句も検索できる', async ({ page }) => {
  await page.evaluate(() => {
    updateContent({
      content:
        '<h1 id="readme">README</h1>' +
        '<p>Alpha <strong>note</strong> appears across formatting.</p>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>'
    });
    activateSidebarTab('toc');
  });

  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 1 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(1);
  await expect.poll(() => currentMatchText(page)).toContain('Alpha note');
});

test('EnterとShift+Enterで次前のヒットへ移動する', async ({ page }) => {
  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 3 件');

  await page.locator('#document-search-input').press('Enter');
  await expect(page.locator('#document-search-summary')).toHaveText('2 / 3 件');
  await expect(page.locator('#document-search-results .document-search-result').nth(1)).toHaveClass(/active/);

  await page.locator('#document-search-input').press('Shift+Enter');
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
    selectFile('notes.md');
  });

  await expect(page.locator('#content')).toContainText('Notes body');
  await expect(page.locator('#document-search-input')).toHaveValue('');
  await expect(page.locator('#document-search-summary')).toHaveText('0 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(0);
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(0);
});

test('ファイル切り替え失敗時は元文書の検索状態を維持する', async ({ page }) => {
  await setDocumentSearchQuery(page, 'alpha');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 3 件');

  await page.evaluate(() => {
    selectFile('missing.md');
  });

  await expect(page.locator('#file-fetch-error-banner')).toContainText('指定したファイルが見つかりません。');
  await expect(page.locator('#content')).toContainText('Alpha note appears here.');
  await expect(page.locator('#document-search-input')).toHaveValue('alpha');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 3 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(3);
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(3);
});

test('live update後も検索結果を再適用する', async ({ page }) => {
  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-summary')).toHaveText('1 / 3 件');

  await page.evaluate(() => {
    updateContent({
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
  });

  await expect(page.locator('#document-search-summary')).toHaveText('1 / 4 件');
  await expect.poll(() => visibleMatchCount(page)).toBe(4);
  await expect.poll(() => currentMatchText(page)).toContain('Alpha note');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(4);
});

test('検索結果一覧に前後文を表示してクリックで該当箇所へ移動する', async ({ page }) => {
  await page.evaluate(() => {
    updateContent({
      content:
        '<h1 id="readme">README</h1>' +
        '<p>Opening sentence. Alpha note appears here. Closing sentence.</p>' +
        '<p>Another intro. Alpha note appears again in the details section. Another ending.</p>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>'
    });
    activateSidebarTab('toc');
  });

  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(2);
  await expect(page.locator('#document-search-results .document-search-result').nth(0))
    .toContainText('Opening sentence. Alpha note appears here. Closing sentence.');
  await expect(page.locator('#document-search-results .document-search-result').nth(1))
    .toContainText('Another intro. Alpha note appears again in the details section. Another ending.');

  await page.evaluate(() => {
    const result = document.querySelectorAll('#document-search-results .document-search-result')[1];
    if (!result) {
      throw new Error('search result not found');
    }
    result.click();
  });
  await expect(page.locator('#document-search-summary')).toHaveText('2 / 2 件');
  await expect.poll(() => currentMatchText(page)).toContain('Alpha note');
  await expect(page.locator('#document-search-results .document-search-result').nth(1)).toHaveClass(/active/);
});

test('検索結果移動時に一覧のスクロール位置を維持する', async ({ page }) => {
  await page.evaluate(() => {
    const paragraphs = Array.from({ length: 18 }, (_, index) => `<p>Entry ${index + 1}. Alpha note appears in result ${index + 1}. Tail ${index + 1}.</p>`).join('');
    updateContent({
      content: '<h1 id="readme">README</h1>' + paragraphs,
      toc: '<ul><li><a href="#readme">README</a></li></ul>'
    });
    activateSidebarTab('toc');
  });

  await setDocumentSearchQuery(page, 'alpha note');
  await expect(page.locator('#document-search-results .document-search-result')).toHaveCount(18);

  const beforeScrollTop = await page.evaluate(() => {
    const results = document.getElementById('document-search-results');
    results.scrollTop = results.scrollHeight;
    return results.scrollTop;
  });

  await page.locator('#document-search-input').press('Enter');

  await expect.poll(() => page.evaluate(() => {
    return document.getElementById('document-search-results').scrollTop;
  })).toBe(beforeScrollTop);
});
