import fs from 'node:fs/promises';
import path from 'node:path';
import { expect, type Page } from '@playwright/test';

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');
const readmePath = path.join(fixtureDir, 'README.md');
const notesPath = path.join(fixtureDir, 'notes.md');

async function memoArtifactPaths() {
  const entries = await fs.readdir(fixtureDir, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && entry.name.endsWith('.memo.md'))
    .map((entry) => path.join(fixtureDir, entry.name));
}

async function assertMemoArtifactsRemoved() {
  const leftovers = await memoArtifactPaths();
  const markdownViewPath = path.join(fixtureDir, '.markdown-view');
  try {
    await fs.access(markdownViewPath);
    leftovers.push(markdownViewPath);
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code !== 'ENOENT') {
      throw error;
    }
  }

  if (leftovers.length > 0) {
    const relativeLeftovers = leftovers.map((entry) => path.relative(fixtureDir, entry)).join(', ');
    throw new Error(`fixture cleanup left stale artifacts: ${relativeLeftovers}`);
  }
}

export type ResetStandardFixturesOptions = {
  cleanupMemoArtifacts?: boolean;
};

export async function resetStandardFixtures(options: ResetStandardFixturesOptions = {}) {
  // 既定で memo artifact を掃除し、前テスト残骸の混入を防ぐ。温存したい spec だけ false で opt-out する。
  if (options.cleanupMemoArtifacts ?? true) {
    await Promise.all(
      (await memoArtifactPaths()).map((memoPath) => fs.rm(memoPath, { force: true }))
    );
    await fs.rm(path.join(fixtureDir, '.markdown-view'), { recursive: true, force: true });
    await assertMemoArtifactsRemoved();
  }
  await fs.writeFile(readmePath, '# README\n\nInitial README content\n');
  await fs.writeFile(notesPath, '# Notes\n\nNotes body\n');
}

export type SelectParagraphTextOptions = {
  match?: 'exact' | 'contains';
};

export async function selectParagraphText(
  page: Page,
  text: string,
  options: SelectParagraphTextOptions = {}
) {
  await page.evaluate(({ targetText, match }) => {
    const content = document.getElementById('content');
    if (!content) {
      throw new Error('content root not found');
    }

    const walker = document.createTreeWalker(content, NodeFilter.SHOW_TEXT);
    const matches: Text[] = [];
    let node: Node | null = null;
    while ((node = walker.nextNode())) {
      const nodeText = node.textContent;
      if (!nodeText) {
        continue;
      }
      const isMatch = match === 'contains' ? nodeText.includes(targetText) : nodeText === targetText;
      if (isMatch) {
        matches.push(node as Text);
      }
    }

    if (matches.length === 0) {
      throw new Error(`text not found: ${targetText}`);
    }
    if (matches.length > 1) {
      throw new Error(`ambiguous text match: ${targetText} (${matches.length} matches)`);
    }

    const matchedNode = matches[0];
    if (!matchedNode) {
      throw new Error(`text not found: ${targetText}`);
    }
    const parent = matchedNode.parentElement;
    if (!parent) {
      throw new Error(`text match has no parent element: ${targetText}`);
    }

    const selection = window.getSelection();
    if (!selection) {
      throw new Error('window selection is not available');
    }
    const range = document.createRange();
    range.selectNodeContents(parent);
    selection.removeAllRanges();
    selection.addRange(range);
  }, { targetText: text, match: options.match ?? 'exact' });
}

export async function clearSelection(page: Page) {
  await page.evaluate(() => {
    const selection = window.getSelection()!;
    selection.removeAllRanges();
    // E2Eでは選択解除を直接操作するため、アプリ側のselectionchange処理も手動で発火する。
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

// メモ保存のdebounce完了を保存済み表示で待つ。
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
    const assertFreshBridge = () => {
      if (window.__bridgedWs !== window.__lastWs) {
        throw new Error('WebSocket test harness bridge is stale; call stabilizeWebSocketHarness after reconnect');
      }
    };
    const alreadyBridged = window.__bridgedWs === lastWs && window.__realWsOnmessage;
    window.__bridgedWs = lastWs;
    if (!alreadyBridged) {
      window.__realWsOnmessage = lastWs.onmessage as unknown as (ev: { data: string }) => void;
    }
    lastWs.onmessage = function() {};
    window.__dispatchWsMessage = (payload) => {
      assertFreshBridge();
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
    if (window.__bridgedWs !== window.__lastWs) {
      throw new Error('WebSocket test harness bridge is stale; call stabilizeWebSocketHarness after reconnect');
    }
    const dispatchMessage = window.__dispatchWsMessage;
    if (!dispatchMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchMessage(messagePayload);
  }, payload);
}

// text_selection_defer のフォールバック検証用。
// 偽メッセージ送信と実 handler 無効化を同じ browser step に閉じ込める。
// この helper の後に dispatchWsMessage を続けて呼ぶ用途では使わない。
export async function dispatchWsMessageAndDisableRealHandler(page: Page, payload: unknown) {
  await page.evaluate((messagePayload) => {
    if (window.__bridgedWs !== window.__lastWs) {
      throw new Error('WebSocket test harness bridge is stale; call stabilizeWebSocketHarness after reconnect');
    }
    const dispatchMessage = window.__dispatchWsMessage;
    if (!dispatchMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    const lastWs = window.__lastWs;
    if (!lastWs) {
      throw new Error('WebSocket test harness is not initialized');
    }
    dispatchMessage(messagePayload);
    // watcher経由の実WSメッセージがpendingUpdateを上書きしないよう、
    // 偽メッセージ送信後にonmessageを無効化する。
    lastWs.onmessage = function() {};
  }, payload);
}

export async function dispatchWsMessages(page: Page, payloads: unknown[]) {
  await page.evaluate((messagePayloads) => {
    if (window.__bridgedWs !== window.__lastWs) {
      throw new Error('WebSocket test harness bridge is stale; call stabilizeWebSocketHarness after reconnect');
    }
    const dispatchMessage = window.__dispatchWsMessage;
    if (!dispatchMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    for (const [index, payload] of messagePayloads.entries()) {
      try {
        dispatchMessage(payload);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        throw new Error(`WebSocket test harness dispatcher failed at payload ${index}: ${message}`);
      }
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

export async function updateContentAndActivateToc(
  page: Page,
  data: MvE2E.UpdateContentPayload,
  opts?: MvE2E.UpdateContentOptions
) {
  await page.evaluate(({ payload, options }) => {
    const updateContent = window.updateContent;
    if (!updateContent) {
      throw new Error('window.updateContent is not exposed for E2E');
    }
    if (typeof activateSidebarTab !== 'function') {
      throw new Error('activateSidebarTab is not exposed for E2E');
    }
    updateContent(payload, options);
    activateSidebarTab('toc');
  }, { payload: data, options: opts });
}
