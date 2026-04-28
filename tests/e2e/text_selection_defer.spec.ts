import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect, type Page } from '@playwright/test';
import { installTestWebSocketHarness } from './browser/test-websocket';

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');
const readmePath = path.join(fixtureDir, 'README.md');
const notesPath = path.join(fixtureDir, 'notes.md');

async function resetFixtures() {
  await fs.writeFile(readmePath, '# README\n\nInitial README content\n');
  await fs.writeFile(notesPath, '# Notes\n\nNotes body\n');
}

async function selectParagraphText(page: Page, text: string) {
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

async function clearSelection(page: Page) {
  await page.evaluate(() => {
    const selection = window.getSelection()!;
    selection.removeAllRanges();
    document.dispatchEvent(new Event('selectionchange'));
  });
}

async function activeTocLabel(page: Page) {
  return page.locator('#toc a.active').innerText();
}

async function activeTocLabelOrEmpty(page: Page) {
  const activeLink = page.locator('#toc a.active');
  return (await activeLink.count()) > 0 ? activeLink.innerText() : '';
}

async function clickTocLink(page: Page, id: string) {
  await page.evaluate((targetId) => {
    const link = document.querySelector(`#toc a[href="#${targetId}"]`) as HTMLAnchorElement | null;
    if (!link) {
      throw new Error(`toc link not found: ${targetId}`);
    }
    link.click();
  }, id);
}

async function currentScrollY(page: Page) {
  return page.evaluate(() => window.scrollY || window.pageYOffset);
}

async function waitForTocTrackingFrame(page: Page) {
  await page.evaluate(() => {
    return new Promise<void>((resolve) => {
      window.requestAnimationFrame(() => {
        window.requestAnimationFrame(() => resolve());
      });
    });
  });
}

async function startTocActiveChangeRecorder(page: Page) {
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

async function stopTocActiveChangeRecorder(page: Page) {
  return page.evaluate(() => {
    const stopTocObserver = window.__stopTocObserver;
    const tocActiveChanges = window.__tocActiveChanges;
    if (!stopTocObserver || !tocActiveChanges) {
      throw new Error('TOC active change recorder is not initialized');
    }
    stopTocObserver();
    delete window.__stopTocObserver;
    return tocActiveChanges.slice();
  });
}

async function stabilizeWebSocketHarness(page: Page) {
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

async function loadDenseHeadingFixture(page: Page) {
  const repeated = Array.from({ length: 12 }, (_, index) => `Paragraph ${index + 1}`).join('\n\n');
  await fs.writeFile(
    readmePath,
    [
      '# README',
      '',
      repeated,
      '',
      '## Alpha',
      '',
      'Alpha body',
      '',
      '## Beta',
      '',
      'Beta body',
      '',
      repeated
    ].join('\n')
  );

  await page.reload();
  await expect(page.locator('#toc')).toContainText('Alpha');
  await expect(page.locator('#toc')).toContainText('Beta');
  await stabilizeWebSocketHarness(page);

  return page.evaluate(() => {
    const alpha = document.getElementById('alpha')!;
    const beta = document.getElementById('beta')!;
    const offset = parseFloat(window.getComputedStyle(alpha).scrollMarginTop) || 112;
    return {
      alphaTop: alpha.getBoundingClientRect().top + window.scrollY,
      betaTop: beta.getBoundingClientRect().top + window.scrollY,
      activationOffset: offset
    };
  });
}

async function loadBottomHeadingFixture(page: Page) {
  await fs.writeFile(
    readmePath,
    [
      '# README',
      '',
      'Intro',
      '',
      '## Alpha',
      '',
      'Alpha body',
      '',
      '## Beta',
      '',
      'Beta body'
    ].join('\n')
  );

  await page.reload();
  await expect(page.locator('#toc')).toContainText('Alpha');
  await expect(page.locator('#toc')).toContainText('Beta');
  await stabilizeWebSocketHarness(page);
}

test.beforeEach(async ({ page }) => {
  await resetFixtures();
  await page.addInitScript(installTestWebSocketHarness, { shorten30sTimeouts: true });
  await page.goto('/');
  await expect(page.locator('#content')).toContainText('Initial README content');
  await stabilizeWebSocketHarness(page);
});

test.afterEach(async () => {
  await resetFixtures();
});

test('ドラッグ選択中はWebSocket更新を延期し、選択解除後に適用する', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');

  await page.evaluate(() => {
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchWsMessage({
      content: '<h1 id="readme">README</h1><p>Deferred update</p>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>',
      file: 'README.md'
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
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchWsMessage({
      content: '<h1 id="readme">README</h1><p>Pending stale update</p>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>',
      file: 'README.md'
    });
  });

  await page.evaluate(() => {
    selectFile('notes.md');
  });
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
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchWsMessage({ refresh: true });
  });

  await expect(page.locator('#content')).toContainText('Initial README content');
  await expect(page.locator('#content')).not.toContainText('Refreshed from server');

  await clearSelection(page);
  await expect(page.locator('#content')).toContainText('Refreshed from server');
});

test('選択中はrefreshが古いバッファ更新より優先される', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');
  await fs.writeFile(readmePath, '# README\n\nRefresh wins after selection\n');

  await page.evaluate(() => {
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchWsMessage({
      content: '<h1 id="readme">README</h1><p>Stale buffered update</p>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>',
      file: 'README.md'
    });
    dispatchWsMessage({ refresh: true });
  });

  await expect(page.locator('#content')).toContainText('Initial README content');

  await clearSelection(page);
  await expect(page.locator('#content')).toContainText('Refresh wins after selection');
  await expect(page.locator('#content')).not.toContainText('Stale buffered update');
});

test('選択解除されなくても30秒フォールバックで保留更新を適用する', async ({ page }) => {
  await selectParagraphText(page, 'Initial README content');

  await page.evaluate(() => {
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    const lastWs = window.__lastWs;
    if (!lastWs) {
      throw new Error('WebSocket test harness is not initialized');
    }
    dispatchWsMessage({
      content: '<h1 id="readme">README</h1><p>Fallback applied</p>',
      toc: '<ul><li><a href="#readme">README</a></li></ul>',
      file: 'README.md'
    });
    // watcher経由の実WSメッセージがpendingUpdateを上書きしないよう、
    // 偽メッセージ送信後にonmessageを無効化する
    lastWs.onmessage = function() {};
  });

  await expect(page.locator('#content')).toContainText('Fallback applied');
});

test('近接した見出し境界でも目次activeが前後に揺れない', async ({ page }) => {
  const positions = await loadDenseHeadingFixture(page);

  await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.alphaTop - positions.activationOffset + 8);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');

  await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.betaTop - positions.activationOffset - 8);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');

  for (const delta of [2, -2, 1, -1]) {
    await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.betaTop - positions.activationOffset - 8 + delta);
    await expect.poll(() => activeTocLabel(page)).toBe('Alpha');
  }

  await page.evaluate(() => {
    window.scrollTo(0, document.getElementById('beta')!.getBoundingClientRect().top + window.scrollY);
  });
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
  const betaActiveScrollTop = await page.evaluate(() => window.scrollY);

  for (const delta of [2, -2, 1, -1]) {
    await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), betaActiveScrollTop + delta);
    await expect.poll(() => activeTocLabel(page)).toBe('Beta');
  }
});

test('最初の見出しに到達するまでは目次activeを付けない', async ({ page }) => {
  const positions = await loadDenseHeadingFixture(page);

  await page.evaluate(() => window.scrollTo(0, 0));
  await expect.poll(() => activeTocLabelOrEmpty(page)).toBe('');

  await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.alphaTop - positions.activationOffset + 8);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');
});

test('目次クリック直後はクリックした見出しをactiveにする', async ({ page }) => {
  const positions = await loadDenseHeadingFixture(page);

  await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.betaTop - positions.activationOffset - 8);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');

  await clickTocLink(page, 'beta');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
  await page.waitForTimeout(450);
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
});

test('下端見出しがactivation位置まで届かなくてもクリック先をactiveにする', async ({ page }) => {
  await loadBottomHeadingFixture(page);

  await clickTocLink(page, 'beta');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
  await page.waitForTimeout(450);
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
});

test('短いページの初期表示では末尾見出しをactiveにしない', async ({ page }) => {
  await loadBottomHeadingFixture(page);

  await page.evaluate(() => window.scrollTo(0, 0));
  await expect.poll(() => activeTocLabelOrEmpty(page)).toBe('');
});

test('目次クリック後は猶予時間経過後に通常スクロール判定へ戻る', async ({ page }) => {
  await loadDenseHeadingFixture(page);

  await clickTocLink(page, 'beta');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
  await page.waitForTimeout(450);

  await page.evaluate(() => {
    var alpha = document.getElementById('alpha')!;
    var offset = parseFloat(window.getComputedStyle(alpha).scrollMarginTop) || 112;
    window.scrollTo(0, alpha.getBoundingClientRect().top + window.scrollY - offset + 8);
  });
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');
});

test('目次クリック直後でも逆方向へスクロールしたら通常判定へ戻る', async ({ page }) => {
  await loadDenseHeadingFixture(page);

  await clickTocLink(page, 'beta');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');

  await page.evaluate(() => {
    var alpha = document.getElementById('alpha')!;
    var offset = parseFloat(window.getComputedStyle(alpha).scrollMarginTop) || 112;
    window.scrollTo(0, alpha.getBoundingClientRect().top + window.scrollY - offset + 8);
  });
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');
});

test('目次クリック直後の小揺らしではクリック先のactiveが維持される', async ({ page }) => {
  await loadDenseHeadingFixture(page);

  await clickTocLink(page, 'beta');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');

  await page.evaluate(() => {
    window.scrollTo(0, (window.scrollY || window.pageYOffset) + 6);
  });
  // grace (400ms) 内にscroll→scheduleTocTrackingUpdate→raFまで走り切らせる。
  // 150msはTOC_NAVIGATION_GRACE_MS未満で意図的に小さい値
  await page.waitForTimeout(150);
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
});

test('目次クリックの猶予中に別の目次をクリックしたら最後のクリック先へ収束する', async ({ page }) => {
  const positions = await loadDenseHeadingFixture(page);

  await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.betaTop - positions.activationOffset - 8);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');

  await clickTocLink(page, 'alpha');
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');
  await clickTocLink(page, 'beta');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');

  const expectedBetaScrollY = positions.betaTop - positions.activationOffset;
  await expect.poll(async () => Math.abs((await currentScrollY(page)) - expectedBetaScrollY)).toBeLessThanOrEqual(4);
});

test('目次クリック後のslack内スクロールではpending activeを維持し、slack外では通常判定へ戻る', async ({ page }) => {
  const positions = await loadDenseHeadingFixture(page);

  await clickTocLink(page, 'beta');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');

  await page.evaluate(
    ({ betaTop, activationOffset }) => {
      window.scrollTo(0, betaTop - activationOffset - 22);
    },
    { betaTop: positions.betaTop, activationOffset: positions.activationOffset }
  );
  await waitForTocTrackingFrame(page);
  expect(await activeTocLabel(page)).toBe('Beta');

  await page.evaluate(
    ({ betaTop, activationOffset }) => {
      window.scrollTo(0, betaTop - activationOffset - 26);
    },
    { betaTop: positions.betaTop, activationOffset: positions.activationOffset }
  );
  await waitForTocTrackingFrame(page);
  expect(await activeTocLabel(page)).toBe('Alpha');
});

test('目次クリック直後の小揺らし中にactiveがBeta以外へ遷移しない', async ({ page }) => {
  await loadDenseHeadingFixture(page);

  await clickTocLink(page, 'beta');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
  await startTocActiveChangeRecorder(page);

  for (const delta of [6, -4, 3]) {
    await page.evaluate((scrollDelta) => {
      window.scrollTo(0, (window.scrollY || window.pageYOffset) + scrollDelta);
    }, delta);
    await waitForTocTrackingFrame(page);
  }

  const activeChanges = await stopTocActiveChangeRecorder(page);
  expect(activeChanges).toContain('Beta');
  expect(activeChanges.filter((label) => label !== 'Beta')).toEqual([]);
});

test('日本語id見出しでも目次クリック直後の逆方向スクロールで通常判定へ戻る', async ({ page }) => {
  // Chromium の location.hash は非ASCII id を URL エンコードして返すため、
  // pendingTocNavigationId (raw) と文字列一致させるには decode が必要。
  // 生 hash 比較のままだと日本語 id で popstate ガードが素通りし、
  // restore→scrollIntoView が明示スクロールを上書きして L333 と同じ症状が
  // 日本語見出しのみで再発する。Codex レビュー P2 指摘の回帰防止
  const repeated = Array.from({ length: 12 }, (_, index) => `Paragraph ${index + 1}`).join('\n\n');
  await fs.writeFile(
    readmePath,
    [
      '# README', '', repeated, '',
      '## Alpha', '', 'Alpha body', '',
      '## 日本語見出し', '', '日本語本文', '',
      repeated
    ].join('\n')
  );
  await page.reload();
  await expect(page.locator('#toc')).toContainText('日本語見出し');
  await stabilizeWebSocketHarness(page);

  await page.evaluate(() => {
    const link = document.querySelector('#toc a[href$="%E6%97%A5%E6%9C%AC%E8%AA%9E%E8%A6%8B%E5%87%BA%E3%81%97"], #toc a[href$="#日本語見出し"]') as HTMLAnchorElement | null;
    if (!link) throw new Error('日本語id TOC link not found');
    link.click();
  });
  await expect.poll(() => activeTocLabel(page)).toBe('日本語見出し');

  await page.evaluate(() => {
    var alpha = document.getElementById('alpha')!;
    var offset = parseFloat(window.getComputedStyle(alpha).scrollMarginTop) || 112;
    window.scrollTo(0, alpha.getBoundingClientRect().top + window.scrollY - offset + 8);
  });
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');
});

test('line-range付きhashのpopstateはpending idと不一致のためrestore経路で処理される', async ({ page }) => {
  await loadDenseHeadingFixture(page);

  await clickTocLink(page, 'beta');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
  const betaScrollY = await page.evaluate(() => window.scrollY);

  // pending='beta'の猶予期間内に同id+line-range形式のhashでpopstateを発火。
  // '#beta:L3'は '#' + 'beta' と文字列不一致のためpopstateガードを通過し、
  // restore → applyContentAnchorNavigation の lineRange 分岐で line 3 相当の
  // ブロック（1個目の "Paragraph 1"）がviewport上端付近へスクロールする。
  // ガード比較が startsWith 等に緩められると line-range ジャンプが redundant
  // 扱いでスキップされ scrollY が beta 位置のまま残る。また restore が hash
  // missで先頭 fallback (scrollY=0) に落ちるだけでも素通りしないよう、line 3
  // を含む block の top が viewport 上端付近に着地したことまで検証する
  await page.evaluate(() => {
    history.pushState(null, '', '?file=README.md#beta:L3');
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeLessThan(betaScrollY);
  const line3BlockTop = await page.evaluate(() => {
    var blocks = document.querySelectorAll('[data-line-block-start]');
    for (var i = 0; i < blocks.length; i++) {
      var block = blocks.item(i);
      var s = parseInt(block.getAttribute('data-line-block-start') || '', 10);
      var e = parseInt(block.getAttribute('data-line-block-end') || '', 10);
      if (s <= 3 && e >= 3) {
        return block.getBoundingClientRect().top;
      }
    }
    return null;
  });
  expect(line3BlockTop).not.toBeNull();
  // block:'start' の scrollIntoView で <p> には scroll-margin-top が無いため
  // viewport top (0) 近辺に着地する。restore が hash miss で scrollTo(0,0) に
  // 落ちた場合でも line3Block 自体は body 上端より下にあり top≈0 と区別しづらい
  // が、上の scrollY<betaScrollY と併せて「beta位置から離れ」かつ「line3が上端」
  // の両方を要求する
  expect(line3BlockTop).toBeLessThanOrEqual(20);
  expect(line3BlockTop).toBeGreaterThanOrEqual(-20);
});

test('同一TOCで再初期化してもクリック処理が重複登録されない', async ({ page }) => {
  const positions = await loadDenseHeadingFixture(page);

  await page.evaluate(() => {
    window.__markPendingCalls = 0;
    const original = window.markPendingTocNavigation;
    if (!original) {
      throw new Error('markPendingTocNavigation is not exposed for E2E');
    }
    window.markPendingTocNavigation = function(id) {
      window.__markPendingCalls = (window.__markPendingCalls ?? 0) + 1;
      return original.call(this, id);
    };
  });

  await page.evaluate(() => {
    const toc = document.getElementById('toc')!.innerHTML;
    const repeated = '<p>Updated paragraph</p>'.repeat(12);
    const payload = {
      content:
        '<h1 id="readme">README</h1>' +
        repeated +
        '<h2 id="alpha">Alpha</h2><p>Alpha body updated</p>' +
        '<h2 id="beta">Beta</h2><p>Beta body updated</p>' +
        repeated,
      toc: toc,
      file: 'README.md'
    };
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchWsMessage(payload);
    dispatchWsMessage(payload);
    dispatchWsMessage(payload);
  });

  await expect(page.locator('#content')).toContainText('Alpha body updated');
  await page.waitForTimeout(150);
  await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.betaTop - positions.activationOffset - 8);
  await clickTocLink(page, 'beta');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
  await expect.poll(() => page.evaluate(() => window.__markPendingCalls ?? 0)).toBe(1);
});

test('WebSocket更新後も同じ見出しを見ている間は目次activeを維持する', async ({ page }) => {
  const positions = await loadDenseHeadingFixture(page);

  await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.alphaTop - positions.activationOffset + 8);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');

  await page.evaluate(() => {
    const repeated = '<p>Updated paragraph</p>'.repeat(12);
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchWsMessage({
      content:
        '<h1 id="readme">README</h1>' +
        repeated +
        '<h2 id="alpha">Alpha</h2><p>Alpha body updated</p>' +
        '<h2 id="beta">Beta</h2><p>Beta body updated</p>' +
        repeated,
      toc:
        '<ul>' +
        '<li><a href="#readme">README</a></li>' +
        '<li><a href="#alpha">Alpha</a></li>' +
        '<li><a href="#beta">Beta</a></li>' +
        '</ul>',
      file: 'README.md'
    });
  });

  await expect(page.locator('#content')).toContainText('Alpha body updated');
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');
  await page.waitForTimeout(100);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');
});

test('更新で見出し位置が変わったら現在位置に合う目次activeへ再計算する', async ({ page }) => {
  const positions = await loadDenseHeadingFixture(page);

  await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.alphaTop - positions.activationOffset + 8);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');

  await page.evaluate(() => {
    const inserted = '<p>Inserted before alpha</p>'.repeat(40);
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchWsMessage({
      content:
        '<h1 id="readme">README</h1>' +
        '<p>Paragraph 1</p>'.repeat(12) +
        inserted +
        '<h2 id="alpha">Alpha</h2><p>Alpha moved down</p>' +
        '<h2 id="beta">Beta</h2><p>Beta body</p>' +
        '<p>Tail</p>'.repeat(12),
      toc:
        '<ul>' +
        '<li><a href="#readme">README</a></li>' +
        '<li><a href="#alpha">Alpha</a></li>' +
        '<li><a href="#beta">Beta</a></li>' +
        '</ul>',
      file: 'README.md'
    });
  });

  await expect(page.locator('#content')).toContainText('Alpha moved down');
  await expect.poll(() => activeTocLabel(page)).toBe('README');
});

test('抑止中のスクロールも抑止明けに目次activeへ反映される', async ({ page }) => {
  const positions = await loadDenseHeadingFixture(page);

  await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.alphaTop - positions.activationOffset + 8);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');

  await page.evaluate(() => {
    const repeated = '<p>Updated paragraph</p>'.repeat(12);
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchWsMessage({
      content:
        '<h1 id="readme">README</h1>' +
        repeated +
        '<h2 id="alpha">Alpha</h2><p>Alpha body updated</p>' +
        '<h2 id="beta">Beta</h2><p>Beta body updated</p>' +
        repeated,
      toc:
        '<ul>' +
        '<li><a href="#readme">README</a></li>' +
        '<li><a href="#alpha">Alpha</a></li>' +
        '<li><a href="#beta">Beta</a></li>' +
        '</ul>',
      file: 'README.md'
    });
    window.scrollTo(0, document.getElementById('beta')!.getBoundingClientRect().top + window.scrollY);
  });

  await expect(page.locator('#content')).toContainText('Alpha body updated');
  await expect.poll(() => activeTocLabel(page)).toBe('Beta');
});

test('同一見出しのburst更新でも目次activeが点滅しない', async ({ page }) => {
  const positions = await loadDenseHeadingFixture(page);

  await page.evaluate((scrollTop) => window.scrollTo(0, scrollTop), positions.alphaTop - positions.activationOffset + 8);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');

  await page.evaluate(() => {
    const toc = document.getElementById('toc')!;
    window.__tocActiveChanges = [];
    let lastLabel = '';
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

  await page.evaluate(() => {
    const repeated = '<p>Burst paragraph</p>'.repeat(12);
    const currentToc = document.getElementById('toc')!.innerHTML;
    const payload = {
      content:
        '<h1 id="readme">README</h1>' +
        repeated +
        '<h2 id="alpha">Alpha</h2><p>Alpha burst</p>' +
        '<h2 id="beta">Beta</h2><p>Beta burst</p>' +
        repeated,
      toc: currentToc,
      file: 'README.md'
    };
    const dispatchWsMessage = window.__dispatchWsMessage;
    if (!dispatchWsMessage) {
      throw new Error('WebSocket test harness dispatcher is not initialized');
    }
    dispatchWsMessage(payload);
    dispatchWsMessage(payload);
    dispatchWsMessage(payload);
  });

  await expect(page.locator('#content')).toContainText('Alpha burst');
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');
  await page.waitForTimeout(250);
  await expect.poll(() => activeTocLabel(page)).toBe('Alpha');

  const activeChanges = await page.evaluate(() => {
    const stopTocObserver = window.__stopTocObserver;
    const tocActiveChanges = window.__tocActiveChanges;
    if (!stopTocObserver || !tocActiveChanges) {
      throw new Error('TOC active change recorder is not initialized');
    }
    stopTocObserver();
    return tocActiveChanges.slice();
  });
  expect(activeChanges).toEqual(['Alpha']);
});
