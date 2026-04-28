import fs from 'node:fs/promises';
import path from 'node:path';
import { expect, type Page } from '@playwright/test';

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');
const readmePath = path.join(fixtureDir, 'README.md');
const notesPath = path.join(fixtureDir, 'notes.md');

export type ResetStandardFixturesOptions = {
  cleanupMemoArtifacts?: boolean;
};

export async function resetStandardFixtures(options: ResetStandardFixturesOptions = {}) {
  if (options.cleanupMemoArtifacts === true) {
    const entries = await fs.readdir(fixtureDir, { withFileTypes: true });
    await Promise.all(entries
      .filter((entry) => entry.isFile() && entry.name.endsWith('.memo.md'))
      .map((entry) => fs.rm(path.join(fixtureDir, entry.name), { force: true })));
    await fs.rm(path.join(fixtureDir, '.markdown-view'), { recursive: true, force: true });
  }
  await fs.writeFile(readmePath, '# README\n\nInitial README content\n');
  await fs.writeFile(notesPath, '# Notes\n\nNotes body\n');
}

export async function selectParagraphText(page: Page, text: string) {
  await page.evaluate((targetText) => {
    const walker = document.createTreeWalker(document.getElementById('content')!, NodeFilter.SHOW_TEXT);
    let node = null;
    while ((node = walker.nextNode())) {
      if (node.textContent && node.textContent.includes(targetText)) {
        const selection = window.getSelection()!;
        const range = document.createRange();
        range.selectNodeContents(node.parentElement!);
        selection.removeAllRanges();
        selection.addRange(range);
        return;
      }
    }
    throw new Error(`text not found: ${targetText}`);
  }, text);
}

export async function clearSelection(page: Page) {
  await page.evaluate(() => {
    const selection = window.getSelection()!;
    selection.removeAllRanges();
    document.dispatchEvent(new Event('selectionchange'));
  });
}

export async function openMemoTab(page: Page) {
  await page.locator('.sidebar-tab[data-tab="memo"]').click();
  await expect(page.locator('#panel-memo.active')).toBeVisible();
}

export async function openFileTab(page: Page) {
  await page.locator('.sidebar-tab[data-tab="files"]').click();
  await expect(page.locator('#panel-files.active')).toBeVisible();
}

export async function selectFile(page: Page, file: string) {
  await openFileTab(page);
  await page.locator(`[data-file="${file}"]`).click();
}

export async function saveMemo(page: Page, text: string) {
  const editor = page.locator('#memo-editor');
  await editor.fill(text);
  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');
}

export async function activeTocLabel(page: Page) {
  return page.locator('#toc a.active').innerText();
}

export async function activeTocLabelOrEmpty(page: Page) {
  const activeLink = page.locator('#toc a.active');
  return (await activeLink.count()) > 0 ? activeLink.innerText() : '';
}

export async function clickTocLink(page: Page, id: string) {
  await page.evaluate((targetId) => {
    const link = document.querySelector(`#toc a[href="#${targetId}"]`) as HTMLAnchorElement | null;
    if (!link) {
      throw new Error(`toc link not found: ${targetId}`);
    }
    link.click();
  }, id);
}

export async function waitForTocTrackingFrame(page: Page) {
  await page.evaluate(() => {
    // 通常の scroll 由来更新用。suppressTocTrackingFor が有効な期間は別途待つ。
    return new Promise<void>((resolve) => {
      window.requestAnimationFrame(() => {
        window.requestAnimationFrame(() => resolve());
      });
    });
  });
}

export async function startTocActiveChangeRecorder(page: Page) {
  await page.evaluate(() => {
    const toc = document.getElementById('toc');
    if (!toc) {
      throw new Error('TOC is not initialized');
    }
    window.__tocActiveChanges = [];
    let lastLabel = '__unset__';
    const recordActive = () => {
      const active = toc.querySelector('a.active');
      const label = active ? active.textContent || '' : '';
      if (label !== lastLabel) {
        const tocActiveChanges = window.__tocActiveChanges;
        if (!tocActiveChanges) {
          throw new Error('TOC active change recorder is not initialized');
        }
        tocActiveChanges.push(label);
        lastLabel = label;
      }
    };
    recordActive();
    const observer = new MutationObserver(recordActive);
    observer.observe(toc, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ['class']
    });
    window.__stopTocObserver = () => observer.disconnect();
  });
}

export async function stopTocActiveChangeRecorder(page: Page) {
  return page.evaluate(() => {
    const stopTocObserver = window.__stopTocObserver;
    const tocActiveChanges = window.__tocActiveChanges;
    if (!stopTocObserver || !tocActiveChanges) {
      throw new Error('TOC active change recorder is not initialized');
    }
    stopTocObserver();
    const changes = tocActiveChanges.slice();
    delete window.__stopTocObserver;
    delete window.__tocActiveChanges;
    return changes;
  });
}

export async function stabilizeWebSocketHarness(page: Page) {
  await page.waitForFunction(() => {
    const lastWs = window.__lastWs;
    return Boolean(lastWs && typeof lastWs.onmessage === 'function');
  });
  await page.evaluate(() => {
    const lastWs = window.__lastWs;
    if (!lastWs || typeof lastWs.onmessage !== 'function') {
      throw new Error('WebSocket test harness is not initialized');
    }
    // __dispatchWsMessage は MessageEvent を生成せず { data: string } を直接渡すため、
    // E2E ハーネス内では onmessage の契約をテスト用の狭い型へ bridge する。
    window.__realWsOnmessage = lastWs.onmessage as unknown as (ev: { data: string }) => void;
    lastWs.onmessage = function() {};
    window.__dispatchWsMessage = (payload) => {
      const realWsOnmessage = window.__realWsOnmessage;
      if (!realWsOnmessage) {
        throw new Error('WebSocket test harness message handler is not initialized');
      }
      realWsOnmessage({ data: JSON.stringify(payload) });
    };
  });
}

export async function dispatchWsMessage(page: Page, payload: unknown) {
  await page.evaluate((messagePayload) => {
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchWsMessage(messagePayload);
  }, payload);
}

export async function dispatchWsMessages(page: Page, payloads: unknown[]) {
  await page.evaluate((messagePayloads) => {
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    for (const payload of messagePayloads) {
      dispatchWsMessage(payload);
    }
  }, payloads);
}

export async function updateContent(
  page: Page,
  data: MvE2E.UpdateContentPayload,
  opts?: MvE2E.UpdateContentOptions
) {
  await page.evaluate(({ payload, options }) => {
    const updateContent = window.updateContent;
    if (!updateContent) {
      throw new Error('window.updateContent is not exposed for E2E');
    }
    updateContent(payload, options);
  }, { payload: data, options: opts });
}
