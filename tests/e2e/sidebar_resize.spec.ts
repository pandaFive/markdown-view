import { execFile, spawn, type ChildProcess } from 'node:child_process';
import { EventEmitter } from 'node:events';
import path from 'node:path';
import { promisify } from 'node:util';
import { test, expect } from '@playwright/test';
import { resetStandardFixtures } from './helpers';

const singleFileServerReadyTimeoutMs = 45000;
const singleFileServerStopTimeoutMs = 5000;
const singleFileServerOutputLimit = 8000;
const execFileAsync = promisify(execFile);

type SingleFileServerOutput = {
  spawnError: string;
  text: string;
};

type CommandError = Error & {
  stdout?: string | Buffer;
  stderr?: string | Buffer;
};

type SignalDelivery = {
  signal: NodeJS.Signals;
  delivered: boolean;
};

function sanitizeServerOutput(value: string): string {
  return value
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '')
    .replace(/[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]/g, '?');
}

function errorMessage(error: unknown): string {
  return sanitizeServerOutput(error instanceof Error ? error.message : String(error));
}

function appendServerOutput(output: SingleFileServerOutput, chunk: Buffer): void {
  output.text = (output.text + sanitizeServerOutput(chunk.toString('utf8'))).slice(-singleFileServerOutputLimit);
}

function summarizeOutputText(value: string): string {
  var sanitized = sanitizeServerOutput(value);
  if (!sanitized) return '(no output)';
  if (sanitized.length <= singleFileServerOutputLimit) return sanitized;
  return `[truncated to last ${singleFileServerOutputLimit} chars]\n${sanitized.slice(-singleFileServerOutputLimit)}`;
}

function serverOutputSummary(output: SingleFileServerOutput): string {
  return summarizeOutputText(output.text);
}

function fakeRunningServer(kill: () => boolean = () => true): ChildProcess {
  return Object.assign(new EventEmitter(), {
    exitCode: null,
    signalCode: null,
    pid: undefined,
    kill
  }) as unknown as ChildProcess;
}

function singleFileServerBinaryPath(): string {
  return path.join(process.cwd(), 'target', 'debug', process.platform === 'win32' ? 'markdown-view.exe' : 'markdown-view');
}

function commandOutputSummary(error: CommandError): string {
  return summarizeOutputText(`${error.stdout?.toString() || ''}${error.stderr?.toString() || ''}`);
}

async function buildSingleFileServerBinary(): Promise<string> {
  try {
    await execFileAsync('cargo', ['build', '--bin', 'markdown-view'], { cwd: process.cwd() });
  } catch (error) {
    if (error instanceof Error) {
      throw new Error(`single file server build failed: ${errorMessage(error)} output=${commandOutputSummary(error as CommandError)}`);
    }
    throw new Error(`single file server build failed: ${errorMessage(error)}`);
  }
  return singleFileServerBinaryPath();
}

async function waitForSingleFileServer(server: ChildProcess, output: SingleFileServerOutput): Promise<string> {
  const deadline = Date.now() + singleFileServerReadyTimeoutMs;
  let lastFetchError = '';
  while (Date.now() < deadline) {
    if (output.spawnError) {
      throw new Error(`single file server spawn failed: ${output.spawnError} output=${serverOutputSummary(output)}`);
    }
    if (server.exitCode !== null || server.signalCode !== null) {
      throw new Error(`single file server exited early: code=${server.exitCode ?? 'null'} signal=${server.signalCode ?? 'null'} output=${serverOutputSummary(output)}`);
    }
    const url = output.text.match(/URL:\s+(http:\/\/127\.0\.0\.1:\d+)/)?.[1];
    if (!url) {
      await new Promise((resolve) => setTimeout(resolve, 200));
      continue;
    }
    try {
      const response = await fetch(url);
      await response.arrayBuffer();
      if (response.ok) return url;
    } catch (error) {
      lastFetchError = error instanceof Error ? error.message : String(error);
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`single file server did not become ready: fetch=${lastFetchError || 'not attempted'} output=${serverOutputSummary(output)}`);
}

function hasServerExited(server: ChildProcess): boolean {
  return server.exitCode !== null || server.signalCode !== null;
}

function signalServer(server: ChildProcess, signal: NodeJS.Signals): SignalDelivery {
  if (hasServerExited(server)) return { signal, delivered: false };
  try {
    if (process.platform !== 'win32' && server.pid) {
      process.kill(-server.pid, signal);
      return { signal, delivered: true };
    }
    return { signal, delivered: server.kill(signal) };
  } catch (error) {
    if (error instanceof Error && 'code' in error && error.code === 'ESRCH') {
      return { signal, delivered: false };
    }
    if (!(error instanceof Error)) {
      throw error;
    }
    throw error;
  }
}

async function waitForServerExit(server: ChildProcess, timeoutMs: number): Promise<boolean> {
  if (hasServerExited(server)) return true;
  return new Promise((resolve) => {
    const timeout = setTimeout(() => {
      server.off('exit', onExit);
      resolve(false);
    }, timeoutMs);
    function onExit(): void {
      clearTimeout(timeout);
      resolve(true);
    }
    server.once('exit', onExit);
  });
}

function signalDeliverySummary(signals: SignalDelivery[]): string {
  return signals.map((entry) => `${entry.signal}=${entry.delivered}`).join(',');
}

function serverStopDiagnostics(server: ChildProcess, output: SingleFileServerOutput, signals: SignalDelivery[]): string {
  return `pid=${server.pid ?? 'unknown'} exitCode=${server.exitCode ?? 'null'} signalCode=${server.signalCode ?? 'null'} signals=${signalDeliverySummary(signals)} output=${serverOutputSummary(output)}`;
}

async function stopServer(
  server: ChildProcess,
  output: SingleFileServerOutput,
  timeoutMs: number = singleFileServerStopTimeoutMs
): Promise<void> {
  if (hasServerExited(server)) return;
  const signals: SignalDelivery[] = [];
  signals.push(signalServer(server, 'SIGTERM'));
  if (await waitForServerExit(server, timeoutMs)) return;
  signals.push(signalServer(server, 'SIGKILL'));
  if (await waitForServerExit(server, timeoutMs)) return;
  throw new Error(`single file server did not stop: ${serverStopDiagnostics(server, output, signals)}`);
}

async function withStoppedServer<T>(
  server: ChildProcess,
  output: SingleFileServerOutput,
  run: () => Promise<T>,
  stopTimeoutMs: number = singleFileServerStopTimeoutMs
): Promise<T> {
  let runError: unknown;
  try {
    return await run();
  } catch (error) {
    runError = error;
    throw error;
  } finally {
    try {
      await stopServer(server, output, stopTimeoutMs);
    } catch (cleanupError) {
      if (!runError) throw cleanupError;
      throw new AggregateError(
        [runError, cleanupError],
        `test failed and single file server cleanup also failed: run=${errorMessage(runError)} cleanup=${errorMessage(cleanupError)}`
      );
    }
  }
}

test.beforeEach(async ({ page }) => {
  await resetStandardFixtures();
  await page.goto('/');
  await expect(page.locator('#sidebar')).toBeVisible();
});

test.afterEach(async () => {
  await resetStandardFixtures();
});

test('command output summaryは長いstdout/stderrを末尾に制限する', () => {
  const error = new Error('build failed') as CommandError;
  error.stdout = `stdout-${'a'.repeat(singleFileServerOutputLimit)}`;
  error.stderr = `stderr-${'b'.repeat(100)}`;

  const summary = commandOutputSummary(error);

  expect(summary.length).toBeLessThanOrEqual(singleFileServerOutputLimit + 64);
  expect(summary).toContain('truncated to last');
  expect(summary).not.toContain('stdout-');
  expect(summary).toContain('stderr-');
});

test('stopServerの停止失敗は診断情報を含む', async () => {
  const output: SingleFileServerOutput = { spawnError: '', text: 'URL: http://127.0.0.1:4123\nready' };
  const server = fakeRunningServer(() => false);

  await expect(stopServer(server, output, 1)).rejects.toThrow(/pid=[\s\S]*exitCode=[\s\S]*signalCode=[\s\S]*SIGTERM=false[\s\S]*SIGKILL=false[\s\S]*ready/);
});

test('withStoppedServerは本体失敗とcleanup失敗をAggregateErrorのmessageにも保持する', async () => {
  const output: SingleFileServerOutput = { spawnError: '', text: 'server output' };
  const server = fakeRunningServer();
  const runError = new Error('assertion failed');

  await expect(withStoppedServer(server, output, async () => {
    throw runError;
  }, 1)).rejects.toMatchObject({
    name: 'AggregateError',
    errors: [
      runError,
      expect.any(Error)
    ],
    message: expect.stringMatching(/run=assertion failed[\s\S]*cleanup=single file server did not stop/)
  });
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

test('サイドバー幅はドラッグ開始だけでは変化しない', async ({ page }) => {
  const sidebar = page.locator('#sidebar');
  const handle = page.locator('#sidebar-width-resizer');
  const initialBox = await sidebar.boundingBox();
  const handleBox = await handle.boundingBox();

  expect(initialBox).not.toBeNull();
  expect(handleBox).not.toBeNull();

  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y + 80);
  await page.mouse.down();

  await expect.poll(async () => Math.round((await sidebar.boundingBox())?.width ?? 0)).toBe(Math.round(initialBox!.width));

  await page.mouse.up();
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

test('内部リサイズはドラッグ開始だけでは高さを変えない', async ({ page }) => {
  const panel = page.locator('#panel-files');
  const content = panel.locator('.sidebar-resizable-content');
  const handle = panel.locator('.sidebar-content-resizer');
  const initialBox = await content.boundingBox();
  const handleBox = await handle.boundingBox();

  expect(initialBox).not.toBeNull();
  expect(handleBox).not.toBeNull();

  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y + handleBox!.height - 1);
  await page.mouse.down();

  await expect.poll(async () => Math.round((await content.boundingBox())?.height ?? 0)).toBe(Math.round(initialBox!.height));

  await page.mouse.up();
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

test('狭いモバイル幅でもサイドバー幅はモバイル契約を維持する', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 720 });
  await page.locator('#sidebar-open').click();

  await expect(page.locator('#sidebar')).toHaveClass(/open/);
  await expect.poll(async () => await page.locator('#sidebar').evaluate((element) => getComputedStyle(element).maxWidth)).toBe('320px');
  await expect.poll(async () => Math.round((await page.locator('#sidebar').boundingBox())?.width ?? 0)).toBe(320);
});

test('デスクトップで縮めた内部高さはモバイル表示へ持ち越さない', async ({ page }) => {
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

  await page.setViewportSize({ width: 390, height: 720 });
  await page.locator('#sidebar-open').click();

  await expect(page.locator('#sidebar')).toHaveClass(/open/);
  await expect.poll(async () => await content.evaluate((element) => getComputedStyle(element).flexGrow)).toBe('1');
  await expect.poll(async () => await content.evaluate((element) => getComputedStyle(element).maxHeight)).toBe('none');
  await expect.poll(async () => (await content.boundingBox())?.height ?? 0).toBeGreaterThan(260);
});

test('内部リサイズのARIA最小値はCSSのmin-heightに追従する', async ({ page }) => {
  await page.evaluate(() => {
    document.documentElement.style.fontSize = '20px';
    window.dispatchEvent(new Event('resize'));
  });

  const content = page.locator('#panel-files .sidebar-resizable-content');
  const handle = page.locator('#panel-files .sidebar-content-resizer');
  await handle.focus();
  for (let i = 0; i < 30; i++) {
    await page.keyboard.press('ArrowUp');
  }

  await expect.poll(async () => Math.round((await content.boundingBox())?.height ?? 0)).toBe(160);
  await expect(handle).toHaveAttribute('aria-valuemin', '160');
  await expect(handle).toHaveAttribute('aria-valuenow', '160');
});

test('低いデスクトップ表示でもサイドバー下部へスクロール到達できる', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 160 });

  const sidebar = page.locator('#sidebar');
  const panel = page.locator('#panel-files');
  const handle = page.locator('#panel-files .sidebar-content-resizer');
  await expect.poll(async () => await panel.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
  await sidebar.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await panel.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect(handle).toBeInViewport();
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
  test.setTimeout(90000);
  const serverOutput: SingleFileServerOutput = { spawnError: '', text: '' };
  const server = spawn(await buildSingleFileServerBinary(), [
    'tests/fixtures/e2e/README.md',
    '--port',
    '0',
    '--no-open'
  ], {
    detached: process.platform !== 'win32',
    cwd: process.cwd(),
    stdio: ['ignore', 'pipe', 'pipe']
  });
  server.stdout?.on('data', (chunk: Buffer) => appendServerOutput(serverOutput, chunk));
  server.stderr?.on('data', (chunk: Buffer) => appendServerOutput(serverOutput, chunk));
  server.once('error', (error: Error) => {
    serverOutput.spawnError = errorMessage(error);
  });

  await withStoppedServer(server, serverOutput, async () => {
    const url = await waitForSingleFileServer(server, serverOutput);
    await page.goto(url);
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
  });
});
