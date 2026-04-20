const fs = require('node:fs/promises');
const path = require('node:path');
const { test, expect } = require('@playwright/test');

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
  const entries = await fs.readdir(fixtureDir, { withFileTypes: true });
  await Promise.all(entries
    .filter((entry) => entry.isFile() && entry.name.endsWith('.memo.md'))
    .map((entry) => fs.rm(path.join(fixtureDir, entry.name), { force: true })));
  await fs.rm(path.join(fixtureDir, '.markdown-view'), { recursive: true, force: true });
  await fs.writeFile(longPath, longContent);
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

test.beforeEach(async ({ page }) => {
  await resetLongFixture();
  await page.goto('/?file=long.md');
  await expect(page.locator('#content')).toContainText('TARGET BLOCK');
  // resetLongFixture の writeFile が watcher 経由 broadcast を発火し、WS 接続後に
  // updateContent が #content を差し替える。直後に .jump-highlight を付与すると
  // 再描画でクラスが消失し L92 相当テストの toBeVisible / scrollY 検証が失敗する。
  // enhanceContentInteractions が heading-anchor button を innerHTML に追加するため
  // content.js:1239 の no-op check (innerHTML === data.content) が常に mismatch し
  // 遅延 broadcast が必ず #content を再描画するという構造的 race（本 PR では test 側で
  // 回避）。debounce 300ms + WS 到達 + jitter を余裕で吸収するため 1000ms 待つ。
  await page.waitForTimeout(1000);
});

test('メモ出典クリックで本文の対応ブロックへスクロールしハイライトされる', async ({ page }) => {
  // 1. 中盤の段落を選択して引用追加 → メモタブが activate される
  await selectParagraphText(page, 'TARGET BLOCK');
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
async function lineBlockStartOf(page, needle) {
  return page.evaluate((text) => {
    const walker = document.createTreeWalker(document.getElementById('content'), NodeFilter.SHOW_TEXT);
    let node;
    while ((node = walker.nextNode())) {
      if (!node.textContent || !node.textContent.includes(text)) continue;
      let el = node.parentElement;
      while (el && !el.hasAttribute('data-line-block-start') && !el.hasAttribute('data-source-start-line')) {
        el = el.parentElement;
      }
      if (!el) return { start: -1, end: -1 };
      const start = parseInt(el.getAttribute('data-line-block-start') || el.getAttribute('data-source-start-line'), 10);
      const end = parseInt(el.getAttribute('data-line-block-end') || el.getAttribute('data-source-end-line'), 10);
      return { start, end };
    }
    return { start: -1, end: -1 };
  }, needle);
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
  // memo writeFile 由来の watcher broadcast が reload 後に到達して #content を
  // 差し替え、直後の .jump-highlight 検証が空振る race を避ける。beforeEach と
  // 同じく debounce 300ms + WS 到達 + jitter を吸収する 1000ms 待ちを入れる。
  await page.waitForTimeout(1000);
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
    const container = document.getElementById('content');
    const link = document.createElement('a');
    link.href = 'other.md';
    link.textContent = 'other';
    container.appendChild(link);
    container.appendChild(document.createTextNode(' L10 onwards'));
    try {
      return augmentHashWithTrailingLineHint(link, '');
    } finally {
      link.remove();
      // 末尾のテキストノードを除去（container の最後の child を削除）
      if (container.lastChild && container.lastChild.nodeType === Node.TEXT_NODE) {
        container.lastChild.remove();
      }
    }
  });
  // ガードが外れると `#L10` に augment される。空文字列のままなら正しくスキップされている
  expect(result).toBe('');
});

test('augmentHashWithTrailingLineHint は `L5abc` など英数字が続く場合は augment しない', async ({ page }) => {
  // L 数字の直後に英数字/アンダースコアが続く別トークン（例: `L5abc`, `L5_foo`）は行番号として
  // 採用しないことを検証。両端アンカーが外れると `#section-b:L5` に誤 augment される
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview');
    const link = document.createElement('a');
    link.href = '?file=long.md#section-b';
    link.textContent = 'dummy';
    container.appendChild(link);
    container.appendChild(document.createTextNode(' L5abc trailing'));
    try {
      return augmentHashWithTrailingLineHint(link, '#section-b');
    } finally {
      link.remove();
      if (container.lastChild && container.lastChild.nodeType === Node.TEXT_NODE) {
        container.lastChild.remove();
      }
    }
  });
  expect(result).toBe('#section-b');
});

test('augmentHashWithTrailingLineHint は `L10 onwards` のような散文では augment しない', async ({ page }) => {
  // ユーザー自作メモでリンク直後に行番号から始まる散文（`L10 onwards は詳しい` 等）が続く場合、
  // sibling textContent 全体が行番号トークンのみで占められないため augment しない。
  // 旧 regex（末尾アンカーなし）は先頭 `L10` を拾って `#intro:L10` に誤書換していた既知の
  // false positive を回帰させないことを担保（Codex review #4136142343 の再発防止）
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview');
    const link = document.createElement('a');
    link.href = '?file=spec.md#intro';
    link.textContent = 'spec';
    container.appendChild(link);
    container.appendChild(document.createTextNode(' L10 onwards は詳しい説明'));
    try {
      return augmentHashWithTrailingLineHint(link, '#intro');
    } finally {
      link.remove();
      if (container.lastChild && container.lastChild.nodeType === Node.TEXT_NODE) {
        container.lastChild.remove();
      }
    }
  });
  expect(result).toBe('#intro');
});

test('augmentHashWithTrailingLineHint は `L15-L17` 範囲形式を正しく hash 末尾に合成する', async ({ page }) => {
  // 範囲形式 positive branch を直接検証。regex の capture group 2 と suffix 生成
  // (`'L' + start + '-L' + end`) がともに機能することを担保
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview');
    const link = document.createElement('a');
    link.href = '?file=long.md#section-b';
    link.textContent = 'dummy';
    container.appendChild(link);
    container.appendChild(document.createTextNode(' L15-L17'));
    try {
      return augmentHashWithTrailingLineHint(link, '#section-b');
    } finally {
      link.remove();
      if (container.lastChild && container.lastChild.nodeType === Node.TEXT_NODE) {
        container.lastChild.remove();
      }
    }
  });
  expect(result).toBe('#section-b:L15-L17');
});

test('augmentHashWithTrailingLineHint は `L17-L15` 逆転範囲では start のみ採用', async ({ page }) => {
  // end < start（逆転）および end == start（単一行）は start 1 行に縮退する。
  // 将来 regex や suffix 生成を改変したとき「逆転時は null を返す」等の silent 仕様変更を検出する
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview');
    const link = document.createElement('a');
    link.href = '?file=long.md#section-b';
    link.textContent = 'dummy';
    container.appendChild(link);
    container.appendChild(document.createTextNode(' L17-L15'));
    try {
      return augmentHashWithTrailingLineHint(link, '#section-b');
    } finally {
      link.remove();
      if (container.lastChild && container.lastChild.nodeType === Node.TEXT_NODE) {
        container.lastChild.remove();
      }
    }
  });
  expect(result).toBe('#section-b:L17');
});

test('augmentHashWithTrailingLineHint は hash に行範囲が既にあれば link.nextSibling の L<n> で上書きしない', async ({ page }) => {
  // リンク href が明示的に行範囲を指定している場合 (`#section-b:L15`)、link.nextSibling の
  // `L20` は旧形式 citation の推測に過ぎないため、明示指定を上書きしないことを保証する。
  // parseLineHash(hash).lineRange が truthy のときの早期 return で実現されている
  const result = await page.evaluate(() => {
    const container = document.getElementById('memo-preview');
    const link = document.createElement('a');
    link.href = '?file=long.md#section-b:L15';
    link.textContent = 'dummy';
    container.appendChild(link);
    container.appendChild(document.createTextNode(' L20'));
    try {
      return augmentHashWithTrailingLineHint(link, '#section-b:L15');
    } finally {
      link.remove();
      if (container.lastChild && container.lastChild.nodeType === Node.TEXT_NODE) {
        container.lastChild.remove();
      }
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
    const container = document.getElementById('memo-preview');
    const link = document.createElement('a');
    link.href = '?file=long.md';
    link.textContent = 'dummy';
    container.appendChild(link);
    container.appendChild(document.createTextNode(' L42'));
    try {
      return {
        empty: augmentHashWithTrailingLineHint(link, ''),
        hashOnly: augmentHashWithTrailingLineHint(link, '#')
      };
    } finally {
      link.remove();
      if (container.lastChild && container.lastChild.nodeType === Node.TEXT_NODE) {
        container.lastChild.remove();
      }
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
  // beforeEach で /?file=long.md へ goto 済み。最初の h2 が見えていることを確認する
  await expect(page.locator('#content h2').first()).toBeVisible();

  // セットアップ: #content の最初の h2 に .jump-highlight を付与
  await page.evaluate(() => {
    const h = document.querySelector('#content h2');
    h.classList.add('jump-highlight');
  });

  // /api/content から現在の data を取得して updateContent を直接呼ぶ。
  // lastAppliedContent と一致するため再描画が起きないことを期待する。
  await page.evaluate(async () => {
    const res = await fetch('/api/content');
    const data = await res.json();
    window.updateContent(data, {});
  });

  // 再描画されなかったので .jump-highlight が残っているはず
  const stillHighlighted = await page.evaluate(() => {
    const h = document.querySelector('#content h2');
    return h && h.classList.contains('jump-highlight');
  });
  expect(stillHighlighted).toBe(true);
});
