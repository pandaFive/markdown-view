import { test, expect } from '@playwright/test';
import { openMemoTab, resetStandardFixtures, saveMemo } from './helpers';

test.beforeEach(async () => {
  await resetStandardFixtures({ cleanupMemoArtifacts: true });
});

test.afterEach(async () => {
  await resetStandardFixtures({ cleanupMemoArtifacts: true });
});

test('同一ファイルを開いている別ページへメモ更新が同期される', async ({ page, context }) => {
  const peer = await context.newPage();

  await page.goto('/');
  await peer.goto('/');

  await openMemoTab(page);
  await openMemoTab(peer);

  await expect(page.locator('#memo-editor')).toHaveValue('');
  await expect(peer.locator('#memo-editor')).toHaveValue('');

  await saveMemo(page, 'shared memo');

  await expect(peer.locator('#memo-editor')).toHaveValue('shared memo');
  await expect(peer.locator('#memo-preview')).toContainText('shared memo');
});

test('受信側が編集中のときはリモートメモ更新で上書きしない', async ({ page, context }) => {
  const peer = await context.newPage();

  await page.goto('/');
  await peer.goto('/');

  await openMemoTab(page);
  await openMemoTab(peer);

  await peer.locator('#memo-editor').fill('local draft');
  await expect(peer.locator('#memo-save-status')).toHaveText('未保存');

  await saveMemo(page, 'remote memo');

  await peer.waitForTimeout(300);
  await expect(peer.locator('#memo-editor')).toHaveValue('local draft');
  await expect(peer.locator('#memo-preview')).not.toContainText('remote memo');
});

test('受信側がフォーカス中でもblur後に保留中のメモ更新が反映される', async ({ page, context }) => {
  const peer = await context.newPage();

  await page.goto('/');
  await peer.goto('/');

  await openMemoTab(page);
  await openMemoTab(peer);

  await peer.locator('#memo-editor').focus();
  await expect(peer.locator('#memo-save-status')).toHaveText('保存済み');

  await saveMemo(page, 'deferred remote');

  await peer.waitForTimeout(300);
  await expect(peer.locator('#memo-editor')).toHaveValue('');
  await expect(peer.locator('#memo-preview')).not.toContainText('deferred remote');

  await peer.locator('.sidebar-tab[data-tab="files"]').focus();
  await peer.locator('#memo-editor').blur();

  await expect(peer.locator('#memo-editor')).toHaveValue('deferred remote');
  await expect(peer.locator('#memo-preview')).toContainText('deferred remote');
});

test('dirty中に届いた古いメモ更新で保存後の内容を巻き戻さない', async ({ page, context }) => {
  const peer = await context.newPage();

  await page.goto('/');
  await peer.goto('/');

  await openMemoTab(page);
  await openMemoTab(peer);

  await saveMemo(peer, 'A');
  await expect(page.locator('#memo-editor')).toHaveValue('A');

  await peer.locator('#memo-editor').fill('AB');
  await expect(peer.locator('#memo-save-status')).toHaveText('未保存');

  await saveMemo(page, 'A');
  await peer.waitForTimeout(300);
  await expect(peer.locator('#memo-editor')).toHaveValue('AB');

  await saveMemo(peer, 'AB');
  await expect(peer.locator('#memo-editor')).toHaveValue('AB');
  await expect(peer.locator('#memo-preview')).toContainText('AB');
  await expect(peer.locator('#memo-preview')).not.toContainText(/^A$/);
});

test('focus中に保留した古い更新はローカル編集開始後に適用しない', async ({ page, context }) => {
  const peer = await context.newPage();

  await page.goto('/');
  await peer.goto('/');

  await openMemoTab(page);
  await openMemoTab(peer);

  await peer.locator('#memo-editor').focus();
  await saveMemo(page, 'A');

  await peer.waitForTimeout(300);
  await expect(peer.locator('#memo-editor')).toHaveValue('');

  await peer.locator('#memo-editor').fill('B');
  await expect(peer.locator('#memo-save-status')).toHaveText('未保存');

  await peer.locator('.sidebar-tab[data-tab="files"]').focus();
  await peer.locator('#memo-editor').blur();
  await peer.waitForTimeout(300);

  await expect(peer.locator('#memo-editor')).toHaveValue('B');
  await expect(peer.locator('#memo-preview')).not.toContainText(/^A$/);

  await saveMemo(peer, 'B');
  await expect(peer.locator('#memo-editor')).toHaveValue('B');
  await expect(peer.locator('#memo-preview')).toContainText('B');
});

test('別ファイルを開いているページにはメモ更新を誤反映しない', async ({ page, context }) => {
  const peer = await context.newPage();

  await page.goto('/');
  await peer.goto('/?file=notes.md');

  await openMemoTab(page);
  await openMemoTab(peer);

  await expect(peer.locator('#content')).toContainText('Notes body');

  await saveMemo(peer, 'notes local');
  await expect(peer.locator('#memo-preview')).toContainText('notes local');

  await saveMemo(page, 'readme remote');

  await peer.waitForTimeout(300);
  await expect(peer.locator('#memo-editor')).toHaveValue('notes local');
  await expect(peer.locator('#memo-preview')).toContainText('notes local');
  await expect(peer.locator('#memo-preview')).not.toContainText('readme remote');
});
