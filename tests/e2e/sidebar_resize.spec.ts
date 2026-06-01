import { spawn, type ChildProcess } from 'node:child_process';
import net from 'node:net';
import { test, expect } from '@playwright/test';
import { resetStandardFixtures } from './helpers';

async function getUnusedPort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        server.close(() => reject(new Error('failed to allocate port')));
        return;
      }
      server.close(() => resolve(address.port));
    });
  });
}

async function waitForSingleFileServer(port: number, server: ChildProcess): Promise<void> {
  const url = `http://127.0.0.1:${port}/`;
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    if (server.exitCode !== null) {
      throw new Error(`single file server exited early: ${server.exitCode}`);
    }
    try {
      const response = await fetch(url);
      await response.arrayBuffer();
      if (response.ok) return;
    } catch (error) {}
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error('single file server did not become ready');
}

async function stopServer(server: ChildProcess): Promise<void> {
  if (server.exitCode !== null || server.killed) return;
  await new Promise<void>((resolve) => {
    server.once('exit', () => resolve());
    server.kill();
    setTimeout(resolve, 5000);
  });
}

test.beforeEach(async ({ page }) => {
  await resetStandardFixtures();
  await page.goto('/');
  await expect(page.locator('#sidebar')).toBeVisible();
});

test.afterEach(async () => {
  await resetStandardFixtures();
});

test('サイドバー幅はドラッグで伸び縮みできる', async ({ page }) => {
  const sidebar = page.locator('#sidebar');
  const handle = page.locator('#sidebar-width-resizer');
  const initialBox = await sidebar.boundingBox();
  const handleBox = await handle.boundingBox();

  expect(initialBox).not.toBeNull();
  expect(handleBox).not.toBeNull();

  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y + 80);
  await page.mouse.down();
  await page.mouse.move(initialBox!.width + 90, handleBox!.y + 80);
  await page.mouse.up();

  await expect.poll(async () => (await sidebar.boundingBox())?.width ?? 0).toBeGreaterThan(initialBox!.width + 60);

  const expandedBox = await sidebar.boundingBox();
  const expandedHandleBox = await handle.boundingBox();
  expect(expandedHandleBox).not.toBeNull();
  await page.mouse.move(expandedHandleBox!.x + expandedHandleBox!.width / 2, expandedHandleBox!.y + 80);
  await page.mouse.down();
  await page.mouse.move(initialBox!.width - 40, handleBox!.y + 80);
  await page.mouse.up();

  await expect.poll(async () => (await sidebar.boundingBox())?.width ?? 0).toBeLessThan(expandedBox!.width - 60);
});

test('サイドバー内部コンテンツエリアはドラッグで伸び縮みできる', async ({ page }) => {
  const panel = page.locator('#panel-files');
  const content = panel.locator('.sidebar-resizable-content');
  const handle = panel.locator('.sidebar-content-resizer');
  const panelBox = await panel.boundingBox();
  const handleBox = await handle.boundingBox();

  expect(panelBox).not.toBeNull();
  expect(handleBox).not.toBeNull();

  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y + handleBox!.height / 2);
  await page.mouse.down();
  await page.mouse.move(handleBox!.x + handleBox!.width / 2, panelBox!.y + 180);
  await page.mouse.up();

  await expect.poll(async () => (await content.boundingBox())?.height ?? 0).toBeLessThan(220);

  const shrunkenBox = await content.boundingBox();
  const shrunkenHandleBox = await handle.boundingBox();
  expect(shrunkenHandleBox).not.toBeNull();
  await page.mouse.move(shrunkenHandleBox!.x + shrunkenHandleBox!.width / 2, shrunkenHandleBox!.y + shrunkenHandleBox!.height / 2);
  await page.mouse.down();
  await page.mouse.move(shrunkenHandleBox!.x + shrunkenHandleBox!.width / 2, panelBox!.y + 260);
  await page.mouse.up();

  await expect.poll(async () => (await content.boundingBox())?.height ?? 0).toBeGreaterThan((shrunkenBox?.height ?? 0) + 50);
});

test('内部リサイズハンドルは初期表示とタブ切替後にARIA値を公開する', async ({ page }) => {
  const filesHandle = page.locator('#panel-files .sidebar-content-resizer');

  await expect(filesHandle).toHaveAttribute('aria-valuemin', '128');
  await expect(filesHandle).toHaveAttribute('aria-valuenow', /\d+/);
  await expect(filesHandle).toHaveAttribute('aria-valuemax', /\d+/);

  await page.locator('.sidebar-tab[data-tab="toc"]').click();
  const tocHandle = page.locator('#panel-toc .sidebar-content-resizer');
  await expect(page.locator('#panel-toc')).toHaveClass(/active/);
  await expect(tocHandle).toHaveAttribute('aria-valuemin', '128');
  await expect(tocHandle).toHaveAttribute('aria-valuenow', /\d+/);
  await expect(tocHandle).toHaveAttribute('aria-valuemax', /\d+/);
});

test('リサイズハンドルはキーボード操作と境界clampを反映する', async ({ page }) => {
  const sidebar = page.locator('#sidebar');
  const widthHandle = page.locator('#sidebar-width-resizer');
  const content = page.locator('#panel-files .sidebar-resizable-content');
  const contentHandle = page.locator('#panel-files .sidebar-content-resizer');

  await widthHandle.focus();
  const initialWidth = (await sidebar.boundingBox())!.width;
  await page.keyboard.press('ArrowRight');
  await expect.poll(async () => (await sidebar.boundingBox())?.width ?? 0).toBeGreaterThan(initialWidth);
  await expect(widthHandle).toHaveAttribute('aria-valuenow', /\d+/);

  for (let i = 0; i < 20; i++) {
    await page.keyboard.press('ArrowLeft');
  }
  await expect.poll(async () => Math.round((await sidebar.boundingBox())?.width ?? 0)).toBe(260);
  await expect(widthHandle).toHaveAttribute('aria-valuenow', '260');

  const maxWidth = Number(await widthHandle.getAttribute('aria-valuemax'));
  for (let i = 0; i < 30; i++) {
    await page.keyboard.press('ArrowRight');
  }
  await expect.poll(async () => Math.round((await sidebar.boundingBox())?.width ?? 0)).toBe(maxWidth);
  await expect(widthHandle).toHaveAttribute('aria-valuenow', String(maxWidth));

  await contentHandle.focus();
  const initialHeight = (await content.boundingBox())!.height;
  await page.keyboard.press('ArrowUp');
  await expect.poll(async () => (await content.boundingBox())?.height ?? 0).toBeLessThan(initialHeight);

  for (let i = 0; i < 30; i++) {
    await page.keyboard.press('ArrowUp');
  }
  await expect.poll(async () => Math.round((await content.boundingBox())?.height ?? 0)).toBe(128);
  await expect(contentHandle).toHaveAttribute('aria-valuenow', '128');

  const maxHeight = Number(await contentHandle.getAttribute('aria-valuemax'));
  for (let i = 0; i < 40; i++) {
    await page.keyboard.press('ArrowDown');
  }
  await expect.poll(async () => Math.round((await content.boundingBox())?.height ?? 0)).toBe(maxHeight);
  await expect(contentHandle).toHaveAttribute('aria-valuenow', String(maxHeight));
});

test('モバイル幅ではリサイズハンドルを非表示にする', async ({ page }) => {
  await page.setViewportSize({ width: 768, height: 720 });

  await expect(page.locator('#sidebar-width-resizer')).toBeHidden();
  await expect(page.locator('#panel-files .sidebar-content-resizer')).toBeHidden();
});

test('pointer capture開始失敗時もリサイズ状態を残さない', async ({ page }) => {
  const result = await page.evaluate(() => {
    const sidebar = document.getElementById('sidebar');
    const handle = document.getElementById('sidebar-width-resizer');
    if (!sidebar || !handle) {
      throw new Error('sidebar width handle not found');
    }
    const beforeWidth = sidebar.getBoundingClientRect().width;
    const beforeAria = handle.getAttribute('aria-valuenow');
    handle.setPointerCapture = function(): void {
      throw new DOMException('missing pointer', 'NotFoundError');
    };
    handle.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true,
      clientX: beforeWidth + 120,
      pointerId: 99
    }));

    return {
      beforeAria,
      beforeWidth,
      afterAria: handle.getAttribute('aria-valuenow'),
      afterWidth: sidebar.getBoundingClientRect().width
    };
  });

  await expect(page.locator('#sidebar')).not.toHaveClass(/resizing/);
  expect(result.afterWidth).toBe(result.beforeWidth);
  expect(result.afterAria).toBe(result.beforeAria);
});

test('内部リサイズのpointer capture開始失敗時もリサイズ状態を残さない', async ({ page }) => {
  const result = await page.evaluate(() => {
    const handle = document.querySelector<HTMLElement>('#panel-files .sidebar-content-resizer');
    const content = document.querySelector<HTMLElement>('#panel-files .sidebar-resizable-content');
    if (!handle || !content) {
      throw new Error('sidebar content handle not found');
    }
    const beforeHeight = content.getBoundingClientRect().height;
    const beforeAria = handle.getAttribute('aria-valuenow');
    handle.setPointerCapture = function(): void {
      throw new DOMException('missing pointer', 'NotFoundError');
    };
    handle.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true,
      clientY: beforeHeight + 120,
      pointerId: 100
    }));

    return {
      beforeAria,
      beforeHeight,
      afterAria: handle.getAttribute('aria-valuenow'),
      afterHeight: content.getBoundingClientRect().height
    };
  });

  await expect(page.locator('#sidebar')).not.toHaveClass(/resizing/);
  expect(result.afterHeight).toBe(result.beforeHeight);
  expect(result.afterAria).toBe(result.beforeAria);
});

test('pointer capture喪失時に幅と内部リサイズの状態を片付ける', async ({ page }) => {
  const widthResult = await page.evaluate(() => {
    const sidebar = document.getElementById('sidebar');
    const handle = document.getElementById('sidebar-width-resizer');
    if (!sidebar || !handle) {
      throw new Error('sidebar width handle not found');
    }
    handle.setPointerCapture = function(): void {};
    handle.hasPointerCapture = function(): boolean { return true; };
    handle.releasePointerCapture = function(): void {};
    handle.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true,
      clientX: 320,
      pointerId: 101
    }));
    if (!sidebar.classList.contains('resizing')) {
      throw new Error('width resize did not start');
    }
    handle.dispatchEvent(new PointerEvent('lostpointercapture', {
      bubbles: true,
      pointerId: 101
    }));
    const widthAfterLostCapture = sidebar.getBoundingClientRect().width;
    const ariaAfterLostCapture = handle.getAttribute('aria-valuenow');
    handle.dispatchEvent(new PointerEvent('pointermove', {
      bubbles: true,
      clientX: widthAfterLostCapture + 160,
      pointerId: 101
    }));

    return {
      ariaAfterLostCapture,
      ariaAfterPointerMove: handle.getAttribute('aria-valuenow'),
      widthAfterLostCapture,
      widthAfterPointerMove: sidebar.getBoundingClientRect().width
    };
  });
  await expect(page.locator('#sidebar')).not.toHaveClass(/resizing/);
  expect(widthResult.widthAfterPointerMove).toBe(widthResult.widthAfterLostCapture);
  expect(widthResult.ariaAfterPointerMove).toBe(widthResult.ariaAfterLostCapture);

  const contentResult = await page.evaluate(() => {
    const sidebar = document.getElementById('sidebar');
    const handle = document.querySelector<HTMLElement>('#panel-files .sidebar-content-resizer');
    const panel = document.getElementById('panel-files');
    const content = document.querySelector<HTMLElement>('#panel-files .sidebar-resizable-content');
    if (!sidebar || !handle || !panel || !content) {
      throw new Error('sidebar content handle not found');
    }
    handle.setPointerCapture = function(): void {};
    handle.hasPointerCapture = function(): boolean { return true; };
    handle.releasePointerCapture = function(): void {};
    handle.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true,
      clientY: panel.getBoundingClientRect().top + 180,
      pointerId: 102
    }));
    if (!sidebar.classList.contains('resizing')) {
      throw new Error('content resize did not start');
    }
    handle.dispatchEvent(new PointerEvent('lostpointercapture', {
      bubbles: true,
      pointerId: 102
    }));
    const heightAfterLostCapture = content.getBoundingClientRect().height;
    const ariaAfterLostCapture = handle.getAttribute('aria-valuenow');
    handle.dispatchEvent(new PointerEvent('pointermove', {
      bubbles: true,
      clientY: panel.getBoundingClientRect().top + heightAfterLostCapture + 120,
      pointerId: 102
    }));

    return {
      ariaAfterLostCapture,
      ariaAfterPointerMove: handle.getAttribute('aria-valuenow'),
      heightAfterLostCapture,
      heightAfterPointerMove: content.getBoundingClientRect().height
    };
  });
  await expect(page.locator('#sidebar')).not.toHaveClass(/resizing/);
  expect(contentResult.heightAfterPointerMove).toBe(contentResult.heightAfterLostCapture);
  expect(contentResult.ariaAfterPointerMove).toBe(contentResult.ariaAfterLostCapture);
});

test('単一ファイルモードでも内部リサイズを操作できる', async ({ page }) => {
  const port = await getUnusedPort();
  const server = spawn('cargo', [
    'run',
    '--',
    'tests/fixtures/e2e/README.md',
    '--port',
    String(port),
    '--no-open'
  ], {
    cwd: process.cwd(),
    stdio: 'ignore'
  });

  try {
    await waitForSingleFileServer(port, server);
    await page.goto(`http://127.0.0.1:${port}/`);
    await expect(page.locator('#sidebar')).toBeVisible();
    await expect(page.locator('#panel-files')).toHaveCount(0);

    const tocPanel = page.locator('#panel-toc');
    const tocContent = tocPanel.locator('.sidebar-resizable-content');
    const tocHandle = tocPanel.locator('.sidebar-content-resizer');
    await expect(tocPanel).toHaveClass(/active/);
    await expect(tocHandle).toHaveAttribute('aria-valuemin', '128');
    await expect(tocHandle).toHaveAttribute('aria-valuenow', /\d+/);
    await expect(tocHandle).toHaveAttribute('aria-valuemax', /\d+/);

    await tocHandle.focus();
    const initialTocHeight = (await tocContent.boundingBox())!.height;
    await page.keyboard.press('ArrowUp');
    await expect.poll(async () => (await tocContent.boundingBox())?.height ?? 0).toBeLessThan(initialTocHeight);

    await page.locator('.sidebar-tab[data-tab="memo"]').click();
    const memoPanel = page.locator('#panel-memo');
    const memoContent = memoPanel.locator('.sidebar-resizable-content');
    const memoHandle = memoPanel.locator('.sidebar-content-resizer');
    await expect(memoPanel).toHaveClass(/active/);
    await expect(memoHandle).toHaveAttribute('aria-valuemin', '128');
    await expect(memoHandle).toHaveAttribute('aria-valuenow', /\d+/);
    await expect(memoHandle).toHaveAttribute('aria-valuemax', /\d+/);

    const memoPanelBox = await memoPanel.boundingBox();
    const memoHandleBox = await memoHandle.boundingBox();
    expect(memoPanelBox).not.toBeNull();
    expect(memoHandleBox).not.toBeNull();
    await page.mouse.move(memoHandleBox!.x + memoHandleBox!.width / 2, memoHandleBox!.y + memoHandleBox!.height / 2);
    await page.mouse.down();
    await page.mouse.move(memoHandleBox!.x + memoHandleBox!.width / 2, memoPanelBox!.y + 180);
    await page.mouse.up();

    await expect.poll(async () => (await memoContent.boundingBox())?.height ?? 0).toBeLessThan(220);
  } finally {
    await stopServer(server);
  }
});
