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

async function clearSelection(page) {
  await page.evaluate(() => {
    const selection = window.getSelection();
    selection.removeAllRanges();
    document.dispatchEvent(new Event('selectionchange'));
  });
}

test.beforeEach(async ({ page }) => {
  await resetFixtures();
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    const nativeSetTimeout = window.setTimeout.bind(window);

    class TestWebSocket extends NativeWebSocket {
      constructor(...args) {
        super(...args);
        window.__lastWs = this;
      }
    }

    TestWebSocket.prototype = NativeWebSocket.prototype;
    Object.setPrototypeOf(TestWebSocket, NativeWebSocket);
    window.WebSocket = TestWebSocket;
    window.setTimeout = (fn, delay, ...args) => {
      const effectiveDelay = delay === 30000 ? 50 : delay;
      return nativeSetTimeout(fn, effectiveDelay, ...args);
    };
    window.__dispatchWsMessage = (payload) => {
      if (!window.__lastWs || typeof window.__lastWs.onmessage !== 'function') {
        throw new Error('WebSocket is not ready');
      }
      window.__lastWs.onmessage({ data: JSON.stringify(payload) });
    };
  });
  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
});

test('ドラッグ選択中はWebSocket更新を延期し、選択解除後に適用する', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');

  await page.evaluate(() => {
    window.__dispatchWsMessage({
      content: '<h1 id="readme">README</h1><p>Deferred update</p>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>'
    });
  });

  await expect(page.locator('#content')).toContainText('Initial README content');
  await expect(page.locator('#content')).not.toContainText('Deferred update');

  await clearSelection(page);
  await expect(page.locator('#content')).toContainText('Deferred update');
});

test('ファイル遷移時は保留更新をクリアし、新しいファイル内容を維持する', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');

  await page.evaluate(() => {
    window.__dispatchWsMessage({
      content: '<h1 id="readme">README</h1><p>Pending stale update</p>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>',
      file: 'README.md'
    });
  });

  await page.locator('[data-file="notes.md"]').click();
  await expect(page.locator('#content')).toContainText('Notes body');

  await clearSelection(page);
  await page.waitForTimeout(100);
  await expect(page.locator('#content')).toContainText('Notes body');
  await expect(page.locator('#content')).not.toContainText('Pending stale update');
});

test('refreshメッセージも選択中は延期し、解除後に再取得する', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');
  await fs.writeFile(readmePath, '# README\n\nRefreshed from server\n');

  await page.evaluate(() => {
    window.__dispatchWsMessage({ refresh: true });
  });

  await expect(page.locator('#content')).toContainText('Initial README content');
  await expect(page.locator('#content')).not.toContainText('Refreshed from server');

  await clearSelection(page);
  await expect(page.locator('#content')).toContainText('Refreshed from server');
});

test('選択解除されなくても30秒フォールバックで保留更新を適用する', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');

  await page.evaluate(() => {
    window.__dispatchWsMessage({
      content: '<h1 id="readme">README</h1><p>Fallback applied</p>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>'
    });
  });

  await expect(page.locator('#content')).toContainText('Fallback applied');
});
