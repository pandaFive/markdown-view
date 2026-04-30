import fs from 'node:fs/promises';
import path from 'node:path';
import { test, expect, type Page } from '@playwright/test';
import { resetStandardFixtures, selectParagraphText, updateContent } from './helpers';

const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');

const longContent = [
  '# Long Document',
  '',
  '## Section A',
  '',
  'Paragraph A1 content.',
  '',
  'Paragraph A2 content.',
  '',
  'Paragraph A3 content.',
  '',
  // 出典クリックのスクロール検証対象が初期 viewport 内に収まると scrollY が 0 のままになる。
  // Section B を十分下げ、テスト前提である「同一ファイル内ジャンプでスクロールが発生する」を固定する。
  ...Array.from({ length: 24 }, (_, index) => `Viewport filler before target ${index + 1}.`),
  '',
  '## Section B',
  '',
  'Paragraph B1 content.',
  '',
  'Paragraph B2 content TARGET BLOCK.',
  '',
  'Paragraph B3 content.',
  '',
  '## Section C',
  '',
  'Paragraph C1 content.',
  '',
  'Paragraph C2 content.',
  '',
  'Paragraph C3 content.',
  '',
  '## Section D',
  '',
  'Paragraph D1 content.',
  '',
  'Paragraph D2 content.',
  '',
  'Paragraph D3 content.',
  '',
  '## Section Blockquote',
  '',
  '> Outer quote opening line.',
  '>',
  '> > Nested blockquote inner TARGET line.',
  '>',
  '> Outer quote closing line.',
  '',
  '## Section Multiline',
  '',
  'Multi line one content.',
  'Multi line two MULTILINE TARGET.',
  'Multi line three content.',
  ''
].join('\n');

async function resetLongFixture() {
  const longPath = path.join(fixtureDir, 'long.md');
  await resetStandardFixtures();
  await fs.writeFile(longPath, longContent);
}

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.__MV_E2E__ = true;
  });
  await resetLongFixture();
  await page.goto('/?file=long.md');
  await expect(page.locator('#content')).toContainText('TARGET BLOCK');
});

test('メモ出典クリックで本文の対応ブロックへスクロールしハイライトされる', async ({ page }) => {
  // 初回WebSocket updateContent が未適用のまま出典クリックすると、クリック直後の
  // live update 再描画で一時ハイライトが消えるため、ユーザー操作前に初期同期を待つ。
  await page.waitForFunction(() => {
    return window.markdownViewTestHooks.lastAppliedContent !== null;
  });

  // 1. 中盤の段落を選択して引用追加 → メモタブが activate される
  await selectParagraphText(page, 'TARGET BLOCK', { match: 'contains' });
  const quoteButton = page.locator('#quote-selection-action');
  await expect(quoteButton).toBeVisible();
  await quoteButton.click();
  await expect(page.locator('#panel-memo.active')).toBeVisible();
  await expect(page.locator('#memo-save-status')).toHaveText('保存済み');

  // 2. メモプレビューの出典リンク href が `:L<n>` フラグメントを含むことを確認
  const sourceLink = page.locator('#memo-preview a[href*="long.md"]').first();
  await expect(sourceLink).toBeVisible();
  const href = await sourceLink.getAttribute('href');
  expect(href).toMatch(/long\.md#section-b:L\d+/);

  // 3. 本文を一度トップへスクロールしてから出典リンクをクリック
  await page.evaluate(() => window.scrollTo(0, 0));
  await sourceLink.click();

  // 4. スクロール位置が変化したことを確認（同じファイル内ジャンプ）
  await expect.poll(() => page.evaluate(() => window.scrollY), {
    timeout: 3000
  }).toBeGreaterThan(0);

  // 5. 対応ブロックに .jump-highlight クラスが一時付与される
  const highlighted = page.locator('#content .jump-highlight');
  await expect(highlighted).toBeVisible();
  await expect(highlighted).toContainText('TARGET BLOCK');

  // 6. アニメーション完了後 (~2.5秒) にクラスが除去される
  await expect(page.locator('#content .jump-highlight')).toHaveCount(0, { timeout: 5000 });
});

/// 指定したテキストを含む block の data-line-block-start を返す。
/// 複数行段落・blockquote内paragraph どちらにも対応する。
async function lineBlockStartOf(page: Page, needle: string) {
  return page.evaluate((text) => {
    const walker = document.createTreeWalker(document.getElementById('content')!, NodeFilter.SHOW_TEXT);
    let node;
    while ((node = walker.nextNode())) {
      if (!node.textContent || !node.textContent.includes(text)) continue;
      let el = node.parentElement;
      while (el && !el.hasAttribute('data-line-block-start') && !el.hasAttribute('data-source-start-line')) {
        el = el.parentElement;
      }
      if (!el) return { start: -1, end: -1 };
      const start = parseInt(el.getAttribute('data-line-block-start') || el.getAttribute('data-source-start-line') || '', 10);
      const end = parseInt(el.getAttribute('data-line-block-end') || el.getAttribute('data-source-end-line') || '', 10);
      return { start, end };
    }
    return { start: -1, end: -1 };
  }, needle);
}

async function fetchLongContent(page: Page, stepLabel: string) {
  return page.evaluate(async (label) => {
    const res = await fetch('/api/content?file=long.md');
    if (!res.ok) throw new Error(`${label} fetch failed: ${res.status}`);
    const data = await res.json();
    if (typeof data.content !== 'string') throw new Error(`${label} data.content missing`);
    return data as MvE2E.UpdateContentPayload;
  }, stepLabel);
}

test('不正な行番号 #L0 はジャンプせずスクロール位置を維持する', async ({ page }) => {
  // renderer は 1-indexed。`scrollToLineRange` は targetLine < 1 で早期return する
  await page.goto('/?file=long.md#L0');
  await expect(page.locator('#content')).toContainText('Long Document');
  // rAFによるscroll復元が走る時間を与える
  await page.waitForTimeout(200);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
  await expect(page.locator('#content .jump-highlight')).toHaveCount(0);
});

test('負の行番号 #L-5 は heading にマッチせずジャンプしない', async ({ page }) => {
  // `^L(\d+)` は負数にマッチしないため headingId="L-5" として扱われ、該当idは存在しない
  await page.goto('/?file=long.md#L-5');
  await expect(page.locator('#content')).toContainText('Long Document');
  await page.waitForTimeout(200);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
  await expect(page.locator('#content .jump-highlight')).toHaveCount(0);
});

test('行範囲が範囲外の場合は heading fallback へスクロールする', async ({ page }) => {
  // 行L99999は存在しないが heading `section-c` は存在する。applyContentAnchorNavigation が
  // 行範囲スクロールに失敗したあと heading fallback に切り替わることを検証する
  await page.goto('/?file=long.md#section-c:L99999');
  await expect(page.locator('#content')).toContainText('Long Document');

  await expect.poll(() => page.evaluate(() => window.scrollY), { timeout: 3000 })
    .toBeGreaterThan(0);

  // heading fallback 経路は jump-highlight を付与しない（scrollToLineRange のみトリガする）
  await expect(page.locator('#content .jump-highlight')).toHaveCount(0);

  // section-c が viewport 内に表示されていることを確認（固定ヘッダー等のoffsetで完全に上端ではない）
  const sectionCInView = await page.evaluate(() => {
    const el = document.getElementById('section-c');
    if (!el) return false;
    const rect = el.getBoundingClientRect();
    return rect.top >= 0 && rect.top < window.innerHeight;
  });
  expect(sectionCInView).toBe(true);
});

test('ネストしたblockquote内の行へジャンプすると最内段落がハイライトされる', async ({ page }) => {
  // 外側blockquoteと内側blockquoteの双方がlineを含むため、最狭マッチの内側<p>が選ばれる必要がある
  await page.goto('/?file=long.md');
  await expect(page.locator('#content')).toContainText('Nested blockquote inner TARGET');

  const range = await lineBlockStartOf(page, 'Nested blockquote inner TARGET');
  expect(range.start).toBeGreaterThan(0);

  await page.goto(`/?file=long.md#L${range.start}`);
  const highlighted = page.locator('#content .jump-highlight');
  await expect(highlighted).toBeVisible();
  await expect(highlighted).toContainText('Nested blockquote inner TARGET');

  // 外側のblockquote全体ではなく innermost <p> がハイライト対象
  const tagName = await highlighted.evaluate((el) => el.tagName.toLowerCase());
  expect(tagName).toBe('p');
});

test('旧形式メモ（リンク外の行番号）の出典クリックでも行範囲ジャンプできる', async ({ page }) => {
  // 行範囲ジャンプ機能導入以前に生成されたメモは `出典: [...](...#heading) L15` のように
  // 行範囲がリンク外テキストとして並ぶ。このレガシー形式でも fine-grained ジャンプできることを検証する。
  // fixture 編集時の行ズレを避けるため TARGET BLOCK の行番号は lineBlockStartOf で動的取得する
  const range = await lineBlockStartOf(page, 'Paragraph B2 content TARGET BLOCK');
  expect(range.start).toBeGreaterThan(0);

  const memoPath = path.join(fixtureDir, '.long.md.memo.md');
  const legacyMemo = [
    '> Paragraph B2 content TARGET BLOCK.',
    '',
    `出典: [long.md > Section B](?file=long.md#section-b) L${range.start}`,
    ''
  ].join('\n');
  await fs.writeFile(memoPath, legacyMemo);

  // サーバー側で初期描画にメモを反映させるためリロード
  await page.reload();
  await page.locator('.sidebar-tab[data-tab="memo"]').click();
  await expect(page.locator('#panel-memo.active')).toBeVisible();

  const sourceLink = page.locator('#memo-preview a[href="?file=long.md#section-b"]').first();
  await expect(sourceLink).toBeVisible();
  // 隣接ノード (text/span) の textContent に `L<n>` が存在することを確認（旧形式の identifying 条件）
  const tail = await sourceLink.evaluate((link) => (link.nextSibling ? link.nextSibling.textContent : ''));
  expect(tail).toMatch(new RegExp(`^\\s*L${range.start}\\b`));

  await page.evaluate(() => window.scrollTo(0, 0));
  await sourceLink.click();

  await expect.poll(() => page.evaluate(() => window.scrollY), { timeout: 3000 }).toBeGreaterThan(0);

  // heading (section-b) ではなく 対応 paragraph に着地
  const highlighted = page.locator('#content .jump-highlight');
  await expect(highlighted).toBeVisible();
  await expect(highlighted).toContainText('TARGET BLOCK');

  await expect(page.locator('#content .jump-highlight')).toHaveCount(0, { timeout: 5000 });
});

test('augmentHashWithTrailingLineHint は memo-preview 外のリンクでは hash を変えない', async ({ page }) => {
  // スコープガードの回帰防止。本文コンテンツの自然文 `[spec](spec.md) L10 onwards...` などが
  // 誤ってジャンプ対象にならないことを、関数を直接呼び出して検証する
  const result = await page.evaluate(() => {
    const container = document.getElementById('content')!;
    const link = document.createElement('a');
    link.href = 'other.md';
    link.textContent = 'other';
    const lineHint = document.createTextNode(' L10 onwards');
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return window.markdownViewTestHooks.augmentHashWithTrailingLineHint(link, '');
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  // ガードが外れると `#L10` に augment される。空文字列のままなら正しくスキップされている
  expect(result).toBe('');
});

test('augmentHashWithTrailingLineHint は `L5abc` など英数字が続く場合は augment しない', async ({ page }) => {
  // L 数字の直後に英数字/アンダースコアが続く別トークン（例: `L5abc`, `L5_foo`）は行番号として
  // 採用しないことを検証。両端アンカーが外れると `#section-b:L5` に誤 augment される
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview')!;
    const link = document.createElement('a');
    link.href = '?file=long.md#section-b';
    link.textContent = 'dummy';
    const lineHint = document.createTextNode(' L5abc trailing');
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return window.markdownViewTestHooks.augmentHashWithTrailingLineHint(link, '#section-b');
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  expect(result).toBe('#section-b');
});

test('augmentHashWithTrailingLineHint は `L10 onwards` のような散文では augment しない', async ({ page }) => {
  // ユーザー自作メモでリンク直後に行番号から始まる散文（`L10 onwards は詳しい` 等）が続く場合、
  // sibling textContent 全体が行番号トークンのみで占められないため augment しない。
  // 旧 regex（末尾アンカーなし）は先頭 `L10` だけを拾って `#intro:L10` に誤書換していたため、
  // sibling 全体が行番号トークンだけで構成されることを固定する。
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview')!;
    const link = document.createElement('a');
    link.href = '?file=spec.md#intro';
    link.textContent = 'spec';
    const lineHint = document.createTextNode(' L10 onwards は詳しい説明');
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return window.markdownViewTestHooks.augmentHashWithTrailingLineHint(link, '#intro');
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  expect(result).toBe('#intro');
});

test('augmentHashWithTrailingLineHint は ELEMENT_NODE sibling の textContent から行番号を補完する', async ({ page }) => {
  // renderer が旧形式メモの行番号テキストを span 等でラップしても、link.nextSibling の
  // textContent から `L<n>` を読み、TEXT_NODE と同じ hash 補完を行うことを固定する。
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview')!;
    const link = document.createElement('a');
    link.href = '?file=long.md#section-b';
    link.textContent = 'dummy';
    const lineHint = document.createElement('span');
    lineHint.textContent = ' L15';
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return window.markdownViewTestHooks.augmentHashWithTrailingLineHint(link, '#section-b');
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  expect(result).toBe('#section-b:L15');
});

test('augmentHashWithTrailingLineHint は ELEMENT_NODE sibling の散文を行番号扱いしない', async ({ page }) => {
  // ELEMENT_NODE 経路でも TEXT_NODE と同じく、sibling textContent 全体が行番号トークン
  // のみで構成されない散文は augment 対象にしないことを固定する。
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview')!;
    const link = document.createElement('a');
    link.href = '?file=spec.md#intro';
    link.textContent = 'spec';
    const lineHint = document.createElement('span');
    lineHint.textContent = ' L10 onwards は詳しい説明';
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return window.markdownViewTestHooks.augmentHashWithTrailingLineHint(link, '#intro');
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  expect(result).toBe('#intro');
});

test('augmentHashWithTrailingLineHint は `L15-L17` 範囲形式を正しく hash 末尾に合成する', async ({ page }) => {
  // 範囲形式 positive branch を直接検証。regex の capture group 2 と suffix 生成
  // (`'L' + start + '-L' + end`) がともに機能することを担保
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview')!;
    const link = document.createElement('a');
    link.href = '?file=long.md#section-b';
    link.textContent = 'dummy';
    const lineHint = document.createTextNode(' L15-L17');
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return window.markdownViewTestHooks.augmentHashWithTrailingLineHint(link, '#section-b');
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  expect(result).toBe('#section-b:L15-L17');
});

test('augmentHashWithTrailingLineHint は `L17-L15` 逆転範囲では start のみ採用', async ({ page }) => {
  // end < start（逆転）および end == start（単一行）は start 1 行に縮退する。
  // 将来 regex や suffix 生成を改変したとき「逆転時は null を返す」等の silent 仕様変更を検出する
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview')!;
    const link = document.createElement('a');
    link.href = '?file=long.md#section-b';
    link.textContent = 'dummy';
    const lineHint = document.createTextNode(' L17-L15');
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return window.markdownViewTestHooks.augmentHashWithTrailingLineHint(link, '#section-b');
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  expect(result).toBe('#section-b:L17');
});

test('augmentHashWithTrailingLineHint は hash に行範囲が既にあれば link.nextSibling の L<n> で上書きしない', async ({ page }) => {
  // リンク href が明示的に行範囲を指定している場合 (`#section-b:L15`)、link.nextSibling の
  // `L20` は旧形式 citation の推測に過ぎないため、明示指定を上書きしないことを保証する。
  // parseLineHash(hash).lineRange が truthy のときの早期 return で実現されている
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview')!;
    const link = document.createElement('a');
    link.href = '?file=long.md#section-b:L15';
    link.textContent = 'dummy';
    const lineHint = document.createTextNode(' L20');
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return window.markdownViewTestHooks.augmentHashWithTrailingLineHint(link, '#section-b:L15');
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  expect(result).toBe('#section-b:L15');
});

test('augmentHashWithTrailingLineHint は空 hash の合成形は #L<n>（#:L<n> にはしない）', async ({ page }) => {
  // 2 つある合成分岐のうち、空 hash 経路では `hash + ':' + suffix` ではなく `'#' + suffix`
  // を選ぶことを固定。headingId を持たない hash の正準形は `#L42` であり、`#:L42` は
  // parseLineHash では一応パース可能だが headingId=null の非直感的フラグメントを生成するため
  // 意図的に避けている。この選択を silent に反転させる退行を検出する
  const results = await page.evaluate(() => {
    const container = document.getElementById('memo-preview')!;
    const link = document.createElement('a');
    link.href = '?file=long.md';
    link.textContent = 'dummy';
    const lineHint = document.createTextNode(' L42');
    container.appendChild(link);
    container.appendChild(lineHint);
    try {
      return {
        empty: window.markdownViewTestHooks.augmentHashWithTrailingLineHint(link, ''),
        hashOnly: window.markdownViewTestHooks.augmentHashWithTrailingLineHint(link, '#')
      };
    } finally {
      link.remove();
      lineHint.remove();
    }
  });
  expect(results.empty).toBe('#L42');
  expect(results.hashOnly).toBe('#L42');
});

test('複数行にまたがる段落の中間行へのジャンプは段落全体を最狭マッチとして選ぶ', async ({ page }) => {
  // 複数行で1つの<p>になる段落。中間行を指定しても同じ段落がハイライトされる
  await page.goto('/?file=long.md');
  await expect(page.locator('#content')).toContainText('MULTILINE TARGET');

  const range = await lineBlockStartOf(page, 'MULTILINE TARGET');
  expect(range.start).toBeGreaterThan(0);
  expect(range.end).toBeGreaterThanOrEqual(range.start);

  const midLine = Math.floor((range.start + range.end) / 2);
  expect(midLine).toBeGreaterThanOrEqual(range.start);
  expect(midLine).toBeLessThanOrEqual(range.end);

  await page.goto(`/?file=long.md#L${midLine}`);
  const highlighted = page.locator('#content .jump-highlight');
  await expect(highlighted).toBeVisible();
  await expect(highlighted).toContainText('MULTILINE TARGET');
});

test('同じdata.contentでの2回目updateContentは.jump-highlightを消さない', async ({ page }) => {
  // beforeEach で /?file=long.md へ goto 済み
  await expect(page.locator('#content h2').first()).toBeVisible();

  // Step 1: 1 回目 updateContent で lastAppliedContent を data.content に prime。
  // 初期値 null は仕様 (bootstrap.js の lastAppliedContent 宣言コメント参照) のため
  // ここでの 1 回目は必ず再描画される、を前提に Step 2/3 が組まれている。
  // fetch / data.content の異常を黙殺すると Step 3 の no-op が「cache 不一致」ではなく
  // 「両方 undefined で skip」で偽陽性化するため必ず ok / 型を assert する。
  const primeContent = await fetchLongContent(page, 'Step 1');
  await updateContent(page, primeContent, {});
  const primeContentLen = primeContent.content.length;
  expect(primeContentLen).toBeGreaterThan(0);

  // Step 2: prime 後に .jump-highlight を付与
  await page.evaluate(() => {
    const h = document.querySelector('#content h2')!;
    h.classList.add('jump-highlight');
  });

  // Step 3: 2 回目 updateContent (同一 data.content) → cache 一致で no-op。
  // 再描画されないため .jump-highlight が保持されることを検証
  const verifyContent = await fetchLongContent(page, 'Step 3');
  await updateContent(page, verifyContent, {});
  const verifyContentLen = verifyContent.content.length;
  expect(
    verifyContentLen,
    'Step1→Step3 で /api/content?file=long.md の content 長が変化 (cache 比較の前提崩壊)'
  ).toBe(primeContentLen);

  const stillHighlighted = await page.evaluate(() => {
    const h = document.querySelector('#content h2');
    return h && h.classList.contains('jump-highlight');
  });
  expect(stillHighlighted).toBe(true);
});

test('updateContentはdata.content/toc欠落時に契約違反warnを出す', async ({ page }) => {
  // beforeEach で /?file=long.md へ goto 済み
  await expect(page.locator('#content h2').first()).toBeVisible();

  const contractWarnings: { text: string; argsLength: number; context: unknown }[] = [];
  const pageErrors: string[] = [];
  page.on('console', async (msg) => {
    if (msg.type() !== 'warning') return;
    if (!msg.text().includes('updateContent') || !msg.text().includes('契約違反')) return;
    const args = msg.args();
    const contextArg = args[1];
    contractWarnings.push({
      text: msg.text(),
      argsLength: args.length,
      context: contextArg ? await contextArg.jsonValue() : null
    });
  });
  page.on('pageerror', (error) => {
    pageErrors.push(error.message);
  });
  const OK_CONTENT = '<p>ok</p>';
  const OK_TOC = '<ul></ul>';
  const contractWarningSuffix = ' (契約違反)';

  // ケース 1: 正常呼び出し → warn は出ない
  await updateContent(page, { content: OK_CONTENT, toc: OK_TOC }, {});
  expect(contractWarnings).toHaveLength(0);

  // key欠落、null、data自体のnull/undefined、WSバッファ経路を網羅し、
  // 契約違反がDOM更新と重複抑制でサイレント化しないことを検証する。
  // 以下の unknown 経由キャストは、正常系型を広げずに契約違反入力だけを再現するためのもの。
  // ケース 2: content だけ欠落 → 'content が欠落または不正' warn
  await updateContent(page, { toc: OK_TOC } as unknown as MvE2E.UpdateContentPayload, {});

  // ケース 3: toc だけ欠落 → 'toc が欠落または不正' warn
  await updateContent(page, { content: OK_CONTENT } as unknown as MvE2E.UpdateContentPayload, {});

  // ケース 4: content と toc 両方欠落 → 'content, toc が欠落または不正' warn
  await updateContent(page, {} as unknown as MvE2E.UpdateContentPayload, {});

  // ケース 5: content が null → 'content が欠落または不正' warn
  await updateContent(page, { content: null, toc: OK_TOC } as unknown as MvE2E.UpdateContentPayload, {});

  // ケース 6: toc が null → 'toc が欠落または不正' warn
  await updateContent(page, { content: OK_CONTENT, toc: null } as unknown as MvE2E.UpdateContentPayload, {});

  // ケース 7: content が number → 'content が欠落または不正' warn
  await updateContent(page, { content: 123, toc: OK_TOC } as unknown as MvE2E.UpdateContentPayload, {});

  // ケース 8: data 自体が null → 'content, toc が欠落または不正' warn
  await updateContent(page, null as unknown as MvE2E.UpdateContentPayload, {});

  // ケース 9: data 自体が undefined → 'content, toc が欠落または不正' warn
  await updateContent(page, undefined as unknown as MvE2E.UpdateContentPayload, {});

  // ケース 10: 同じ全欠落 payload がWSバッファ経由で再度来ても warn される
  await page.evaluate(() => {
    window.markdownViewTestHooks.scheduleBufferedLiveUpdate({} as unknown as MvE2E.UpdateContentPayload);
  });

  await expect.poll(() => contractWarnings.length).toBe(9);

  const contractWarningSummaries = contractWarnings.map((w) => w.text.split(contractWarningSuffix)[0]);
  expect(contractWarningSummaries).toEqual([
    '[markdown-view] updateContent: content が欠落または不正',
    '[markdown-view] updateContent: toc が欠落または不正',
    '[markdown-view] updateContent: content, toc が欠落または不正',
    '[markdown-view] updateContent: content が欠落または不正',
    '[markdown-view] updateContent: toc が欠落または不正',
    '[markdown-view] updateContent: content が欠落または不正',
    '[markdown-view] updateContent: content, toc が欠落または不正',
    '[markdown-view] updateContent: content, toc が欠落または不正',
    '[markdown-view] updateContent: content, toc が欠落または不正'
  ]);
  expect(contractWarnings.map((w) => w.argsLength)).toEqual(Array(9).fill(2));
  expect(contractWarnings.map((w) => w.context)).toEqual([
    { missing: ['content'], file: null, contentLength: null, tocLength: OK_TOC.length },
    { missing: ['toc'], file: null, contentLength: OK_CONTENT.length, tocLength: null },
    { missing: ['content', 'toc'], file: null, contentLength: null, tocLength: null },
    { missing: ['content'], file: null, contentLength: null, tocLength: OK_TOC.length },
    { missing: ['toc'], file: null, contentLength: OK_CONTENT.length, tocLength: null },
    { missing: ['content'], file: null, contentLength: null, tocLength: OK_TOC.length },
    { missing: ['content', 'toc'], file: null, contentLength: null, tocLength: null },
    { missing: ['content', 'toc'], file: null, contentLength: null, tocLength: null },
    { missing: ['content', 'toc'], file: null, contentLength: null, tocLength: null }
  ]);
  expect(pageErrors).toEqual([]);
  await expect(page.locator('#content')).toBeVisible();
  await expect(page.locator('#content')).not.toHaveText('null');
  await expect(page.locator('#toc')).toBeAttached();
});

test('data.contentが変わるとupdateContentは再描画される (cache invariantの逆方向)', async ({ page }) => {
  // beforeEach で /?file=long.md へ goto 済み
  await expect(page.locator('#content h2').first()).toBeVisible();

  // Step 1: 実 fetch で prime → lastAppliedContent に実 HTML を入れる。
  // 既存テスト「同じdata.contentでの2回目...」と同形のエラーメッセージ prefix で識別性を維持。
  const primeContent = await fetchLongContent(page, 'prime');
  await updateContent(page, primeContent, {});
  const primeContentLen = primeContent.content.length;
  expect(primeContentLen).toBeGreaterThan(0);

  // Step 2: 再描画 sentinel として .jump-highlight を付与
  await page.evaluate(() => {
    const h = document.querySelector('#content h2')!;
    h.classList.add('jump-highlight');
  });

  // Step 3: ダミー HTML で updateContent → cache 不一致で再描画される
  // toc も <ul></ul> を渡して契約違反 warn が出ないようにする
  const DUMMY = '<h1 data-test-changed>changed content</h1>';
  await updateContent(page, { content: DUMMY, toc: '<ul></ul>' }, {});

  const afterRerender = await page.evaluate(() => ({
    highlighted: !!document.querySelector('#content .jump-highlight'),
    hasDummy: !!document.querySelector('#content [data-test-changed]')
  }));
  expect(afterRerender.highlighted, 'cache 不一致時に再描画されず .jump-highlight が残存').toBe(false);
  expect(afterRerender.hasDummy, 'ダミー HTML が反映されていない').toBe(true);

  // Step 4: 再度 sentinel 付与 → 同一ダミー HTML で updateContent → cache 一致で no-op
  // lastAppliedContent が新値で更新されたことの逆方向検証
  await page.evaluate(() => {
    const h = document.querySelector('#content [data-test-changed]')!;
    h.classList.add('jump-highlight');
  });
  await updateContent(page, { content: DUMMY, toc: '<ul></ul>' }, {});

  const afterNoOp = await page.evaluate(() =>
    !!document.querySelector('#content .jump-highlight')
  );
  expect(afterNoOp, 'cache 更新後の同一呼び出しが no-op にならず再描画された').toBe(true);
});
