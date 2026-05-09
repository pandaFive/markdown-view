# Content Controller Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `content.js` into a content controller and focused browser modules while preserving current behavior, security boundaries, inline script delivery, and E2E-only hook policy.

**Architecture:** Keep the current IIFE concatenation model in `inline_script.rs`, but create `content-controller.js` as the only content lifecycle coordinator. Move document search, directory search, internal navigation, and post-render enhancements into `createX(ctx, deps)` modules that communicate through explicit controller dependencies and `appContext`.

**Tech Stack:** Plain browser JavaScript, Rust `include_str!` asset concatenation, Playwright E2E tests, Rust template/CSP tests, `./verify.sh`.

---

## File Structure

- Create `src/template/assets/js/content-enhancements.js`: live status, document stats, reading progress, title sync, heading/code copy buttons, TOC filter setup.
- Create `src/template/assets/js/content-navigation.js`: internal Markdown link resolution, file-query hash resolution, line hash parsing, anchor navigation, history hash helpers, memo citation line hint compatibility.
- Create `src/template/assets/js/document-search.js`: current-document search state rendering, highlight creation, search result list rendering, keyboard/input setup, `openDocumentSearch`.
- Create `src/template/assets/js/directory-search.js`: `/api/search` orchestration, directory result state, generation/query stale result rejection, directory result rendering and file-open requests.
- Create `src/template/assets/js/content-controller.js`: `createContentController(ctx, deps)` with `setup`, `updateContent`, `applyPendingUpdate`, `restoreNavigationFromLocation`, `openDocumentSearch`, `moveDocumentSearch`, and `applyDocumentSearchQuery`.
- Modify `src/template/assets/js/content.js`: remove migrated implementation during Tasks 2-5, then delete the file in Task 6 after removing it from `inline_script.rs`.
- Modify `src/template/assets/inline_script.rs`: include new JS files in dependency order before `memo.js`, `fetch.js`, `websocket.js`, and `sidebar.js`.
- Modify `src/template/assets/js/fetch.js`: call `appContext.content.updateContent`, `appContext.content.clearDocumentSearchQuery`, `appContext.content.syncDocumentChrome`, and `appContext.content.renderDirectorySearchUi` instead of content top-level functions.
- Modify `src/template/assets/js/selection.js`: call `appContext.content.applyPendingUpdate()`.
- Modify `src/template/assets/js/sidebar.js`: initialize the content controller in `startMarkdownViewApp`, wire E2E hooks through `appContext.content`, and call content APIs for reading progress and restore navigation.
- Modify `src/template/assets/js/websocket.js`: keep the injected `deps.updateContent(data)` path, but route live status, directory-search reschedule, and update payload contract violations through content-controller-facing dependencies.
- Modify `tests/e2e/update_content_exposure.spec.ts`: assert new internal names are discovered and production `window` remains clean.
- Modify or add tests in `tests/e2e/document_search.spec.ts` and `tests/e2e/markdown_links.spec.ts` for security/stale-result regressions.

## Task 1: Lock Exposure And Search-Safety Tests

**Files:**
- Modify: `tests/e2e/update_content_exposure.spec.ts`
- Modify: `tests/e2e/document_search.spec.ts`
- Test: `tests/e2e/update_content_exposure.spec.ts`
- Test: `tests/e2e/document_search.spec.ts`

- [ ] **Step 1: Update internal exposure expectations for the future controller names**

In `tests/e2e/update_content_exposure.spec.ts`, extend the first test so it requires the controller/module factory declarations after the implementation lands:

```ts
test('内部グローバル名抽出は実ファイルから十分な宣言数を拾う', async () => {
  expect(internalGlobalNames.length).toBeGreaterThan(20);
  expect(internalGlobalNames).toContain('startMarkdownViewApp');
  expect(internalGlobalNames).toContain('createContentController');
  expect(internalGlobalNames).toContain('createContentEnhancements');
  expect(internalGlobalNames).toContain('createContentNavigation');
  expect(internalGlobalNames).toContain('createDocumentSearchController');
  expect(internalGlobalNames).toContain('createDirectorySearchController');
  expect(internalGlobalNames).toContain('createWebSocketController');
  expect(internalGlobalNames).toContain('validateUpdatePayload');
  expect(internalGlobalNames).toContain('applyValidatedUpdateHtml');
});
```

Keep the production exposure test unchanged except for adding explicit checks for the future controller names:

```ts
await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).createContentController)).resolves.toBe('undefined');
await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).createDocumentSearchController)).resolves.toBe('undefined');
await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).createDirectorySearchController)).resolves.toBe('undefined');
await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).appContext)).resolves.toBe('undefined');
```

- [ ] **Step 2: Add a document-search HTML injection regression**

Append this test to `tests/e2e/document_search.spec.ts`:

```ts
test('検索queryは検索結果リストでHTMLとして解釈されない', async ({ page }) => {
  await stabilizeWebSocketHarness(page);
  await page.goto('/');
  await updateContentAndActivateToc(page, {
    content: '<p>literal &lt;img src=x onerror=alert(1)&gt; appears here</p>',
    toc: '<ul></ul>'
  });

  const query = '<img src=x onerror=alert(1)>';
  await page.evaluate((value) => {
    window.markdownViewTestHooks.applyDocumentSearchQuery(value);
  }, query);

  await expect(page.locator('#document-search-results')).toContainText(query);
  await expect(page.locator('#document-search-results img')).toHaveCount(0);
  await expect(page.locator('mark.document-search-match')).toHaveCount(1);
});
```

- [ ] **Step 3: Run the new tests and confirm the expected mixed result**

Run:

```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts tests/e2e/document_search.spec.ts
```

Expected before implementation: `update_content_exposure.spec.ts` fails because the new factory names do not exist yet. The new document-search injection regression should pass on current code. If the injection regression fails, stop this refactor and make a separate fix that changes search result rendering to use `textContent` / `createTextNode` before resuming this plan.

- [ ] **Step 4: Commit tests**

```bash
git add tests/e2e/update_content_exposure.spec.ts tests/e2e/document_search.spec.ts
git commit -m "test: content controller境界の公開面を固定"
```

## Task 2: Add Content Enhancements Module

**Files:**
- Create: `src/template/assets/js/content-enhancements.js`
- Modify: `src/template/assets/js/content.js`
- Modify: `src/template/assets/inline_script.rs`
- Test: `tests/e2e/update_content_exposure.spec.ts`
- Test: `tests/e2e/memo_jump.spec.ts`

- [ ] **Step 1: Create `createContentEnhancements`**

Move these functions from `content.js` into `src/template/assets/js/content-enhancements.js` and wrap them in a factory:

```js
function createContentEnhancements(ctx, deps) {
  function setLiveStatus(state) {
    if (!ctx.elements.liveStatusEl) return;
    ctx.elements.liveStatusEl.textContent = ctx.labels.liveStatus[state] || state;
    ctx.elements.liveStatusEl.dataset.state = state;
    if (state === 'live') {
      deps.clearMemoSyncPendingStatus();
    }
  }

  function updateDocumentStats() {
    if (!ctx.elements.contentRoot) return;
    var headings = ctx.elements.contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6').length;
    var text = (ctx.elements.contentRoot.textContent || '').replace(/\s+/g, '');
    if (ctx.elements.docHeadingCountEl) {
      ctx.elements.docHeadingCountEl.textContent = '見出し ' + headings;
    }
    if (ctx.elements.docCharCountEl) {
      ctx.elements.docCharCountEl.textContent = '文字 ' + text.length;
    }
  }

  function updateReadingProgress() {
    var scrollTop = window.scrollY || window.pageYOffset;
    var maxScroll = Math.max(document.documentElement.scrollHeight - window.innerHeight, 1);
    var progress = Math.min(100, Math.max(0, (scrollTop / maxScroll) * 100));
    if (ctx.elements.readingProgressBar) {
      ctx.elements.readingProgressBar.style.width = progress + '%';
    }
    if (ctx.elements.backToTop) {
      ctx.elements.backToTop.classList.toggle('visible', scrollTop > 360);
    }
  }

  function syncDocumentChrome(file) {
    var title = file ? file.split('/').pop() : (ctx.elements.contentRoot ? ctx.elements.contentRoot.getAttribute('data-title') : '');
    if (!title) title = 'markdown-view';
    if (ctx.elements.documentTitleEl) {
      ctx.elements.documentTitleEl.textContent = title;
    }
    document.title = title + ' - markdown-view';
  }

  function copyText(text) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(text);
    }
    return new Promise(function(resolve, reject) {
      try {
        var input = document.createElement('textarea');
        input.value = text;
        input.setAttribute('readonly', 'readonly');
        input.style.position = 'fixed';
        input.style.opacity = '0';
        document.body.appendChild(input);
        input.select();
        var success = document.execCommand('copy');
        input.remove();
        if (success) {
          resolve();
        } else {
          reject(new Error('execCommand("copy") returned false'));
        }
      } catch (error) {
        reject(error);
      }
    });
  }

  function flashCopiedState(button, copiedLabel, baseLabel) {
    if (!button) return;
    button.classList.add('copied');
    button.textContent = copiedLabel;
    setTimeout(function() {
      button.classList.remove('copied');
      button.textContent = baseLabel;
    }, 1200);
  }

  function handleCopyClick(button, text, baseLabel) {
    copyText(text).then(function() {
      flashCopiedState(button, 'Copied', baseLabel);
    }).catch(function(err) {
      console.warn('[markdown-view] コピーに失敗:', err);
      flashCopiedState(button, 'Failed', baseLabel);
    });
  }

  function enhanceContentInteractions() {
    if (!ctx.elements.contentRoot) return;

    var headings = ctx.elements.contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6');
    headings.forEach(function(heading) {
      if (!heading.id || heading.querySelector('.heading-anchor')) return;
      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'heading-anchor';
      button.textContent = '#';
      button.setAttribute('aria-label', '見出しリンクをコピー');
      button.addEventListener('click', function() {
        var url = new URL(location.href);
        url.hash = heading.id;
        handleCopyClick(button, url.toString(), '#');
      });
      heading.appendChild(button);
    });

    var blocks = ctx.elements.contentRoot.querySelectorAll('pre.code-block');
    blocks.forEach(function(block) {
      if (block.querySelector('.code-copy')) return;
      var code = block.querySelector('code');
      if (!code) return;
      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'code-copy';
      button.textContent = 'Copy';
      button.setAttribute('aria-label', 'コードをコピー');
      button.addEventListener('click', function() {
        handleCopyClick(button, code.innerText || code.textContent || '', 'Copy');
      });
      block.appendChild(button);
    });
  }

  function setupFilterableList(options) {
    var input = document.getElementById(options.inputId);
    var root = document.getElementById(options.rootId);
    if (!input || !root) return;

    var items = options.getItems(root);
    var applyFilter = function() {
      var query = input.value.trim().toLowerCase();
      options.apply(items, query, input);
    };

    input.addEventListener('input', applyFilter);
    applyFilter();
  }

  function setupTocFilter() {
    setupFilterableList({
      inputId: 'toc-filter',
      rootId: 'toc',
      getItems: function(root) {
        return root.querySelectorAll('li');
      },
      apply: function(items, query) {
        items.forEach(function(item) {
          var link = item.querySelector(':scope > a');
          if (!link) return;
          var matched = !query || link.textContent.toLowerCase().indexOf(query) !== -1;
          item.hidden = !matched;
        });
      }
    });
  }

  return {
    setLiveStatus: setLiveStatus,
    updateDocumentStats: updateDocumentStats,
    updateReadingProgress: updateReadingProgress,
    syncDocumentChrome: syncDocumentChrome,
    enhanceContentInteractions: enhanceContentInteractions,
    setupFilterableList: setupFilterableList,
    setupTocFilter: setupTocFilter
  };
}
```

- [ ] **Step 2: Include the new file before `content.js`**

Edit `src/template/assets/inline_script.rs` so the start of `TEMPLATE` includes:

```rust
    include_str!("js/content-renderer.js"),
    "\n",
    include_str!("js/content-enhancements.js"),
    "\n",
    include_str!("js/content.js"),
```

- [ ] **Step 3: Temporarily bridge existing calls**

In `content.js`, remove the moved function definitions and add this bridge near the top:

```js
var contentEnhancements = createContentEnhancements(appContext, {
  clearMemoSyncPendingStatus: clearMemoSyncPendingStatus
});
var setLiveStatus = contentEnhancements.setLiveStatus;
var updateDocumentStats = contentEnhancements.updateDocumentStats;
var updateReadingProgress = contentEnhancements.updateReadingProgress;
var syncDocumentChrome = contentEnhancements.syncDocumentChrome;
var enhanceContentInteractions = contentEnhancements.enhanceContentInteractions;
var setupFilterableList = contentEnhancements.setupFilterableList;
var setupTocFilter = contentEnhancements.setupTocFilter;
```

- [ ] **Step 4: Run focused verification**

Run:

```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts tests/e2e/memo_jump.spec.ts
cargo test --all-targets --all-features
```

Expected: E2E production exposure still fails only on not-yet-created factory names from Task 1. No runtime failure should occur from the enhancement extraction. Cargo tests pass, proving inline asset/CSP generation still compiles.

- [ ] **Step 5: Commit**

```bash
git add src/template/assets/js/content-enhancements.js src/template/assets/js/content.js src/template/assets/inline_script.rs
git commit -m "refactor: content enhancementを分離"
```

## Task 3: Add Content Navigation Module

**Files:**
- Create: `src/template/assets/js/content-navigation.js`
- Modify: `src/template/assets/js/content.js`
- Modify: `src/template/assets/inline_script.rs`
- Test: `tests/e2e/markdown_links.spec.ts`
- Test: `tests/e2e/memo_jump.spec.ts`

- [ ] **Step 1: Create `createContentNavigation`**

Move these functions from `content.js` into `src/template/assets/js/content-navigation.js`: `updateLocationHash`, `setLocationHash`, `isModifiedClick`, `isExternalSchemeHref`, `resolveFileQueryHref`, `resolveMarkdownLinkTarget`, `parseLineHash`, `augmentHashWithTrailingLineHint`, `scrollToLineRange`, `triggerJumpHighlight`, `applyContentAnchorNavigation`, `restoreContentNavigationFromLocation`, `handleInternalLinkClick`, `setupContentLinkNavigation`, and `setupMemoLinkNavigation`.

Wrap them with this outer shape:

```js
function createContentNavigation(ctx, deps) {
  // Start from the exact existing function bodies listed above.
  // In those bodies, replace every appContext reference with ctx.
  // In applyContentAnchorNavigation, call deps.markPendingTocNavigation(parsed.headingId).
  // In restoreContentNavigationFromLocation, call deps.clearPendingTocNavigation()
  // and deps.restoreActiveTocHeading('').
  // In handleInternalLinkClick, call deps.setFileParam(...) and deps.selectFile(...).

  return {
    augmentHashWithTrailingLineHint: augmentHashWithTrailingLineHint,
    applyContentAnchorNavigation: applyContentAnchorNavigation,
    restoreContentNavigationFromLocation: restoreContentNavigationFromLocation,
    setupContentLinkNavigation: setupContentLinkNavigation,
    setupMemoLinkNavigation: setupMemoLinkNavigation,
    setLocationHash: setLocationHash
  };
}
```

The moved `handleInternalLinkClick` body must use:

```js
deps.selectFile(target.file, true, {
  scrollMode: target.hash ? 'none' : 'reset',
  anchorHash: target.hash,
  historyHash: target.hash || ''
});
```

- [ ] **Step 2: Include the navigation file before `content.js`**

Edit `src/template/assets/inline_script.rs`:

```rust
    include_str!("js/content-enhancements.js"),
    "\n",
    include_str!("js/content-navigation.js"),
    "\n",
    include_str!("js/content.js"),
```

- [ ] **Step 3: Temporarily bridge existing calls**

In `content.js`, instantiate navigation after the enhancement bridge:

```js
var contentNavigation = createContentNavigation(appContext, {
  selectFile: selectFile,
  setFileParam: setFileParam,
  markPendingTocNavigation: markPendingTocNavigation,
  clearPendingTocNavigation: clearPendingTocNavigation,
  restoreActiveTocHeading: restoreActiveTocHeading
});
var augmentHashWithTrailingLineHint = contentNavigation.augmentHashWithTrailingLineHint;
var applyContentAnchorNavigation = contentNavigation.applyContentAnchorNavigation;
var restoreContentNavigationFromLocation = contentNavigation.restoreContentNavigationFromLocation;
var setupContentLinkNavigation = contentNavigation.setupContentLinkNavigation;
var setupMemoLinkNavigation = contentNavigation.setupMemoLinkNavigation;
var setLocationHash = contentNavigation.setLocationHash;
```

Remove the original moved function definitions from `content.js`.

- [ ] **Step 4: Run navigation regressions**

Run:

```bash
npm run test:e2e -- tests/e2e/markdown_links.spec.ts tests/e2e/memo_jump.spec.ts
cargo test --all-targets --all-features
```

Expected: navigation and memo citation tests pass. Cargo tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/template/assets/js/content-navigation.js src/template/assets/js/content.js src/template/assets/inline_script.rs
git commit -m "refactor: content navigationを分離"
```

## Task 4: Add Document Search Module

**Files:**
- Create: `src/template/assets/js/document-search.js`
- Modify: `src/template/assets/js/content.js`
- Modify: `src/template/assets/inline_script.rs`
- Test: `tests/e2e/document_search.spec.ts`

- [ ] **Step 1: Create `createDocumentSearchController`**

Move current-document search functions from `content.js` into `src/template/assets/js/document-search.js`: `createDocumentSearchEmptyState`, `updateDocumentSearchSummary`, `clearDocumentSearchHighlights`, `DOCUMENT_SEARCH_BLOCK_SELECTOR`, `shouldSkipDocumentSearchNode`, `createDocumentSearchMark`, `escapeRegExp`, `trimSentenceRange`, `splitTextIntoSentenceRanges`, `getSentenceForMatch`, `getAdjacentSentence`, `buildDocumentSearchContext`, `getDocumentSearchBlocks`, `collectDocumentSearchTextNodes`, `wrapDocumentSearchSegment`, `wrapDocumentSearchMatch`, `renderDocumentSearchResultContext`, `renderDocumentSearchResults`, `applyDocumentSearchHighlights`, `setCurrentDocumentSearchMatch`, `moveDocumentSearch`, `applyDocumentSearchQuery`, `clearDocumentSearchQuery`, `syncDocumentSearchAfterContentUpdate`, `openDocumentSearch`, and `setupDocumentSearch`.

Use this outer shape:

```js
function createDocumentSearchController(ctx, deps) {
  // Start from the exact existing function bodies listed above.
  // In those bodies, replace every appContext reference with ctx.
  // Keep DOM creation through document.createElement, document.createTextNode,
  // textContent, and appendChild.
  // In renderDocumentSearchResults, call deps.renderDirectorySearchResults().
  // In directory-mode UI paths, call deps.renderDirectorySearchUi().
  // In query scheduling paths, call deps.scheduleDirectorySearch(query).
  // In content-update sync, call deps.applyPendingDirectorySearchNavigation().
  // In moveDocumentSearch, call deps.openDirectorySearchResult(index).
  // In openDocumentSearch, call deps.activateSidebarTab('toc').

  return {
    applyDocumentSearchHighlights: applyDocumentSearchHighlights,
    applyDocumentSearchQuery: applyDocumentSearchQuery,
    clearDocumentSearchHighlights: clearDocumentSearchHighlights,
    clearDocumentSearchQuery: clearDocumentSearchQuery,
    createDocumentSearchEmptyState: createDocumentSearchEmptyState,
    moveDocumentSearch: moveDocumentSearch,
    openDocumentSearch: openDocumentSearch,
    renderDocumentSearchResultContext: renderDocumentSearchResultContext,
    renderDocumentSearchResults: renderDocumentSearchResults,
    setCurrentDocumentSearchMatch: setCurrentDocumentSearchMatch,
    setupDocumentSearch: setupDocumentSearch,
    syncDocumentSearchAfterContentUpdate: syncDocumentSearchAfterContentUpdate,
    updateDocumentSearchSummary: updateDocumentSearchSummary
  };
}
```

- [ ] **Step 2: Include document search before `content.js`**

Edit `src/template/assets/inline_script.rs`:

```rust
    include_str!("js/content-navigation.js"),
    "\n",
    include_str!("js/document-search.js"),
    "\n",
    include_str!("js/content.js"),
```

- [ ] **Step 3: Temporarily bridge document search**

In `content.js`, instantiate after navigation and before directory search code:

```js
var documentSearchController = createDocumentSearchController(appContext, {
  activateSidebarTab: activateSidebarTab,
  applyPendingDirectorySearchNavigation: function() {
    return applyPendingDirectorySearchNavigation();
  },
  openDirectorySearchResult: function(index) {
    return openDirectorySearchResult(index);
  },
  renderDirectorySearchResults: function() {
    return renderDirectorySearchResults();
  },
  renderDirectorySearchUi: function() {
    return renderDirectorySearchUi();
  },
  scheduleDirectorySearch: function(query) {
    return scheduleDirectorySearch(query);
  }
});
var applyDocumentSearchHighlights = documentSearchController.applyDocumentSearchHighlights;
var applyDocumentSearchQuery = documentSearchController.applyDocumentSearchQuery;
var clearDocumentSearchHighlights = documentSearchController.clearDocumentSearchHighlights;
var clearDocumentSearchQuery = documentSearchController.clearDocumentSearchQuery;
var createDocumentSearchEmptyState = documentSearchController.createDocumentSearchEmptyState;
var moveDocumentSearch = documentSearchController.moveDocumentSearch;
var openDocumentSearch = documentSearchController.openDocumentSearch;
var renderDocumentSearchResultContext = documentSearchController.renderDocumentSearchResultContext;
var renderDocumentSearchResults = documentSearchController.renderDocumentSearchResults;
var setCurrentDocumentSearchMatch = documentSearchController.setCurrentDocumentSearchMatch;
var setupDocumentSearch = documentSearchController.setupDocumentSearch;
var syncDocumentSearchAfterContentUpdate = documentSearchController.syncDocumentSearchAfterContentUpdate;
var updateDocumentSearchSummary = documentSearchController.updateDocumentSearchSummary;
```

Remove the original moved function definitions from `content.js`.

- [ ] **Step 4: Run document-search regressions**

Run:

```bash
npm run test:e2e -- tests/e2e/document_search.spec.ts tests/e2e/update_content_exposure.spec.ts
cargo test --all-targets --all-features
```

Expected: document search tests pass, including the HTML injection regression. Exposure test still fails only if later factory names are not created yet.

- [ ] **Step 5: Commit**

```bash
git add src/template/assets/js/document-search.js src/template/assets/js/content.js src/template/assets/inline_script.rs tests/e2e/document_search.spec.ts
git commit -m "refactor: document searchを分離"
```

## Task 5: Add Directory Search Module

**Files:**
- Create: `src/template/assets/js/directory-search.js`
- Modify: `src/template/assets/js/content.js`
- Modify: `src/template/assets/inline_script.rs`
- Test: `tests/e2e/document_search.spec.ts`

- [ ] **Step 1: Create a stale-result E2E regression**

Append this test to `tests/e2e/document_search.spec.ts`:

```ts
test('ディレクトリ検索の古い応答は現在queryへ適用されない', async ({ page }) => {
  let firstRequestStarted = false;
  let releaseFirstResponse: (() => void) | null = null;

  await page.route('**/api/search**', async (route) => {
    const url = new URL(route.request().url());
    const query = url.searchParams.get('q');

    if (query === 'alpha') {
      firstRequestStarted = true;
      await new Promise<void>((resolve) => {
        releaseFirstResponse = resolve;
      });
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          query: 'alpha',
          results: [{ file: 'notes.md', line: 1, before: '', current: 'alpha old', after: '', file_match_index: 0 }],
          skipped_files: 0,
          truncated: false,
          truncated_reasons: []
        })
      });
      return;
    }

    if (query === 'beta') {
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          query: 'beta',
          results: [{ file: 'README.md', line: 1, before: '', current: 'beta current', after: '', file_match_index: 0 }],
          skipped_files: 0,
          truncated: false,
          truncated_reasons: []
        })
      });
      return;
    }

    await route.fallback();
  });

  await page.evaluate(() => {
    window.markdownViewTestHooks.setDirModeForTest(true);
    window.markdownViewTestHooks.setCurrentFileForTest('README.md');
  });
  await updateContentAndActivateToc(page, {
    content: '<h1 id="readme">README</h1><p>alpha text</p><p>beta text</p>',
    toc: '<ul><li><a href="#readme">README</a></li></ul>'
  });

  await setDocumentSearchQuery(page, 'alpha');
  await expect.poll(() => firstRequestStarted).toBe(true);

  await setDocumentSearchQuery(page, 'beta');
  await expect(page.locator('#document-search-results')).toContainText('beta current');

  releaseFirstResponse?.();
  await page.waitForTimeout(100);
  await expect(page.locator('#document-search-results')).toContainText('beta current');
  await expect(page.locator('#document-search-results')).not.toContainText('alpha old');
});
```

This test intentionally waits until the first request has started. Without that wait, the 300ms debounce could cancel `alpha` before any stale response exists.

- [ ] **Step 2: Create `createDirectorySearchController`**

Move directory search functions from `content.js` into `src/template/assets/js/directory-search.js`: `createDirectorySearchTruncatedState`, `formatDirectorySearchSummary`, `renderDirectorySearchResults`, `renderDirectorySearchUi`, `applyPendingDirectorySearchNavigation`, `scheduleDirectorySearch`, `getPreferredDirectorySearchSelection`, `resolveDirectorySearchIndex`, `runDirectorySearch`, and `openDirectorySearchResult`.

Use this outer shape:

```js
function createDirectorySearchController(ctx, deps) {
  // Start from the exact existing function bodies listed above.
  // In those bodies, replace every appContext reference with ctx.
  // Replace createDocumentSearchEmptyState(...) with deps.createDocumentSearchEmptyState(...).
  // Replace renderDocumentSearchResultContext(...) with deps.renderDocumentSearchResultContext(...).
  // Replace updateDocumentSearchSummary() with deps.updateDocumentSearchSummary().
  // Replace setCurrentDocumentSearchMatch(...) with deps.setCurrentDocumentSearchMatch(...).
  // Replace selectFile(...) with deps.openFileSearchResult(...).
  // Keep fetch('/api/search?q=' + encodeURIComponent(query), ...) unchanged.

  return {
    applyPendingDirectorySearchNavigation: applyPendingDirectorySearchNavigation,
    openDirectorySearchResult: openDirectorySearchResult,
    renderDirectorySearchResults: renderDirectorySearchResults,
    renderDirectorySearchUi: renderDirectorySearchUi,
    scheduleDirectorySearch: scheduleDirectorySearch
  };
}
```

The module must keep this stale-result guard in both success and failure paths:

```js
if (generation !== ctx.search.documentFetchGeneration) return;
if (query !== ctx.search.currentDocumentQuery) return;
```

For success responses, keep the server echo guard:

```js
if ((data.query || '') !== ctx.search.currentDocumentQuery) return;
```

- [ ] **Step 3: Include directory search before `content.js`**

Edit `src/template/assets/inline_script.rs`:

```rust
    include_str!("js/document-search.js"),
    "\n",
    include_str!("js/directory-search.js"),
    "\n",
    include_str!("js/content.js"),
```

- [ ] **Step 4: Temporarily bridge directory search**

In `content.js`, replace the Task 4 document-search bridge with this two-controller bridge:

```js
var directorySearchController;
var documentSearchController = createDocumentSearchController(appContext, {
  activateSidebarTab: activateSidebarTab,
  applyPendingDirectorySearchNavigation: function() {
    return directorySearchController.applyPendingDirectorySearchNavigation();
  },
  openDirectorySearchResult: function(index) {
    return directorySearchController.openDirectorySearchResult(index);
  },
  renderDirectorySearchResults: function() {
    return directorySearchController.renderDirectorySearchResults();
  },
  renderDirectorySearchUi: function() {
    return directorySearchController.renderDirectorySearchUi();
  },
  scheduleDirectorySearch: function(query) {
    return directorySearchController.scheduleDirectorySearch(query);
  }
});

directorySearchController = createDirectorySearchController(appContext, {
  createDocumentSearchEmptyState: function(message) {
    return documentSearchController.createDocumentSearchEmptyState(message);
  },
  getFileFetchErrorMessage: getFileFetchErrorMessage,
  openFileSearchResult: function(file, options) {
    return selectFile(file, false, options);
  },
  renderDocumentSearchResultContext: function(container, text, query, variant) {
    return documentSearchController.renderDocumentSearchResultContext(container, text, query, variant);
  },
  setCurrentDocumentSearchMatch: function(index, scrollIntoView) {
    return documentSearchController.setCurrentDocumentSearchMatch(index, scrollIntoView);
  },
  updateDocumentSearchSummary: function() {
    return documentSearchController.updateDocumentSearchSummary();
  }
});

var applyDocumentSearchHighlights = documentSearchController.applyDocumentSearchHighlights;
var applyDocumentSearchQuery = documentSearchController.applyDocumentSearchQuery;
var clearDocumentSearchHighlights = documentSearchController.clearDocumentSearchHighlights;
var clearDocumentSearchQuery = documentSearchController.clearDocumentSearchQuery;
var createDocumentSearchEmptyState = documentSearchController.createDocumentSearchEmptyState;
var moveDocumentSearch = documentSearchController.moveDocumentSearch;
var openDocumentSearch = documentSearchController.openDocumentSearch;
var renderDocumentSearchResultContext = documentSearchController.renderDocumentSearchResultContext;
var renderDocumentSearchResults = documentSearchController.renderDocumentSearchResults;
var setCurrentDocumentSearchMatch = documentSearchController.setCurrentDocumentSearchMatch;
var setupDocumentSearch = documentSearchController.setupDocumentSearch;
var syncDocumentSearchAfterContentUpdate = documentSearchController.syncDocumentSearchAfterContentUpdate;
var updateDocumentSearchSummary = documentSearchController.updateDocumentSearchSummary;

var applyPendingDirectorySearchNavigation = directorySearchController.applyPendingDirectorySearchNavigation;
var openDirectorySearchResult = directorySearchController.openDirectorySearchResult;
var renderDirectorySearchResults = directorySearchController.renderDirectorySearchResults;
var renderDirectorySearchUi = directorySearchController.renderDirectorySearchUi;
var scheduleDirectorySearch = directorySearchController.scheduleDirectorySearch;
```

This ordering is intentional: document-search receives wrappers that read `directorySearchController` later. Those wrappers are only called after both controllers have been assigned.

- [ ] **Step 5: Run directory-search regressions**

Run:

```bash
npm run test:e2e -- tests/e2e/document_search.spec.ts
cargo test --all-targets --all-features
```

Expected: document search tests pass, including stale-result regression. Cargo tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/template/assets/js/directory-search.js src/template/assets/js/content.js src/template/assets/inline_script.rs tests/e2e/document_search.spec.ts
git commit -m "refactor: directory searchを分離"
```

## Task 6: Create Content Controller And Remove Top-Level Content Bridges

**Files:**
- Create: `src/template/assets/js/content-controller.js`
- Modify: `src/template/assets/js/content.js`
- Modify: `src/template/assets/js/fetch.js`
- Modify: `src/template/assets/js/selection.js`
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/sidebar.js`
- Modify: `src/template/assets/inline_script.rs`
- Test: `tests/e2e/update_content_exposure.spec.ts`
- Test: `tests/e2e/document_search.spec.ts`
- Test: `tests/e2e/markdown_links.spec.ts`
- Test: `tests/e2e/memo_jump.spec.ts`

- [ ] **Step 1: Create `createContentController`**

Create `src/template/assets/js/content-controller.js`:

```js
function createContentController(ctx, deps) {
  var enhancements = createContentEnhancements(ctx, {
    clearMemoSyncPendingStatus: deps.clearMemoSyncPendingStatus
  });
  var navigation = createContentNavigation(ctx, {
    selectFile: deps.selectFile,
    setFileParam: deps.setFileParam,
    markPendingTocNavigation: deps.markPendingTocNavigation,
    clearPendingTocNavigation: deps.clearPendingTocNavigation,
    restoreActiveTocHeading: deps.restoreActiveTocHeading
  });
  var directorySearch;
  var documentSearch = createDocumentSearchController(ctx, {
    activateSidebarTab: deps.activateSidebarTab,
    applyPendingDirectorySearchNavigation: function() {
      return directorySearch.applyPendingDirectorySearchNavigation();
    },
    openDirectorySearchResult: function(index) {
      return directorySearch.openDirectorySearchResult(index);
    },
    renderDirectorySearchResults: function() {
      return directorySearch.renderDirectorySearchResults();
    },
    renderDirectorySearchUi: function() {
      return directorySearch.renderDirectorySearchUi();
    },
    scheduleDirectorySearch: function(query) {
      return directorySearch.scheduleDirectorySearch(query);
    }
  });

  directorySearch = createDirectorySearchController(ctx, {
    createDocumentSearchEmptyState: function(message) {
      return documentSearch.createDocumentSearchEmptyState(message);
    },
    getFileFetchErrorMessage: deps.getFileFetchErrorMessage,
    openFileSearchResult: function(file, options) {
      return deps.selectFile(file, false, options);
    },
    renderDocumentSearchResultContext: function(container, text, query, variant) {
      return documentSearch.renderDocumentSearchResultContext(container, text, query, variant);
    },
    setCurrentDocumentSearchMatch: function(index, scrollIntoView) {
      return documentSearch.setCurrentDocumentSearchMatch(index, scrollIntoView);
    },
    updateDocumentSearchSummary: function() {
      return documentSearch.updateDocumentSearchSummary();
    }
  });

  function applyPendingUpdate() {
    if (!ctx.state.pendingUpdate) return;
    if (ctx.state.pendingUpdateTimer) {
      clearTimeout(ctx.state.pendingUpdateTimer);
      ctx.state.pendingUpdateTimer = null;
    }
    if (ctx.state.pendingUpdate.refresh) {
      var refreshFile = ctx.state.pendingUpdate.file;
      ctx.state.pendingUpdate = null;
      if (ctx.config.isDirMode && refreshFile) {
        deps.selectFile(refreshFile, false);
      } else {
        console.warn('[markdown-view] refresh 保留更新を適用できませんでした。', {
          isDirMode: ctx.config.isDirMode,
          refreshFile: refreshFile || '',
          currentFile: ctx.state.currentFile || ''
        });
      }
      return;
    }
    if (ctx.config.isDirMode && ctx.state.pendingUpdate.file && ctx.state.pendingUpdate.file !== ctx.state.currentFile) {
      console.warn('[markdown-view] 現在のファイルと異なる保留更新を破棄しました。', {
        currentFile: ctx.state.currentFile,
        messageFile: ctx.state.pendingUpdate.file
      });
      ctx.state.pendingUpdate = null;
      return;
    }
    var data = ctx.state.pendingUpdate;
    ctx.state.pendingUpdate = null;
    updateContent(data);
    deps.hideWsServerErrorBanner();
    deps.hideFileFetchErrorBanner();
    enhancements.setLiveStatus('live');
  }

  function updateContent(data, options) {
    options = options || {};
    var validation = validateUpdatePayload(data);
    var safeData = validation.safeData;

    if (validation.hasContractViolation) {
      logUpdatePayloadContractViolation(validation);
    }

    if (ctx.state.pendingUpdateTimer) {
      clearTimeout(ctx.state.pendingUpdateTimer);
      ctx.state.pendingUpdateTimer = null;
    }
    ctx.state.pendingUpdate = null;
    var scrollY = window.scrollY;
    var scrollMode = options.scrollMode || 'preserve';
    var preservedActiveTocId = deps.getCurrentActiveTocId();
    var contentEl = document.getElementById('content');
    var tocEl = document.getElementById('toc');

    applyValidatedUpdateHtml(ctx, {
      contentEl: contentEl,
      tocEl: tocEl
    }, validation);

    deps.setupTocTracking();
    deps.suppressTocTrackingFor(120);

    requestAnimationFrame(function() {
      var currentScrollY = window.scrollY || window.pageYOffset;
      var anchorApplied = false;
      if (scrollMode === 'preserve' && Math.abs(currentScrollY - scrollY) <= 1) {
        window.scrollTo(0, scrollY);
      } else if (scrollMode === 'reset') {
        window.scrollTo(0, 0);
      }
      if (options.anchorHash) {
        anchorApplied = navigation.applyContentAnchorNavigation(options.anchorHash, true);
        if (!anchorApplied) {
          console.warn('[markdown-view] リンク先の見出しが見つかりません:', options.anchorHash);
          window.scrollTo(0, 0);
          if (options.clearHashOnMiss !== false) {
            navigation.setLocationHash('', true);
          }
          deps.clearPendingTocNavigation();
        }
      }
      enhancements.updateReadingProgress();
      deps.restoreActiveTocHeading(preservedActiveTocId);
    });

    enhancements.updateDocumentStats();
    enhancements.syncDocumentChrome(ctx.state.currentFile);
    enhancements.enhanceContentInteractions();
    documentSearch.syncDocumentSearchAfterContentUpdate(options);
    enhancements.setupTocFilter();
    deps.hideQuoteSelectionAction();
    if (!validation.hasContractViolation && ctx.websocket) {
      ctx.websocket.rememberAppliedLiveUpdate(safeData);
    }
  }

  function setup() {
    documentSearch.setupDocumentSearch();
    navigation.setupContentLinkNavigation();
    navigation.setupMemoLinkNavigation();
  }

  return {
    setup: setup,
    updateContent: updateContent,
    applyPendingUpdate: applyPendingUpdate,
    restoreNavigationFromLocation: navigation.restoreContentNavigationFromLocation,
    openDocumentSearch: documentSearch.openDocumentSearch,
    moveDocumentSearch: documentSearch.moveDocumentSearch,
    applyDocumentSearchQuery: documentSearch.applyDocumentSearchQuery,
    clearDocumentSearchQuery: documentSearch.clearDocumentSearchQuery,
    renderDirectorySearchUi: directorySearch.renderDirectorySearchUi,
    augmentHashWithTrailingLineHint: navigation.augmentHashWithTrailingLineHint,
    setLiveStatus: enhancements.setLiveStatus,
    updateDocumentStats: enhancements.updateDocumentStats,
    updateReadingProgress: enhancements.updateReadingProgress,
    syncDocumentChrome: enhancements.syncDocumentChrome,
    enhanceContentInteractions: enhancements.enhanceContentInteractions,
    setupFilterableList: enhancements.setupFilterableList,
    setupTocFilter: enhancements.setupTocFilter
  };
}
```

Before using `documentSearch.createDocumentSearchEmptyState`, confirm this property already exists in the `createDocumentSearchController` return object from Task 4:

```js
createDocumentSearchEmptyState: createDocumentSearchEmptyState,
```

This initialization order is intentional: `documentSearch` receives wrapper functions that read `directorySearch` later. Those wrappers are not called during controller construction, and `directorySearch` is assigned before `setup()` can register events or run searches.

- [ ] **Step 2: Include controller and remove `content.js` from the bundle**

Edit `src/template/assets/inline_script.rs` so content-related files are:

```rust
    include_str!("js/content-renderer.js"),
    "\n",
    include_str!("js/content-enhancements.js"),
    "\n",
    include_str!("js/content-navigation.js"),
    "\n",
    include_str!("js/document-search.js"),
    "\n",
    include_str!("js/directory-search.js"),
    "\n",
    include_str!("js/content-controller.js"),
```

Remove the `include_str!("js/content.js")` entry. Delete `src/template/assets/js/content.js` if no code remains in it.

- [ ] **Step 3: Wire content controller in `startMarkdownViewApp`**

In `src/template/assets/js/bootstrap.js`, add a nullable controller slot near the end of `createAppContext`:

```js
    websocket: null,
    content: null
```

In `src/template/assets/js/sidebar.js`, at the start of `startMarkdownViewApp()` after `setupHistoryUrlSync()` add:

```js
appContext.content = createContentController(appContext, {
  activateSidebarTab: activateSidebarTab,
  clearMemoSyncPendingStatus: clearMemoSyncPendingStatus,
  clearPendingTocNavigation: clearPendingTocNavigation,
  getCurrentActiveTocId: getCurrentActiveTocId,
  getFileFetchErrorMessage: getFileFetchErrorMessage,
  hideFileFetchErrorBanner: hideFileFetchErrorBanner,
  hideQuoteSelectionAction: hideQuoteSelectionAction,
  hideWsServerErrorBanner: hideWsServerErrorBanner,
  markPendingTocNavigation: markPendingTocNavigation,
  restoreActiveTocHeading: restoreActiveTocHeading,
  selectFile: selectFile,
  setFileParam: setFileParam,
  setupTocTracking: setupTocTracking,
  suppressTocTrackingFor: suppressTocTrackingFor
});
```

Then replace:

```js
setupDocumentSearch();
setupContentLinkNavigation();
setupMemoLinkNavigation();
```

with:

```js
appContext.content.setup();
```

Replace the later calls:

```js
updateDocumentStats();
updateReadingProgress();
syncDocumentChrome(appContext.state.currentFile);
enhanceContentInteractions();
setupTocFilter();
```

with:

```js
appContext.content.updateDocumentStats();
appContext.content.updateReadingProgress();
appContext.content.syncDocumentChrome(appContext.state.currentFile);
appContext.content.enhanceContentInteractions();
appContext.content.setupTocFilter();
```

- [ ] **Step 4: Replace cross-file top-level calls**

Apply these replacements:

```js
// src/template/assets/js/fetch.js
clearDocumentSearchQuery()
// becomes
appContext.content.clearDocumentSearchQuery()

updateContent(data, options)
// becomes
appContext.content.updateContent(data, options)

syncDocumentChrome(appContext.state.currentFile)
// becomes
appContext.content.syncDocumentChrome(appContext.state.currentFile)

renderDirectorySearchUi()
// becomes
appContext.content.renderDirectorySearchUi()

setLiveStatus('live')
// becomes
appContext.content.setLiveStatus('live')

setLiveStatus('error')
// becomes
appContext.content.setLiveStatus('error')
```

```js
// src/template/assets/js/selection.js
applyPendingUpdate()
// becomes
appContext.content.applyPendingUpdate()
```

```js
// src/template/assets/js/sidebar.js
restoreContentNavigationFromLocation()
// becomes
appContext.content.restoreNavigationFromLocation()

updateReadingProgress
// event listener callbacks become
appContext.content.updateReadingProgress

setupFilterableList(...)
// in setupFileFilter becomes
appContext.content.setupFilterableList(...)
```

When passing websocket deps in `sidebar.js`, replace:

```js
updateContent: updateContent,
```

with:

```js
updateContent: appContext.content.updateContent,
```

- [ ] **Step 5: Wire E2E hooks through content controller**

In `installMarkdownViewTestHooks()`, replace content hook bodies:

```js
applyDocumentSearchQuery: function(query) {
  return appContext.content.applyDocumentSearchQuery(query);
},
augmentHashWithTrailingLineHint: function(link, hash) {
  return appContext.content.augmentHashWithTrailingLineHint(link, hash);
},
moveDocumentSearch: function(direction) {
  return appContext.content.moveDocumentSearch(direction);
},
updateContent: function(data, options) {
  return appContext.content.updateContent(data, options);
},
```

Keep non-content hooks such as `activateSidebarTab`, `markPendingTocNavigation`, and `selectFile` where they are.

- [ ] **Step 6: Verify no migrated top-level content calls remain**

Run:

```bash
rg "updateContent\\(|applyPendingUpdate\\(|openDocumentSearch\\(|applyDocumentSearchQuery\\(|moveDocumentSearch\\(|restoreContentNavigationFromLocation\\(|setupDocumentSearch\\(|setupContentLinkNavigation\\(|setupMemoLinkNavigation\\(" src/template/assets/js
```

Expected: matches are inside `content-controller.js`, factory return objects, E2E hook wrappers, or `appContext.content.*` call sites. There are no bare cross-file calls to deleted content functions.

- [ ] **Step 7: Run focused E2E and Rust verification**

Run:

```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts tests/e2e/document_search.spec.ts tests/e2e/markdown_links.spec.ts tests/e2e/memo_jump.spec.ts
cargo test --all-targets --all-features
```

Expected: all listed tests pass. `update_content_exposure.spec.ts` now passes because all expected factory names exist and production `window` has no added properties.

- [ ] **Step 8: Commit**

```bash
git add src/template/assets/js src/template/assets/inline_script.rs tests/e2e/update_content_exposure.spec.ts
git commit -m "refactor: content controllerで本文境界を統合"
```

## Task 7: Final Verification And Documentation Touch-Up

**Files:**
- Modify: `docs/todo/TODO.md`
- Test: full repo verification

- [ ] **Step 1: Update TODO completion evidence**

In `docs/todo/TODO.md`, move the Medium item `ブラウザ JS の責務境界を小モジュールへ分割する` from `Medium Priority` to `Done Summary`. Use this completion text:

```markdown
- [x] ブラウザ JS の責務境界を小モジュールへ分割する
  - 完了根拠: `content.js` の責務を `content-controller.js`、`content-enhancements.js`、`content-navigation.js`、`document-search.js`、`directory-search.js` に分割し、本文更新、検索、ディレクトリ検索、内部リンク解決、描画後副作用を明示境界へ分けた。`content-renderer.js` の sanitize 済み HTML 反映境界は維持し、検索 query と検索結果は DOM API で描画する構成にした。production `window` への内部 API 露出は増やさず、E2E hook は `window.__MV_E2E__ === true` の場合だけ公開する。検索、Markdown link、memo citation jump、update exposure の E2E で回帰を固定した
```

- [ ] **Step 2: Run full verification**

Run:

```bash
./verify.sh
npm run test:e2e
```

Expected: both commands pass.

- [ ] **Step 3: Inspect final diff**

Run:

```bash
git status --short
git diff --stat HEAD
```

Expected: only intended browser JS, E2E tests, inline asset include order, and `docs/todo/TODO.md` are changed since the last commit.

- [ ] **Step 4: Commit TODO update**

```bash
git add docs/todo/TODO.md
git commit -m "docs: content controller分割の完了を記録"
```

## Self-Review

Spec coverage:

- Module split is covered by Tasks 2-6.
- Explicit controller API is covered by Task 6.
- `content-renderer.js` sanitized HTML boundary is preserved by Task 6, which calls `applyValidatedUpdateHtml` without moving it.
- Search HTML injection risk is covered by Task 1 and Task 4/5 verification.
- Production `window` exposure is covered by Task 1 and Task 6 verification.
- Existing behavior for search, directory search, Markdown links, memo citation jump, and update contracts is covered by Tasks 3-7 E2E commands.
- Rollback remains PR-revertable because no server/renderer contract changes are included.

Placeholder scan:

- No `TBD`, `TODO`, `implement later`, or unspecified validation steps remain.
- Every task names exact files, commands, expected outcomes, and commit commands.

Type/name consistency:

- Factory names are consistent across tests, file names, and include order: `createContentEnhancements`, `createContentNavigation`, `createDocumentSearchController`, `createDirectorySearchController`, `createContentController`.
- Public controller methods match the design: `setup`, `updateContent`, `applyPendingUpdate`, `restoreNavigationFromLocation`, `openDocumentSearch`, `moveDocumentSearch`, `applyDocumentSearchQuery`.
