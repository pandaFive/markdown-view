# Browser JS Deglobalization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce browser-side global state by moving shared DOM references and mutable application state into an injected context object while preserving the current no-build inline JavaScript delivery model.

**Architecture:** Keep `src/template/assets/inline_script.rs` concatenation and the existing JavaScript files, but introduce an explicit `appContext` object in `bootstrap.js`. Migrate call sites incrementally from implicit globals to `ctx` parameters and controller-style factories, leaving only the bootstrapping surface intentionally global for E2E compatibility until the final task narrows it.

The main alternative is an IIFE closure that wraps the concatenated bundle and keeps most call sites unchanged. This plan chooses explicit `ctx` injection instead because it makes dependencies visible at each function boundary, supports targeted unit-style extraction later, and prepares the code for a future ES module or bundled layout without changing the no-build delivery model now. The cost is a larger call-site diff; the mitigation is task-by-task migration with targeted E2E coverage after each state group moves.

**Tech Stack:** Plain browser JavaScript, Rust `include_str!` inline assets, Axum-rendered HTML, Playwright E2E tests, Rust template tests.

---

## Scope And Completion Criteria

**In scope:**
- Move content DOM references, content update state, text selection deferral state, file navigation state, memo state, document search state, directory search state, WebSocket state, and sidebar/TOC state out of unstructured top-level globals.
- Keep the inline script concatenation approach and current browser support assumptions.
- Keep E2E-only hooks available, but expose them only under `window.__MV_E2E__ === true`.

**Out of scope:**
- Introducing ES modules, a bundler, TypeScript for production browser JS, or a package build step.
- Rewriting the UI behavior, CSS, server API, renderer, or WebSocket protocol.

**Done means:**
- `rg "var (documentSearchInputEl|documentSearchSummaryEl|documentSearchResultsEl|documentSearchPrevEl|documentSearchNextEl|documentSearchClearEl|documentSearchMatches|currentDocumentSearch|currentDirectorySearch|documentSearchDebounceTimer|documentSearchFetchGeneration|pendingDirectorySearchNavigation|memoLoadGeneration|memoSaveGeneration|fetchGeneration|currentFile|pendingUpdate|isMouseSelecting|ws|reconnectAttempts)" src/template/assets/js` returns no migrated global declarations.
- Production pages do not expose `window.updateContent`, `window.selectFile`, or `window.markdownViewTestHooks`.
- Production pages do not expose boot, context, or helper functions such as `window.appContext`, `window.startMarkdownViewApp`, `window.connectWS`, `window.applyDocumentSearchQuery`, or `window.markPendingTocNavigation`.
- E2E pages with `window.__MV_E2E__ === true` expose a single `window.markdownViewTestHooks` object containing every hook used by tests.
- CSP script hashes continue to be generated from `src/template/assets.rs` / `inline_js()` and are verified by Rust tests; no manual CSP hash update is needed.
- `./verify.sh`, `npm run typecheck`, and `npm run test:e2e` pass at the end.

## File Structure

- Modify `src/template/assets/js/bootstrap.js`: create `createAppContext(document)` and hold app state in one object.
- Modify `src/template/assets/js/content.js`: pass `ctx` into content helpers and remove direct reads of migrated globals.
- Modify `src/template/assets/js/memo.js`: pass `ctx` into memo helpers and keep autosave state inside `ctx.memo`.
- Modify `src/template/assets/js/fetch.js`: pass `ctx` into file selection and keep navigation generation inside `ctx.fetch`.
- Modify `src/template/assets/js/websocket.js`: pass `ctx` and explicit dependencies into WebSocket setup.
- Modify `src/template/assets/js/sidebar.js`: replace final top-level boot calls with `startMarkdownViewApp()`.
- Modify `src/template/assets/js/selection.js`: move selection deferral state access to `ctx.state`.
- Modify `src/template/assets/inline_script.rs`: keep script order stable while migration is in progress.
- Modify `tests/e2e/globals.d.ts`: keep only intentionally exposed test hooks.
- Add or modify E2E tests under `tests/e2e/` only when a task changes observable browser behavior.

## Migration Rules

- Top-level `var appContext = createAppContext(document);` is the only new shared root until Task 10 creates the final boot wrapper.
- Existing globals may temporarily coexist with `ctx` fields inside one task, but each task must state which side is authoritative before the task ends.
- Do not add top-level reads of `appContext` to files that are loaded before `bootstrap.js`; current `inline_script.rs` order already loads `bootstrap.js` first, so event listeners and later boot code may reference `appContext`.
- Use `ctx*` names for injected context parameters and keep legacy top-level globals unprefixed only until their removal step.
- Keep state in `ctx` when multiple feature files need to read or mutate it, or when E2E hooks must inspect or set it. Keep state inside a controller closure only when it is owned by that controller and external code needs only a narrow method surface. That is why selection/content deferral state moves to `ctx.state`, while WebSocket buffer internals stay private behind `ctx.websocket.scheduleBufferedLiveUpdate`, `ctx.websocket.discardBufferedLiveUpdate`, and `ctx.websocket.rememberAppliedLiveUpdate`.
- Note: the rules above describe the original migration direction; the final implementation intentionally diverges. See **Self-Review Notes**: Tasks 2-7 did not fully apply `ctx` parameter injection to every helper, and the final bundle instead uses one private IIFE-scoped `appContext` root while reserving explicit controller closure state for WebSocket-owned internals.
- Inline script CSP hashes are derived from `inline_js()` in `src/template/assets.rs`; any JS text change updates the runtime hash automatically. Each JS-changing task still runs `cargo test --all-targets --all-features` or `./verify.sh` so CSP integration tests catch hash/header regressions.
- Each migration task ends with the same pattern: `rg` for legacy names, targeted E2E or full verification, then a focused commit. The task sections repeat exact commands so an implementer can execute tasks independently.

## Task 1: Add AppContext Without Behavior Changes

**Files:**
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/sidebar.js`
- Test: existing Rust template tests and E2E smoke coverage

- [ ] **Step 1: Add a context factory beside the existing globals**

Add this near the top of `bootstrap.js`, using the template placeholder as the single source for the configured file-size limit:

```js
function createAppContext(doc) {
  var html = doc.documentElement;
  return {
    config: {
      maxFileSizeMb: __MAX_FILE_SIZE_MB__,
      isDirMode: html.getAttribute('data-dir-mode') === 'true'
    },
    state: {
      currentFile: html.getAttribute('data-current-file') || '',
      lastAppliedContent: null,
      pendingUpdate: null,
      pendingUpdateTimer: null
    },
    elements: {
      htmlEl: html,
      documentTitleEl: doc.getElementById('document-title'),
      docHeadingCountEl: doc.getElementById('doc-heading-count'),
      docCharCountEl: doc.getElementById('doc-char-count'),
      liveStatusEl: doc.getElementById('live-status'),
      readingProgressBar: doc.getElementById('reading-progress-bar'),
      backToTop: doc.getElementById('back-to-top'),
      contentRoot: doc.getElementById('content')
    },
    labels: {
      liveStatus: {
        live: 'Live',
        retry: 'Reconnecting',
        error: 'Error',
        offline: 'Offline'
      }
    }
  };
}
```

At this point the new `ctx.state.lastAppliedContent`, `ctx.state.pendingUpdate`, and `ctx.state.pendingUpdateTimer` are only mirrored placeholders. The existing globals remain authoritative until Task 3 migrates the related reads and writes.

- [ ] **Step 2: Create the context while keeping current globals**

Add this after the current top-level DOM references in `bootstrap.js`:

```js
var appContext = createAppContext(document);
```

Expected behavior: no current function uses `appContext` yet, so rendering and WebSocket behavior remain unchanged.

- [ ] **Step 3: Run targeted verification**

Run:

```bash
cargo test template::tests::test_websocket_json_parse_error時の視覚フィードバックjsが埋め込まれる
cargo test --all-targets --all-features
npm run typecheck
```

Expected: all commands pass. The existing template assertion should keep passing because Task 1 only adds `createAppContext` and does not remove the WebSocket parse-error banner code that the test searches for.

- [ ] **Step 4: Commit**

```bash
git add src/template/assets/js/bootstrap.js
git commit -m "refactor: ブラウザJSのAppContext導入"
```

## Task 2: Move Immutable DOM References To Context

**Files:**
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/content.js`
- Modify: `src/template/assets/js/fetch.js`
- Modify: `src/template/assets/js/sidebar.js`
- Modify: `src/template/assets/js/websocket.js`

- [ ] **Step 1: Convert read-only DOM helper functions first**

Change functions such as `setLiveStatus`, `updateDocumentStats`, `updateReadingProgress`, and `syncDocumentChrome` in `content.js` to accept `ctx`:

```js
function setLiveStatus(ctx, state) {
  var liveStatusEl = ctx.elements.liveStatusEl;
  if (!liveStatusEl) return;
  liveStatusEl.textContent = ctx.labels.liveStatus[state] || state;
  liveStatusEl.dataset.state = state;
}
```

- [ ] **Step 2: Update call sites to pass `appContext`**

Replace calls like:

```js
setLiveStatus('live');
```

with:

```js
setLiveStatus(appContext, 'live');
```

Apply this to every call site in the concatenated browser bundle, not only `content.js`. In particular, migrate the `setLiveStatus('live')` and `setLiveStatus('error')` calls in `fetch.js` in the same task.

Verify all call sites have the new signature:

```bash
rg "setLiveStatus\\(" src/template/assets/js
```

Expected: every call passes `appContext` or an injected `ctx` as the first argument. There are no remaining `setLiveStatus('...')` calls.

- [ ] **Step 3: Remove duplicated immutable globals**

After all call sites use `ctx.elements`, remove the corresponding top-level variables from `bootstrap.js`:

```js
var documentTitleEl = document.getElementById('document-title');
var docHeadingCountEl = document.getElementById('doc-heading-count');
var docCharCountEl = document.getElementById('doc-char-count');
var liveStatusEl = document.getElementById('live-status');
var readingProgressBar = document.getElementById('reading-progress-bar');
var backToTop = document.getElementById('back-to-top');
var contentRoot = document.getElementById('content');
```

Then verify no removed DOM globals remain outside `ctx.elements`:

```bash
rg "documentTitleEl|docHeadingCountEl|docCharCountEl|liveStatusEl|readingProgressBar|backToTop|contentRoot" src/template/assets/js
```

Expected: matches are only inside `createAppContext`, local variables assigned from `ctx.elements`, or comments explaining the migration.

- [ ] **Step 4: Run focused E2E tests**

Run:

```bash
cargo test --all-targets --all-features
npm run typecheck
npm run test:e2e -- tests/e2e/helpers.spec.ts tests/e2e/document_search.spec.ts
```

Expected: typecheck passes and both E2E files pass.

- [ ] **Step 5: Commit**

```bash
git add src/template/assets/js/bootstrap.js src/template/assets/js/content.js src/template/assets/js/fetch.js src/template/assets/js/sidebar.js src/template/assets/js/websocket.js
git commit -m "refactor: DOM参照をAppContext経由に移行"
```

## Task 3: Move Mutable Content And Selection State To Context

**Files:**
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/content.js`
- Modify: `src/template/assets/js/selection.js`
- Modify: `src/template/assets/js/websocket.js`

- [ ] **Step 1: Add explicit state groups**

Extend `createAppContext`:

```js
state: {
  currentFile: html.getAttribute('data-current-file') || '',
  lastAppliedContent: null,
  pendingUpdate: null,
  pendingUpdateTimer: null,
  isMouseSelecting: false
}
```

- [ ] **Step 2: Update selection helpers**

Change `selection.js` helpers to accept `ctx`:

```js
function isTextSelected(ctx) {
  if (ctx.state.isMouseSelecting) return true;
  var sel = window.getSelection();
  return sel && !sel.isCollapsed;
}

function ensurePendingUpdateTimer(ctx) {
  if (!ctx.state.pendingUpdateTimer) {
    ctx.state.pendingUpdateTimer = setTimeout(function() {
      ctx.state.pendingUpdateTimer = null;
      applyPendingUpdate(ctx);
    }, 30000);
  }
}
```

- [ ] **Step 3: Update event listeners**

Use `appContext` inside the existing `mousedown`, `mouseup`, and `selectionchange` listeners:

```js
appContext.state.isMouseSelecting = true;
```

and:

```js
if (sel && sel.isCollapsed && appContext.state.pendingUpdate) {
  applyPendingUpdate(appContext);
}
```

- [ ] **Step 4: Migrate content update cache**

Change `updateContent`, `applyPendingUpdate`, and WebSocket buffering code to read and write:

```js
ctx.state.lastAppliedContent
ctx.state.pendingUpdate
ctx.state.pendingUpdateTimer
```

- [ ] **Step 5: Remove migrated globals**

Remove these variables from `bootstrap.js` once no references remain:

```js
var lastAppliedContent = null;
var pendingUpdate = null;
var pendingUpdateTimer = null;
var isMouseSelecting = false;
```

During Task 3, `ctx.state.pendingUpdate`, `ctx.state.pendingUpdateTimer`, and `ctx.state.isMouseSelecting` become authoritative before the legacy globals are removed. Do not keep writes to both locations after Step 4.

Verify no removed state globals remain:

```bash
rg "lastAppliedContent|pendingUpdate|pendingUpdateTimer|isMouseSelecting" src/template/assets/js
```

Expected: matches are only `ctx.state.*` references or comments explaining the migration.

- [ ] **Step 6: Run selection regression test**

Run:

```bash
cargo test --all-targets --all-features
npm run test:e2e -- tests/e2e/text_selection_defer.spec.ts
```

Expected: text selection is preserved while live updates are deferred.

- [ ] **Step 7: Commit**

```bash
git add src/template/assets/js/bootstrap.js src/template/assets/js/content.js src/template/assets/js/selection.js src/template/assets/js/websocket.js
git commit -m "refactor: 本文更新状態をAppContextへ移行"
```

## Task 4: Move File Navigation State To Context

**Files:**
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/fetch.js`
- Modify: `src/template/assets/js/memo.js`
- Modify: `src/template/assets/js/content.js`
- Modify: `src/template/assets/js/sidebar.js`
- Modify: `src/template/assets/js/websocket.js`
- Modify: `tests/e2e/globals.d.ts`

- [ ] **Step 1: Add fetch state**

Extend `createAppContext`:

```js
fetch: {
  generation: 0
}
```

- [ ] **Step 2: Change `selectFile` signature**

Change:

```js
function selectFile(file, pushHistory, options) {
```

to:

```js
function selectFile(ctx, file, pushHistory, options) {
```

Inside the function, replace `currentFile` with `ctx.state.currentFile` and `fetchGeneration` with `ctx.fetch.generation`.

Task 4 makes `ctx.fetch.generation` authoritative for both content and memo request ownership. Do not leave a parallel `var fetchGeneration` counter in `fetch.js`.

Task 4 also makes `ctx.state.currentFile` authoritative for the active file. Do not update only `selectFile`; migrate all same-bundle reads and writes of `currentFile` in `content.js`, `fetch.js`, `memo.js`, `sidebar.js`, and `websocket.js` during this task. If a call site cannot be migrated immediately, keep a temporary compatibility getter/setter that reads and writes `ctx.state.currentFile`; do not keep an independent `var currentFile`.

- [ ] **Step 3: Keep a temporary E2E compatibility wrapper**

At the bottom of `fetch.js`, expose this wrapper only while existing tests still call `selectFile(...)` and only under the E2E flag. This wrapper is temporary and must be removed in Task 9:

```js
if (window.__MV_E2E__ === true) {
  window.selectFile = function(file, pushHistory, options) {
    return selectFile(appContext, file, pushHistory, options);
  };
}
```

Production pages must not expose `window.selectFile` at any intermediate commit.

- [ ] **Step 4: Move memo ownership checks to the same generation counter**

Replace `memo.js` reads of `fetchGeneration` with `ctx.fetch.generation` in the same task as the `fetch.js` counter migration. Also update `websocket.js` and `content.js` call sites that read `currentFile` so they read `ctx.state.currentFile`. Representative replacements:

```js
loadMemo(ctx, ctx.state.currentFile, ctx.fetch.generation);
if (ownerGeneration !== undefined && ownerGeneration !== ctx.fetch.generation) return;
if (ctx.config.isDirMode && data.file !== ctx.state.currentFile) return;
```

This prevents split-brain request ownership where `selectFile` increments `ctx.fetch.generation` while `memo.js` still checks the old global `fetchGeneration`.

- [ ] **Step 5: Update internal call sites**

Replace internal calls in `content.js`, `fetch.js`, `sidebar.js`, and `websocket.js`:

```js
selectFile(file, false);
```

with:

```js
selectFile(appContext, file, false);
```

Representative `currentFile` replacements:

```js
currentFile = file;
```

becomes:

```js
ctx.state.currentFile = file;
```

and:

```js
if (isDirMode && data.file !== currentFile) return;
```

becomes:

```js
if (ctx.config.isDirMode && data.file !== ctx.state.currentFile) return;
```

- [ ] **Step 6: Verify no legacy navigation globals remain**

Run:

```bash
rg "fetchGeneration|currentFile" src/template/assets/js
```

Expected: `fetchGeneration` has no matches. `currentFile` matches only `ctx.state.currentFile`, test-hook wrappers, comments, or compatibility code scheduled for removal in Task 9. There is no `var currentFile` declaration.

- [ ] **Step 7: Run navigation and search tests**

Run:

```bash
cargo test --all-targets --all-features
npm run typecheck
npm run test:e2e -- tests/e2e/document_search.spec.ts tests/e2e/markdown_links.spec.ts tests/e2e/memo_jump.spec.ts
```

Expected: directory navigation, Markdown links, search result navigation, and memo source links still work.

- [ ] **Step 8: Commit**

```bash
git add src/template/assets/js/bootstrap.js src/template/assets/js/fetch.js src/template/assets/js/memo.js src/template/assets/js/content.js src/template/assets/js/sidebar.js src/template/assets/js/websocket.js tests/e2e/globals.d.ts
git commit -m "refactor: ファイル選択状態をAppContextへ移行"
```

## Task 5: Move Memo State To Context

**Files:**
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/memo.js`
- Modify: `src/template/assets/js/fetch.js`
- Modify: `src/template/assets/js/websocket.js`

- [ ] **Step 1: Add memo elements and state**

Extend `elements` first:

```js
memoEditorEl: doc.getElementById('memo-editor'),
memoPreviewEl: doc.getElementById('memo-preview'),
memoSaveStatusEl: doc.getElementById('memo-save-status'),
quoteSelectionActionEl: doc.getElementById('quote-selection-action')
```

Add these keys to the existing `elements` object; do not replace the previously migrated `elements` keys.

Extend `createAppContext`:

```js
memo: {
  loadGeneration: 0,
  saveGeneration: 0,
  saveTimer: null,
  caretStart: 0,
  caretEnd: 0,
  pendingReload: null
}
```

After context creation, initialize caret from the editor:

```js
appContext.memo.caretStart = appContext.elements.memoEditorEl
  ? appContext.elements.memoEditorEl.value.length
  : 0;
appContext.memo.caretEnd = appContext.memo.caretStart;
```

After this step, `ctx.memo.caretStart` and `ctx.memo.caretEnd` are the authoritative caret state. Any remaining legacy `memoCaretStart` and `memoCaretEnd` variables must be read-only only until the next step removes or rewrites their call sites; do not write both locations.

- [ ] **Step 2: Change memo helpers to accept `ctx`**

Change representative signatures:

```js
function loadMemo(ctx, file, ownerGeneration) {
function saveMemoNow(ctx, targetFileOverride, rawOverride) {
function applyRemoteMemoUpdate(ctx, data) {
```

Replace `memoLoadGeneration`, `memoSaveGeneration`, `memoSaveTimer`, `pendingMemoReload`, and memo DOM globals with `ctx.memo.*` and `ctx.elements.*`.

Also replace `memoCaretStart` and `memoCaretEnd` with `ctx.memo.caretStart` and `ctx.memo.caretEnd`.

- [ ] **Step 3: Update callers**

Replace calls like:

```js
loadMemo(currentFile, gen);
applyRemoteMemoUpdate(data);
```

with:

```js
loadMemo(appContext, appContext.state.currentFile, gen);
applyRemoteMemoUpdate(appContext, data);
```

- [ ] **Step 4: Verify no legacy memo globals remain**

Run:

```bash
rg "memoLoadGeneration|memoSaveGeneration|memoSaveTimer|pendingMemoReload|memoCaretStart|memoCaretEnd|memoEditorEl|memoPreviewEl|memoSaveStatusEl|quoteSelectionActionEl" src/template/assets/js
```

Expected: matches are only `ctx.memo.*`, `ctx.elements.*`, or comments explaining the migration.

- [ ] **Step 5: Run memo E2E tests**

Run:

```bash
cargo test --all-targets --all-features
npm run test:e2e -- tests/e2e/memo_sync.spec.ts tests/e2e/memo_quote.spec.ts tests/e2e/memo_jump.spec.ts
```

Expected: autosave, remote memo refresh, quote insertion, and source jumping still work.

- [ ] **Step 6: Commit**

```bash
git add src/template/assets/js/bootstrap.js src/template/assets/js/memo.js src/template/assets/js/fetch.js src/template/assets/js/websocket.js
git commit -m "refactor: メモ状態をAppContextへ移行"
```

## Task 6: Move Search State To Context

**Files:**
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/content.js`
- Modify: `src/template/assets/js/fetch.js`
- Modify: `src/template/assets/js/sidebar.js`
- Modify: `tests/e2e/globals.d.ts`

- [ ] **Step 1: Add search elements and state**

Extend `createAppContext` with search DOM references:

```js
elements: {
  documentSearchInputEl: doc.getElementById('document-search-input'),
  documentSearchSummaryEl: doc.getElementById('document-search-summary'),
  documentSearchResultsEl: doc.getElementById('document-search-results'),
  documentSearchPrevEl: doc.getElementById('document-search-prev'),
  documentSearchNextEl: doc.getElementById('document-search-next'),
  documentSearchClearEl: doc.getElementById('document-search-clear')
}
```

Add these keys to the existing `elements` object; do not replace the existing content and memo element keys.

Add search state:

```js
search: {
  documentMatches: [],
  currentDocumentIndex: -1,
  currentDocumentQuery: '',
  currentDirectoryResults: [],
  currentDirectoryIndex: -1,
  currentDirectorySkippedFiles: 0,
  currentDirectoryLoading: false,
  currentDirectoryError: '',
  documentDebounceTimer: null,
  documentFetchGeneration: 0,
  pendingDirectoryNavigation: null
}
```

- [ ] **Step 2: Migrate document-search helpers**

Change document-search helpers in `content.js` to accept `ctx` and replace direct globals:

```js
function applyDocumentSearchQuery(ctx, query) {
  ctx.search.currentDocumentQuery = query;
  ctx.search.currentDocumentIndex = -1;
  ctx.search.documentMatches = collectDocumentSearchMatches(ctx, query);
}
```

Representative replacements:

```js
documentSearchMatches -> ctx.search.documentMatches
currentDocumentSearchIndex -> ctx.search.currentDocumentIndex
currentDocumentSearchQuery -> ctx.search.currentDocumentQuery
documentSearchDebounceTimer -> ctx.search.documentDebounceTimer
documentSearchInputEl -> ctx.elements.documentSearchInputEl
```

- [ ] **Step 3: Migrate directory-search helpers**

Replace directory search state with `ctx.search.*`:

```js
currentDirectorySearchResults -> ctx.search.currentDirectoryResults
currentDirectorySearchIndex -> ctx.search.currentDirectoryIndex
currentDirectorySearchSkippedFiles -> ctx.search.currentDirectorySkippedFiles
currentDirectorySearchLoading -> ctx.search.currentDirectoryLoading
currentDirectorySearchError -> ctx.search.currentDirectoryError
documentSearchFetchGeneration -> ctx.search.documentFetchGeneration
pendingDirectorySearchNavigation -> ctx.search.pendingDirectoryNavigation
```

Update cross-file call sites in `fetch.js` and `sidebar.js` to pass `appContext`.

- [ ] **Step 4: Keep temporary E2E compatibility wrappers**

Expose wrappers only under the existing E2E guard until Task 9 consolidates them:

```js
if (window.__MV_E2E__ === true) {
  window.applyDocumentSearchQuery = function(query) {
    return applyDocumentSearchQuery(appContext, query);
  };
  window.moveDocumentSearch = function(direction) {
    return moveDocumentSearch(appContext, direction);
  };
}
```

- [ ] **Step 5: Verify no legacy search globals remain**

Run:

```bash
rg "documentSearch(Input|Summary|Results|Prev|Next|Clear)El|documentSearchMatches|currentDocumentSearch|currentDirectorySearch|documentSearchDebounceTimer|documentSearchFetchGeneration|pendingDirectorySearchNavigation" src/template/assets/js
```

Expected: matches are only `ctx.elements.*`, `ctx.search.*`, E2E wrappers guarded by `window.__MV_E2E__ === true`, or comments explaining the migration.

- [ ] **Step 6: Run search tests**

Run:

```bash
cargo test --all-targets --all-features
npm run typecheck
npm run test:e2e -- tests/e2e/document_search.spec.ts tests/e2e/markdown_links.spec.ts
```

Expected: document search, directory search, and file navigation from search results still work.

- [ ] **Step 7: Commit**

```bash
git add src/template/assets/js/bootstrap.js src/template/assets/js/content.js src/template/assets/js/fetch.js src/template/assets/js/sidebar.js tests/e2e/globals.d.ts
git commit -m "refactor: 検索状態をAppContextへ移行"
```

## Task 7: Move Sidebar And TOC State To Context

**Files:**
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/sidebar.js`
- Modify: `src/template/assets/js/content.js`

- [ ] **Step 1: Add sidebar and TOC state**

Extend `createAppContext`:

```js
sidebar: {
  currentTocTracking: null,
  tocTrackingFrame: null,
  currentActiveTocId: '',
  suppressTocTrackingUntil: 0,
  suppressTocTrackingTimer: null,
  pendingSuppressedTocTrackingUpdate: false,
  pendingTocNavigationId: '',
  pendingTocNavigationUntil: 0,
  tocRoot: doc.getElementById('toc')
},
test: {
  markPendingTocNavigationObserver: null
}
```

- [ ] **Step 2: Change sidebar and TOC helpers to accept `ctx`**

Representative signature changes:

```js
function activateSidebarTab(ctx, target) {
function markPendingTocNavigation(ctx, id) {
function updateActiveTocHeading(ctx) {
function getViewportActiveTocId(ctx) {
```

Preserve the existing E2E ability to observe `markPendingTocNavigation` calls without exposing the internal function directly. Add this at the start of `markPendingTocNavigation(ctx, id)`:

```js
if (ctx.test.markPendingTocNavigationObserver) {
  ctx.test.markPendingTocNavigationObserver(id);
}
```

The observer is notification-only. It must not replace or short-circuit the existing `markPendingTocNavigation` body; the original navigation logic always continues after the observer call.

Representative state replacements:

```js
currentTocTracking -> ctx.sidebar.currentTocTracking
tocTrackingFrame -> ctx.sidebar.tocTrackingFrame
currentActiveTocId -> ctx.sidebar.currentActiveTocId
suppressTocTrackingUntil -> ctx.sidebar.suppressTocTrackingUntil
pendingTocNavigationId -> ctx.sidebar.pendingTocNavigationId
pendingTocNavigationUntil -> ctx.sidebar.pendingTocNavigationUntil
tocRoot -> ctx.sidebar.tocRoot
```

- [ ] **Step 3: Update content-to-TOC call sites**

Update call sites in `content.js` that currently call TOC helpers directly:

```js
markPendingTocNavigation(appContext, parsed.headingId);
```

Do not leave mixed call signatures for the same helper.

- [ ] **Step 4: Verify no legacy sidebar globals remain**

Run:

```bash
rg "currentTocTracking|tocTrackingFrame|currentActiveTocId|suppressTocTracking|pendingSuppressedTocTrackingUpdate|pendingTocNavigation|tocRoot" src/template/assets/js
```

Expected: matches are only `ctx.sidebar.*`, local variables, or comments explaining the migration.

- [ ] **Step 5: Run sidebar and navigation tests**

Run:

```bash
cargo test --all-targets --all-features
npm run typecheck
npm run test:e2e -- tests/e2e/document_search.spec.ts tests/e2e/memo_jump.spec.ts tests/e2e/text_selection_defer.spec.ts
```

Expected: sidebar tabs, active TOC tracking, pending TOC navigation, search, and memo jump behavior still work.

- [ ] **Step 6: Commit**

```bash
git add src/template/assets/js/bootstrap.js src/template/assets/js/sidebar.js src/template/assets/js/content.js
git commit -m "refactor: サイドバーと目次状態をAppContextへ移行"
```

## Task 8: Convert WebSocket To An Injected Controller

**Files:**
- Modify: `src/template/assets/js/websocket.js`
- Modify: `src/template/assets/js/sidebar.js`
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/fetch.js`
- Modify: `src/template/assets/js/content.js`

- [ ] **Step 1: Wrap WebSocket state in a factory**

Replace top-level WebSocket mutable variables with:

```js
function createWebSocketController(ctx, deps) {
  var ws = null;
  var reconnectAttempts = 0;
  var pendingWsUpdate = null;
  var pendingWsUpdateSignature = '';
  var pendingWsUpdateTimer = null;
  var lastAppliedUpdateSignature = '';

  function connect() {
    var protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    ws = new WebSocket(protocol + '//' + location.host + '/ws');
    // move existing onopen/onmessage/onclose/onerror body here
  }

  return {
    connect: connect,
    discardBufferedLiveUpdate: discardBufferedLiveUpdate,
    rememberAppliedLiveUpdate: rememberAppliedLiveUpdate,
    scheduleBufferedLiveUpdate: scheduleBufferedLiveUpdate
  };
}
```

Add a controller slot to `appContext` before startup:

```js
appContext.websocket = null;
```

- [ ] **Step 2: Inject dependencies instead of calling globals**

Inside the controller, call:

```js
deps.updateContent(ctx, data);
deps.selectFile(ctx, ctx.state.currentFile, false);
deps.applyRemoteMemoUpdate(ctx, data);
deps.queueRemoteMemoReload(ctx, data);
```

- [ ] **Step 3: Start the controller from the boot sequence**

In `sidebar.js`, replace:

```js
connectWS();
```

with:

```js
appContext.websocket = createWebSocketController(appContext, {
  updateContent: updateContent,
  selectFile: selectFile,
  applyRemoteMemoUpdate: applyRemoteMemoUpdate,
  queueRemoteMemoReload: queueRemoteMemoReload
});
appContext.websocket.connect();
```

- [ ] **Step 4: Route existing cross-file dependencies through the controller**

`fetch.js` and `content.js` currently call WebSocket buffer helpers as globals. Replace those calls with the controller stored on context:

```js
if (ctx.websocket) {
  ctx.websocket.discardBufferedLiveUpdate();
}
```

and:

```js
if (ctx.websocket) {
  ctx.websocket.rememberAppliedLiveUpdate(data);
}
```

Update any `scheduleBufferedLiveUpdate(...)` test hook path to call `ctx.websocket.scheduleBufferedLiveUpdate(...)`.

Verify no global helper calls remain outside `websocket.js`:

```bash
rg "discardBufferedLiveUpdate\\(|rememberAppliedLiveUpdate\\(|scheduleBufferedLiveUpdate\\(" src/template/assets/js
```

Expected: non-`websocket.js` matches call through `ctx.websocket` or through the guarded test hook object. There are no bare global calls in `fetch.js` or `content.js`.

- [ ] **Step 5: Run WebSocket-focused tests**

Run:

```bash
cargo test --test integration_test websocket
cargo test --all-targets --all-features
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts tests/e2e/text_selection_defer.spec.ts
```

Expected: WebSocket initial load, update exposure behavior, and deferred selection updates still pass.

- [ ] **Step 6: Commit**

```bash
git add src/template/assets/js/websocket.js src/template/assets/js/sidebar.js src/template/assets/js/bootstrap.js src/template/assets/js/fetch.js src/template/assets/js/content.js
git commit -m "refactor: WebSocket処理を依存注入型に整理"
```

## Task 9: Narrow Intentional Global Test Hooks

**Files:**
- Modify: `src/template/assets/js/sidebar.js`
- Modify: `tests/e2e/globals.d.ts`
- Modify: `tests/e2e/helpers.ts`
- Modify: E2E tests that directly call removed globals

- [ ] **Step 1: Create a single test hook object**

Near the current startup code, create an `installMarkdownViewTestHooks(ctx)` function and call it from the current startup area. Expose hooks only under `window.__MV_E2E__ === true`; production must not expose `window.markdownViewTestHooks`. Task 10 will move only the call site into the final `startMarkdownViewApp()` boot function, avoiding a second rewrite of the hook object.

```js
function installMarkdownViewTestHooks(ctx) {
  if (window.__MV_E2E__ !== true) return;
  window.markdownViewTestHooks = {
    activateSidebarTab: function(target) {
      return activateSidebarTab(ctx, target);
    },
    applyDocumentSearchQuery: function(query) {
      return applyDocumentSearchQuery(ctx, query);
    },
    augmentHashWithTrailingLineHint: function(link, hash) {
      return augmentHashWithTrailingLineHint(ctx, link, hash);
    },
    markPendingTocNavigation: function(id) {
      return markPendingTocNavigation(ctx, id);
    },
    setMarkPendingTocNavigationObserverForTest: function(callback) {
      ctx.test.markPendingTocNavigationObserver = typeof callback === 'function'
        ? callback
        : null;
    },
    moveDocumentSearch: function(direction) {
      return moveDocumentSearch(ctx, direction);
    },
    scheduleBufferedLiveUpdate: function(data) {
      return ctx.websocket.scheduleBufferedLiveUpdate(data);
    },
    selectFile: function(file, pushHistory, options) {
      return selectFile(ctx, file, pushHistory, options);
    },
    setCurrentFileForTest: function(file) {
      ctx.state.currentFile = file || '';
    },
    setDirModeForTest: function(value) {
      ctx.config.isDirMode = !!value;
    },
    updateContent: function(data, options) {
      return updateContent(ctx, data, options);
    },
    get isDirMode() {
      return ctx.config.isDirMode;
    },
    get currentFile() {
      return ctx.state.currentFile;
    },
    get lastAppliedContent() {
      return ctx.state.lastAppliedContent;
    }
  };
}

installMarkdownViewTestHooks(appContext);
```

- [ ] **Step 2: Update TypeScript global declarations**

Replace direct global declarations in `tests/e2e/globals.d.ts` with a single hook object. Include every hook currently used by E2E tests:

```ts
declare global {
  namespace MvE2E {
    type UpdateMessage = Partial<UpdateContentPayload> & {
      file?: string;
      refresh?: boolean;
      memo_refresh?: boolean;
      memo_file?: string;
      type?: string;
      error?: string;
      raw?: string;
      html?: string;
      load_error?: string;
    };
  }

  interface Window {
    markdownViewTestHooks: {
      activateSidebarTab(target: string): void;
      applyDocumentSearchQuery(query: string): void;
      augmentHashWithTrailingLineHint(link: HTMLAnchorElement, hash: string): string;
      markPendingTocNavigation(id: string): void;
      moveDocumentSearch(direction: number): void;
      scheduleBufferedLiveUpdate(data: MvE2E.UpdateMessage): void;
      selectFile(file: string, pushHistory?: boolean, options?: MvE2E.UpdateContentOptions): void;
      setCurrentFileForTest(file: string): void;
      setDirModeForTest(value: boolean): void;
      setMarkPendingTocNavigationObserverForTest(callback: ((id: string) => void) | null): void;
      updateContent(data: MvE2E.UpdateMessage, opts?: MvE2E.UpdateContentOptions): void;
      readonly isDirMode: boolean;
      readonly currentFile: string;
      readonly lastAppliedContent: string | null;
    };
  }
}
```

- [ ] **Step 3: Remove temporary direct globals**

Remove compatibility wrappers created in earlier tasks:

```js
if (window.__MV_E2E__ === true) {
  window.selectFile = function(file, pushHistory, options) {
    return selectFile(appContext, file, pushHistory, options);
  };
}
```

Also remove any guarded temporary wrappers for:

```js
window.applyDocumentSearchQuery
window.moveDocumentSearch
window.updateContent
window.markPendingTocNavigation
window.augmentHashWithTrailingLineHint
window.scheduleBufferedLiveUpdate
```

- [ ] **Step 4: Update E2E helpers and direct test calls**

First update `tests/e2e/helpers.ts` so helper functions use `window.markdownViewTestHooks.updateContent(...)` instead of `window.updateContent(...)`, and use `window.markdownViewTestHooks.activateSidebarTab('toc')` instead of direct `activateSidebarTab('toc')`.

Replace E2E snippets like:

```ts
selectFile('notes.md');
```

with:

```ts
window.markdownViewTestHooks.selectFile('notes.md');
```

Apply the same pattern to `markPendingTocNavigation`, `augmentHashWithTrailingLineHint`, `scheduleBufferedLiveUpdate`, `activateSidebarTab`, `applyDocumentSearchQuery`, `moveDocumentSearch`, `isDirMode`, and `currentFile`.

For tests that currently monkey-patch `window.markPendingTocNavigation`, replace the monkey patch with:

```ts
window.markdownViewTestHooks.setMarkPendingTocNavigationObserverForTest((id) => {
  window.__markPendingCalls = (window.__markPendingCalls ?? 0) + 1;
});
```

Clear it at test end when needed:

```ts
window.markdownViewTestHooks.setMarkPendingTocNavigationObserverForTest(null);
```

This change is primarily in `tests/e2e/text_selection_defer.spec.ts` around the existing `window.markPendingTocNavigation` monkey patch.

Replace direct state assignments:

```ts
isDirMode = true;
currentFile = 'README.md';
```

with:

```ts
window.markdownViewTestHooks.setDirModeForTest(true);
window.markdownViewTestHooks.setCurrentFileForTest('README.md');
```

Replace reads:

```ts
currentFile
isDirMode
```

with:

```ts
window.markdownViewTestHooks.currentFile
window.markdownViewTestHooks.isDirMode
```

- [ ] **Step 5: Add exposure regression coverage**

Update `tests/e2e/update_content_exposure.spec.ts` or add a sibling test so production mode verifies:

```ts
await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('undefined');
```

And E2E mode verifies:

```ts
await page.addInitScript(() => {
  window.__MV_E2E__ = true;
});
await expect(page.evaluate(() => typeof window.markdownViewTestHooks.selectFile)).resolves.toBe('function');
```

- [ ] **Step 6: Verify global exposure and migrated declarations**

Run:

```bash
rg "window\\.(selectFile|updateContent|applyDocumentSearchQuery|moveDocumentSearch|markPendingTocNavigation|augmentHashWithTrailingLineHint|scheduleBufferedLiveUpdate)" src/template/assets/js
rg "\\b(selectFile\\(|updateContent\\(|activateSidebarTab\\(|applyDocumentSearchQuery\\(|moveDocumentSearch\\(|augmentHashWithTrailingLineHint\\(|scheduleBufferedLiveUpdate\\(|markPendingTocNavigation\\(|isDirMode\\s*=|currentFile\\s*=)" tests/e2e
```

Expected: direct `window.*` compatibility wrappers are gone, and E2E tests call hooks through `window.markdownViewTestHooks`. Other top-level function declarations still become `window` properties until Task 10 wraps the concatenated script in an IIFE.

- [ ] **Step 7: Run full verification**

Run:

```bash
./verify.sh
npm run typecheck
npm run test:e2e
```

Expected: Rust format, clippy, Rust tests, TypeScript typecheck, and Playwright E2E tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/template/assets/js/sidebar.js tests/e2e/globals.d.ts tests/e2e/helpers.ts tests/e2e
git commit -m "refactor: ブラウザJSの公開テストフックを集約"
```

## Task 10: Close The Boot Scope

**Files:**
- Modify: `src/template/assets/js/bootstrap.js`
- Modify: `src/template/assets/js/selection.js`
- Modify: `src/template/assets/js/content.js`
- Modify: `src/template/assets/js/memo.js`
- Modify: `src/template/assets/js/fetch.js`
- Modify: `src/template/assets/js/websocket.js`
- Modify: `src/template/assets/js/sidebar.js`
- Modify: `src/template/assets/inline_script.rs`
- Modify: `tests/e2e/update_content_exposure.spec.ts`

- [ ] **Step 1: Wrap the concatenated script in one IIFE**

Change `src/template/assets/inline_script.rs` so the generated script starts with an IIFE and ends by calling a single boot function. Keep `bootstrap.js` first inside the wrapper and `sidebar.js` last.

```rust
const TEMPLATE: &str = concat!(
    "(function() {\n",
    include_str!("js/bootstrap.js"),
    "\n",
    include_str!("js/selection.js"),
    "\n",
    include_str!("js/content.js"),
    "\n",
    include_str!("js/memo.js"),
    "\n",
    include_str!("js/fetch.js"),
    "\n",
    include_str!("js/websocket.js"),
    "\n",
    include_str!("js/sidebar.js"),
    "\n",
    "startMarkdownViewApp();\n",
    "}());\n",
);
```

This keeps classic script delivery but prevents top-level `function` and `var` declarations from becoming `window` properties.

- [ ] **Step 2: Move startup side effects into `startMarkdownViewApp`**

Create the boot function in `sidebar.js` or a small new `boot.js` included last. Do not call nonexistent aggregate helpers; either create them in this task or call the concrete setup functions listed in Step 3.

```js
function startMarkdownViewApp() {
  setupSelectionDeferral(appContext);
  setupDocumentSearch(appContext);
  setupContentLinkNavigation(appContext);
  setupMemoLinkNavigation(appContext);
  setupMemoInteractions(appContext);
  setupHistoryUrlSync(appContext);
  setupSidebarInteractions(appContext);
  setupThemeToggle(appContext);

  appContext.websocket = createWebSocketController(appContext, {
    updateContent: updateContent,
    selectFile: selectFile,
    applyRemoteMemoUpdate: applyRemoteMemoUpdate,
    queueRemoteMemoReload: queueRemoteMemoReload
  });
  appContext.websocket.connect();

  installMarkdownViewTestHooks(appContext);
}
```

Move any current top-level startup calls from `sidebar.js` into this function.

- [ ] **Step 3: Move all current top-level side effects into setup functions**

Move these current top-level side effects into explicit setup functions before relying on the IIFE:

```js
// selection.js
document.addEventListener('mousedown', ...)
document.addEventListener('mouseup', ...)
document.addEventListener('selectionchange', ...)

// content.js
setupDocumentSearch();
setupContentLinkNavigation();
setupMemoLinkNavigation();
if (window.__MV_E2E__ === true) { window.updateContent = updateContent; } // if still present before Task 9 cleanup

// memo.js
memoEditorEl.addEventListener(...)
quoteSelectionActionEl.addEventListener(...)
document.addEventListener('selectionchange', ...)
window.addEventListener('scroll', ...)
window.addEventListener('resize', ...)

// fetch.js
if (isDirMode) {
  if (currentFile) {
    setFileParam(currentFile, true);
  }
}

// sidebar.js
sidebarToggle.addEventListener(...)
sidebarOpen.addEventListener(...)
backToTop.addEventListener(...)
tocRoot.addEventListener(...)
themeToggle.addEventListener(...)
connectWS();
setupTocTracking();
restoreActiveTocHeading('');
updateDocumentStats();
updateReadingProgress();
syncDocumentChrome(currentFile);
enhanceContentInteractions();
setupTocFilter();
window.addEventListener('scroll', ...)
window.addEventListener('resize', ...)
setupTabs();
setupFileList();
setupFileFilter();
```

Create concrete setup functions with `ctx` parameters:

```js
function setupSelectionDeferral(ctx) { ... }
function setupHistoryUrlSync(ctx) { ... }
function setupMemoInteractions(ctx) { ... }
function setupSidebarInteractions(ctx) { ... }
function setupThemeToggle(ctx) { ... }
```

Existing setup functions such as `setupDocumentSearch`, `setupContentLinkNavigation`, `setupMemoLinkNavigation`, `setupTocTracking`, `setupTocFilter`, `setupTabs`, `setupFileList`, and `setupFileFilter` should accept `ctx` by the end of this task if they read migrated context state.

`setupHistoryUrlSync(ctx)` should run before the WebSocket controller connects so the URL state is normalized before live updates begin:

```js
function setupHistoryUrlSync(ctx) {
  if (ctx.config.isDirMode && ctx.state.currentFile) {
    setFileParam(ctx.state.currentFile, true);
  }
}
```

- [ ] **Step 4: Keep test hook installation explicit and guarded**

Move the `installMarkdownViewTestHooks(ctx)` call created in Task 9 into `startMarkdownViewApp()`. Keep the hook object implementation from Task 9; only move the call site so the installer runs as part of the final boot sequence.

```js
function startMarkdownViewApp() {
  // ...other setup...
  installMarkdownViewTestHooks(appContext);
}
```

- [ ] **Step 5: Verify the global allowlist**

Add or update `tests/e2e/update_content_exposure.spec.ts` so production pages verify these are not exposed:

```ts
await expect(page.evaluate(() => typeof window.updateContent)).resolves.toBe('undefined');
await expect(page.evaluate(() => typeof window.selectFile)).resolves.toBe('undefined');
await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('undefined');
await expect(page.evaluate(() => typeof window.appContext)).resolves.toBe('undefined');
await expect(page.evaluate(() => typeof window.startMarkdownViewApp)).resolves.toBe('undefined');
await expect(page.evaluate(() => typeof window.connectWS)).resolves.toBe('undefined');
await expect(page.evaluate(() => typeof window.applyDocumentSearchQuery)).resolves.toBe('undefined');
await expect(page.evaluate(() => typeof window.markPendingTocNavigation)).resolves.toBe('undefined');
```

E2E mode should verify only the guarded hook object exists:

```ts
await page.addInitScript(() => {
  window.__MV_E2E__ = true;
});
await expect(page.evaluate(() => typeof window.markdownViewTestHooks)).resolves.toBe('object');
await expect(page.evaluate(() => typeof window.markdownViewTestHooks.selectFile)).resolves.toBe('function');
```

- [ ] **Step 6: Verify no setup side effects remain at top level**

Run:

```bash
rg "^(setupDocumentSearch\\(|setupContentLinkNavigation\\(|setupMemoLinkNavigation\\(|connectWS\\(|setupTocTracking\\(|restoreActiveTocHeading\\(|updateDocumentStats\\(|updateReadingProgress\\(|syncDocumentChrome\\(|enhanceContentInteractions\\(|setupTocFilter\\(|setupTabs\\(|setupFileList\\(|setupFileFilter\\()" src/template/assets/js
rg "^if \\(isDirMode\\)" src/template/assets/js/fetch.js
rg "^(document|window)\\.addEventListener" src/template/assets/js
```

Expected: no matches except inside setup functions or comments. Startup occurs only through `startMarkdownViewApp()` inside the IIFE.

- [ ] **Step 7: Run full verification**

Run:

```bash
./verify.sh
npm run typecheck
npm run test:e2e
```

Expected: Rust format, clippy, Rust tests, TypeScript typecheck, and Playwright E2E tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/template/assets/js src/template/assets/inline_script.rs tests/e2e/update_content_exposure.spec.ts
git commit -m "refactor: ブラウザJSの起動スコープを閉じる"
```

## Self-Review Notes

- The plan preserves inline JavaScript delivery and does not require bundlers or ES modules.
- Each task keeps the application runnable after completion.
- Temporary globals are allowed only as compatibility bridges and are removed or narrowed by Task 10.
- The highest-risk areas are text selection deferral, memo autosave generation counters, and directory navigation history; each has targeted E2E verification.
- Implementation note: Tasks 2-7 intentionally stopped short of full `ctx` parameter injection for every helper. The final implementation wraps the concatenated bundle in an IIFE and keeps one private `appContext` root inside that closure, while using a controller closure only for WebSocket-owned buffer/reconnect state. This diverges from the original "make every dependency visible at each function boundary" direction, but keeps the no-build inline script smaller, avoids a very large call-site churn across 1000+ lines of browser JavaScript, and still satisfies the security-facing goal: production no longer exposes app state or internal helper functions on `window`. Future module extraction should revisit explicit dependency injection at the new module boundaries rather than retrofitting every current helper in place. The final implementation also removed the standalone `MAX_FILE_SIZE_MB` runtime variable and reads the template placeholder through `appContext.config.maxFileSizeMb`.
- Implementation note: Task 8's factory snippet used draft identifiers such as `ws` and `reconnectAttempts`; the final `websocket.js` implementation uses `socket` and `socketReconnectAttempts` to keep the owned state names explicit inside the controller closure.
