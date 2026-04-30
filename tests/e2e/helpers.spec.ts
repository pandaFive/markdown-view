import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect } from '@playwright/test';
import { installTestWebSocketHarness } from './browser/test-websocket';
import {
  dispatchWsMessage,
  dispatchWsMessageAndDisableRealHandler,
  dispatchWsMessages,
  resetStandardFixtures,
  selectParagraphText,
  stabilizeWebSocketHarness,
  updateContent
} from './helpers';

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.__MV_E2E__ = true;
  });
  await resetStandardFixtures();
  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
});

test.afterEach(async () => {
  await resetStandardFixtures();
});

test('selectParagraphTextは完全一致の一意候補だけを選択する', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');

  const selectedText = await page.evaluate(() => window.getSelection()?.toString() ?? '');
  expect(selectedText).toBe('Initial README content');
});

test('selectParagraphTextは既定で部分一致を採用しない', async ({ page }) => {
  await expect(selectParagraphText(page, 'README content')).rejects.toThrow(/text not found: README content/);
});

test('selectParagraphTextは明示された部分一致なら選択できる', async ({ page }) => {
  await selectParagraphText(page, 'README content', { match: 'contains' });

  const selectedText = await page.evaluate(() => window.getSelection()?.toString() ?? '');
  expect(selectedText).toBe('Initial README content');
});

test('selectParagraphTextは複数候補の部分一致を曖昧として失敗させる', async ({ page }) => {
  await updateContent(page, {
    content: '<h1 id="readme">README</h1><p>duplicate target alpha</p><p>duplicate target beta</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await expect(selectParagraphText(page, 'duplicate target', { match: 'contains' }))
    .rejects.toThrow(/ambiguous text match: duplicate target \(2 matches\)/);
});

test('selectParagraphTextは完全一致でも複数候補を曖昧として失敗させる', async ({ page }) => {
  await updateContent(page, {
    content: '<h1 id="readme">README</h1><p>repeated target</p><p>repeated target</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await expect(selectParagraphText(page, 'repeated target'))
    .rejects.toThrow(/ambiguous text match: repeated target \(2 matches\)/);
});

test('resetStandardFixturesはmemo artifactと設定ディレクトリを削除する', async () => {
  await fs.writeFile(path.join(fixtureDir, '.README.md.memo.md'), 'stale memo');
  await fs.mkdir(path.join(fixtureDir, '.markdown-view'), { recursive: true });
  await fs.writeFile(path.join(fixtureDir, '.markdown-view', 'state.json'), '{}');

  await resetStandardFixtures();

  await expect(fs.access(path.join(fixtureDir, '.README.md.memo.md'))).rejects.toThrow();
  await expect(fs.access(path.join(fixtureDir, '.markdown-view'))).rejects.toThrow();
});

test('resetStandardFixturesはcleanupMemoArtifacts falseならartifact検査も行わない', async () => {
  await fs.writeFile(path.join(fixtureDir, '.README.md.memo.md'), 'kept memo');
  await fs.mkdir(path.join(fixtureDir, '.markdown-view'), { recursive: true });
  await fs.writeFile(path.join(fixtureDir, '.markdown-view', 'state.json'), '{}');

  await resetStandardFixtures({ cleanupMemoArtifacts: false });

  await expect(fs.access(path.join(fixtureDir, '.README.md.memo.md'))).resolves.toBeUndefined();
  await expect(fs.access(path.join(fixtureDir, '.markdown-view'))).resolves.toBeUndefined();
});

test('resetStandardFixturesは削除失敗時に対象パスを含むエラーを返す', async () => {
  test.skip(process.platform === 'win32', 'POSIX permission semantics are required for this fixture cleanup test');

  const markdownViewPath = path.join(fixtureDir, '.markdown-view');
  await fs.mkdir(markdownViewPath, { recursive: true });
  await fs.writeFile(path.join(markdownViewPath, 'state.json'), '{}');
  await fs.chmod(markdownViewPath, 0o500);

  try {
    await expect(resetStandardFixtures())
      .rejects.toThrow(/fixture cleanup failed for \.markdown-view/);
  } finally {
    await fs.chmod(markdownViewPath, 0o700).catch(() => {});
    await fs.rm(markdownViewPath, { recursive: true, force: true });
  }
});

test('WebSocket dispatchはstale bridgeを失敗させる', async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.reload();
  await stabilizeWebSocketHarness(page);

  await page.evaluate(() => {
    window.__lastWs = {
      onmessage: function() {},
      close: function() {},
      send: function() {},
      readyState: WebSocket.OPEN
    } as unknown as MvE2E.TestWebSocketInstance;
  });

  await expect(dispatchWsMessage(page, {
    content: '<h1 id="readme">README</h1><p>stale update</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  })).rejects.toThrow(/WebSocket test harness bridge is stale/);
});

test('WebSocket連続dispatchはstale bridgeを失敗させる', async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.reload();
  await stabilizeWebSocketHarness(page);

  await page.evaluate(() => {
    window.__lastWs = {
      onmessage: function() {},
      close: function() {},
      send: function() {},
      readyState: WebSocket.OPEN
    } as unknown as MvE2E.TestWebSocketInstance;
  });

  await expect(dispatchWsMessages(page, [{
    content: '<h1 id="readme">README</h1><p>stale update</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  }])).rejects.toThrow(/WebSocket test harness bridge is stale/);
});

test('WebSocket handler無効化付きdispatchはstale bridgeを失敗させる', async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.reload();
  await stabilizeWebSocketHarness(page);

  await page.evaluate(() => {
    window.__lastWs = {
      onmessage: function() {},
      close: function() {},
      send: function() {},
      readyState: WebSocket.OPEN
    } as unknown as MvE2E.TestWebSocketInstance;
  });

  await expect(dispatchWsMessageAndDisableRealHandler(page, {
    content: '<h1 id="readme">README</h1><p>stale update</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  })).rejects.toThrow(/WebSocket test harness bridge is stale/);
});

test('WebSocket harnessは同じ接続の再安定化でも実handlerを保持する', async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.reload();
  await stabilizeWebSocketHarness(page);
  await stabilizeWebSocketHarness(page);

  await dispatchWsMessage(page, {
    content: '<h1 id="readme">README</h1><p>double stabilize update</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await expect(page.locator('#content')).toContainText('double stabilize update');
});

test('WebSocket harnessは再接続後に再安定化すればdispatchできる', async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.reload();
  await stabilizeWebSocketHarness(page);

  await page.evaluate(() => {
    window.__lastWs = {
      onmessage: function(ev: MessageEvent) {
        const hooks = window.markdownViewTestHooks;
        if (!hooks) {
          throw new Error('window.markdownViewTestHooks is not exposed for E2E');
        }
        hooks.updateContent(JSON.parse(ev.data as string));
      },
      close: function() {},
      send: function() {},
      readyState: WebSocket.OPEN
    } as unknown as MvE2E.TestWebSocketInstance;
  });

  await stabilizeWebSocketHarness(page);
  await dispatchWsMessage(page, {
    content: '<h1 id="readme">README</h1><p>reconnected update</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await expect(page.locator('#content')).toContainText('reconnected update');
});

test('WebSocketが不正JSONを受信したら接続を閉じて再接続経路に入る', async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  await page.reload();
  await stabilizeWebSocketHarness(page);

  const parseResult = await page.evaluate(() => {
    const lastWs = window.__lastWs;
    const realWsOnmessage = window.__realWsOnmessage;
    if (!lastWs || !realWsOnmessage) {
      throw new Error('WebSocket test harness is not initialized');
    }
    const originalClose = lastWs.close.bind(lastWs);
    window.__wsCloseCalls = 0;
    lastWs.close = function(...args: Parameters<WebSocket['close']>) {
      window.__wsCloseCalls = (window.__wsCloseCalls ?? 0) + 1;
      return originalClose(...args);
    };

    realWsOnmessage({ data: '{invalid json' });
    return {
      closeCalls: window.__wsCloseCalls ?? 0,
      liveState: document.getElementById('live-status')?.dataset.state ?? '',
      bannerText: document.getElementById('ws-parse-error-banner')?.textContent ?? ''
    };
  });

  expect(parseResult).toEqual(expect.objectContaining({
    closeCalls: 1,
    liveState: 'error'
  }));
  expect(parseResult.bannerText).toContain('不正なJSON');
});
