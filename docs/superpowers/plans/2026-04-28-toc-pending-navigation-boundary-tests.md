# TOC pending navigation Boundary Tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** TOC pending navigation の連続クリック、slack 境界、小揺らし中 active 遷移を E2E で固定する。

**Architecture:** `tests/e2e/text_selection_defer.spec.ts` に黒箱 E2E テストを追加する。既存の dense heading fixture と TOC helper を使い、production 側の新しいグローバル API は増やさない。`src/template/assets/js/sidebar.js` は追加テストが現行挙動の不備を示した場合だけ最小修正する。

**Tech Stack:** TypeScript, Playwright, Chromium E2E, Rust preview server via `cargo run`

---

## File Structure

- Modify: `tests/e2e/text_selection_defer.spec.ts`
  - 既存 helper 群の近くに、scrollY 取得、TOC tracking frame 待機、TOC active 変化記録 helper を追加する。
  - TOC pending navigation 既存テスト群の近くに、3 本の E2E テストを追加する。
- Conditional Modify: `src/template/assets/js/sidebar.js`
  - 新規 E2E が失敗し、現行 pending navigation の不備が確認できた場合だけ、`markPendingTocNavigation` / `getPendingTocNavigationId` / `setActiveTocLink` 周辺を最小修正する。
- Modify: `docs/todo/BACKLOG.md`
  - E2E が通った後、`TOC pending navigation の境界回帰テスト強化` を `Done` に移し、完了根拠のコミットを記録する。

## Task 1: E2E helper を同一 spec 内に追加する

**Files:**
- Modify: `tests/e2e/text_selection_defer.spec.ts`

- [ ] **Step 1: helper を追加する**

`clickTocLink` の直後、`stabilizeWebSocketHarness` の前に以下を追加する。

```ts
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
    window.__stopTocObserver = undefined;
    return tocActiveChanges.slice();
  });
}
```

- [ ] **Step 2: typecheck を実行する**

Run:

```bash
npm run typecheck
```

Expected: PASS。`Window.__tocActiveChanges` と `Window.__stopTocObserver` は `tests/e2e/globals.d.ts` で定義済みなので、新しい型定義は不要。

- [ ] **Step 3: helper 追加をコミットする**

```bash
git add tests/e2e/text_selection_defer.spec.ts
git commit -m "test: TOC active監視ヘルパーを追加"
```

## Task 2: TOC pending navigation の境界 E2E を追加する

**Files:**
- Modify: `tests/e2e/text_selection_defer.spec.ts`

- [ ] **Step 1: 3 本の E2E テストを追加する**

`test('目次クリック直後の小揺らしではクリック先のactiveが維持される', ...)` の直後に以下を追加する。

```ts
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
```

- [ ] **Step 2: 対象 E2E を実行する**

Run:

```bash
npm run test:e2e -- text_selection_defer.spec.ts
```

Expected: PASS。失敗した場合、失敗したテスト名、actual active label、scrollY 差分を確認し、Task 3 へ進む。PASS した場合は Task 4 へ進む。

- [ ] **Step 3: typecheck を実行する**

Run:

```bash
npm run typecheck
```

Expected: PASS。

- [ ] **Step 4: E2E 追加をコミットする**

```bash
git add tests/e2e/text_selection_defer.spec.ts
git commit -m "test: TOC pending navigation境界を固定"
```

## Task 3: 条件付きで sidebar の pending 判定を最小修正する

**Files:**
- Conditional Modify: `src/template/assets/js/sidebar.js`
- Test: `tests/e2e/text_selection_defer.spec.ts`

この Task は Task 2 の E2E が失敗した場合だけ実行する。Task 2 が PASS した場合は実行しない。

- [ ] **Step 1: 連続クリック失敗時の修正を適用する**

連続クリックテストで最後のクリック先が `Beta` にならない場合は、`markPendingTocNavigation` が id と期限を常に上書きする形になっているか確認し、関数を以下の形にする。

```js
function markPendingTocNavigation(id) {
  if (!findTrackedHeading(id)) return;
  pendingTocNavigationId = id;
  pendingTocNavigationUntil = Date.now() + TOC_NAVIGATION_GRACE_MS;
  setActiveTocLink(id);
}
```

- [ ] **Step 2: slack 境界失敗時の修正を適用する**

slack 内外の判定が逆になる、または `SLACK + 2` 相当で pending が残る場合は、`getPendingTocNavigationId` の slack 判定を以下の形にする。

```js
function getPendingTocNavigationId(activationOffset) {
  if (!pendingTocNavigationId) return '';
  var heading = findTrackedHeading(pendingTocNavigationId);
  var navigationTop;
  var maxScrollTop;
  var currentScrollTop;
  if (!heading) {
    clearPendingTocNavigation();
    return '';
  }
  if (Date.now() > pendingTocNavigationUntil) {
    clearPendingTocNavigation();
    return '';
  }
  navigationTop = heading.getBoundingClientRect().top;
  currentScrollTop = window.scrollY || window.pageYOffset;
  maxScrollTop = Math.max(document.documentElement.scrollHeight - window.innerHeight, 0);
  if (
    navigationTop <= activationOffset + TOC_NAVIGATION_SLACK_PX &&
    navigationTop >= activationOffset - TOC_NAVIGATION_SLACK_PX
  ) {
    return heading.id;
  }
  if (currentScrollTop >= maxScrollTop - 1 && navigationTop < activationOffset - TOC_NAVIGATION_SLACK_PX) {
    return heading.id;
  }
  clearPendingTocNavigation();
  return '';
}
```

- [ ] **Step 3: active 点滅失敗時の修正を適用する**

小揺らし中に `Alpha` や空文字へ切り替わる場合は、`setActiveTocLink` が current / next 以外を最後に正規化する形になっているか確認し、関数末尾を以下の形にする。

```js
  currentActiveTocId = activeId;
  currentTocTracking.links.forEach(function(link, id) {
    if (link !== nextLink && link !== currentLink) {
      link.classList.toggle('active', id === activeId);
    }
  });
```

- [ ] **Step 4: 修正後に対象 E2E を実行する**

Run:

```bash
npm run test:e2e -- text_selection_defer.spec.ts
```

Expected: PASS。

- [ ] **Step 5: typecheck と Rust 検証を実行する**

Run:

```bash
npm run typecheck
./verify.sh
```

Expected: both PASS。

- [ ] **Step 6: sidebar 修正をコミットする**

```bash
git add src/template/assets/js/sidebar.js tests/e2e/text_selection_defer.spec.ts
git commit -m "fix: TOC pending navigation境界判定を安定化"
```

## Task 4: BACKLOG を完了状態へ更新する

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: 未完了項目を Done へ移す**

`docs/todo/BACKLOG.md` の P2 から `TOC pending navigation の境界回帰テスト強化` を削除し、`## Done` の先頭へ以下の完了項目を追加する。完了根拠には、直前の実装コミットを `git log --oneline -1 -- tests/e2e/text_selection_defer.spec.ts src/template/assets/js/sidebar.js` で確認して記入する。

```markdown
- [x] TOC pending navigation の境界回帰テスト強化
  - ファイル: `tests/e2e/text_selection_defer.spec.ts`
  - 内容:
    - grace 400ms 以内の連続 TOC クリックで最後のクリック先に収束することを検証
    - `TOC_NAVIGATION_SLACK_PX` の内側 / 外側で pending 維持と通常判定復帰が分かれることを検証
    - `MutationObserver` で小揺らし中にクリック先以外へ active が切り替わらないことを検証
  - 完了根拠: `test: TOC pending navigation境界を固定`
  - 由来: PR #77 レビュー (2026-04-20)
```

- [ ] **Step 2: BACKLOG の該当項目が重複していないことを確認する**

Run:

```bash
rg -n "TOC pending navigation の境界回帰テスト強化|TOC_NAVIGATION_SLACK_PX|MutationObserver" docs/todo/BACKLOG.md
```

Expected: `TOC pending navigation の境界回帰テスト強化` は `Done` に 1 件だけ表示される。

- [ ] **Step 3: BACKLOG 更新をコミットする**

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: TOC pending navigation完了をBACKLOGに反映"
```

## Task 5: 最終検証と完了確認

**Files:**
- Verify: `tests/e2e/text_selection_defer.spec.ts`
- Verify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: 対象 E2E を再実行する**

Run:

```bash
npm run test:e2e -- text_selection_defer.spec.ts
```

Expected: PASS。

- [ ] **Step 2: TypeScript 型検査を実行する**

Run:

```bash
npm run typecheck
```

Expected: PASS。

- [ ] **Step 3: リポジトリ標準検証を実行する**

Run:

```bash
./verify.sh
```

Expected: PASS。

- [ ] **Step 4: 変更範囲を確認する**

Run:

```bash
git status --short
git log --oneline -5
```

Expected: working tree は clean。最新コミット群に E2E 追加と BACKLOG 更新が含まれる。

## Self-Review

- Spec coverage: 連続クリック、slack 22px / 26px、小揺らし中の `MutationObserver` 監視、production API 非追加、条件付き `sidebar.js` 最小修正を各 Task に対応させた。
- Placeholder scan: 未決定語や空の手順は入れていない。将来の実装コミット hash は Task 4 のコマンドで確認して BACKLOG に記入する手順にした。
- Type consistency: `currentScrollY`、`waitForTocTrackingFrame`、`startTocActiveChangeRecorder`、`stopTocActiveChangeRecorder` は Task 2 のテストから同じ名前で参照している。`Window.__tocActiveChanges` と `Window.__stopTocObserver` は既存 `tests/e2e/globals.d.ts` に一致している。
