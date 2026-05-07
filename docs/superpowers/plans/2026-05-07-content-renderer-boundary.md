# Content Renderer Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> Repository instructions, direct user approval, and branch/worktree safety checks take precedence over the example commands in this plan.

**Goal:** Move `updateContent` payload validation and `#content` / `#toc` HTML application into a focused browser JS content renderer boundary.

**Architecture:** Keep the existing no-build IIFE bundle and add `src/template/assets/js/content-renderer.js` before `content.js` in `inline_script.rs`. `content.js` remains the orchestrator for scroll, TOC, search, memo, and WebSocket side effects, while `content-renderer.js` owns update payload validation, contract warning metadata, TOC HTML normalization, and sanitized HTML assignment.

**Tech Stack:** Plain browser JavaScript, Rust `include_str!` inline assets, Playwright E2E tests, Rust/Cargo verification, repository `./verify.sh`.

---

## Scope And File Structure

**Create:**
- `src/template/assets/js/content-renderer.js`: browser-side update payload contract and sanitized HTML application boundary.

**Modify:**
- `src/template/assets/inline_script.rs`: include `content-renderer.js` after `selection.js` and before `content.js`.
- `src/template/assets/js/content.js`: remove inline payload validation, `normalizeTocHtml`, and direct `#content` / `#toc` assignments from `updateContent`; delegate to `content-renderer.js`.
- `tests/e2e/update_content_exposure.spec.ts`: require new internal renderer functions to be discovered as internal declarations and not exposed on `window`.
- `docs/todo/TODO.md`: split the browser JS TODO into completed `content-renderer` boundary work and remaining search/navigation/controller work.

**Do not modify:**
- Server API or WebSocket payload schema.
- Renderer sanitizer behavior.
- Production JS delivery model, CSP meaning, ES modules, bundlers, or TypeScript setup.

## Task 1: Add Exposure Contract For Content Renderer Internals

**Files:**
- Modify: `tests/e2e/update_content_exposure.spec.ts`

- [ ] **Step 1: Write the failing exposure assertion**

In `tests/e2e/update_content_exposure.spec.ts`, replace the first test with this version:

```ts
test('内部グローバル名抽出は実ファイルから十分な宣言数を拾う', async () => {
  expect(internalGlobalNames.length).toBeGreaterThan(25);
  expect(internalGlobalNames).toContain('startMarkdownViewApp');
  expect(internalGlobalNames).toContain('updateContent');
  expect(internalGlobalNames).toContain('createWebSocketController');
  expect(internalGlobalNames).toContain('validateUpdatePayload');
  expect(internalGlobalNames).toContain('applyValidatedUpdateHtml');
});
```

Add this assertion block near the end of `test('production実行では内部APIを公開しない', ...)`, immediately after `expect(exposed).toEqual([]);`:

```ts
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).validateUpdatePayload)).resolves.toBe('undefined');
  await expect(page.evaluate(() => typeof (window as unknown as Record<string, unknown>).applyValidatedUpdateHtml)).resolves.toBe('undefined');
```

- [ ] **Step 2: Run the targeted E2E test and verify it fails**

Run:

```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts
```

Expected: FAIL because `validateUpdatePayload` and `applyValidatedUpdateHtml` do not exist yet in any browser JS asset.

- [ ] **Step 3: Keep the failing test uncommitted until Task 2**

Do not commit after the failing run. Task 2 will make the test pass and commit the test with the implementation, so the branch history does not contain an intentionally broken commit.

## Task 2: Add `content-renderer.js` And Include It In The Inline Bundle

**Files:**
- Create: `src/template/assets/js/content-renderer.js`
- Modify: `src/template/assets/inline_script.rs`
- Modify: `tests/e2e/update_content_exposure.spec.ts`
- Test: `tests/e2e/update_content_exposure.spec.ts`

- [ ] **Step 1: Create `content-renderer.js`**

Create `src/template/assets/js/content-renderer.js` with exactly this content:

```js
function validateUpdatePayload(data) {
  var safeData = data && typeof data === 'object' && !Array.isArray(data) ? data : {};
  var missing = [];

  if (typeof safeData.content !== 'string') missing.push('content');
  if (typeof safeData.toc !== 'string') missing.push('toc');

  return {
    safeData: safeData,
    missing: missing,
    hasContractViolation: missing.length > 0
  };
}

function logUpdatePayloadContractViolation(validation) {
  var safeData = validation.safeData;
  console.warn('[markdown-view] updateContent: ' + validation.missing.join(', ') + ' が欠落または不正 (契約違反)', {
    missing: validation.missing.slice(),
    file: typeof safeData.file === 'string' ? safeData.file : null,
    contentLength: typeof safeData.content === 'string' ? safeData.content.length : null,
    tocLength: typeof safeData.toc === 'string' ? safeData.toc.length : null
  });
}

function normalizeTocHtml(html) {
  return (html || '').replace(/>\s+</g, '><').trim();
}

// サーバーサイドでサニタイズ済みのHTMLだけを #content に反映する境界。
// XSS防止: src/renderer/render.rs で raw/inline HTML event を破棄済み。
function applySanitizedContentHtml(ctx, contentEl, content) {
  if (!contentEl || typeof content !== 'string') {
    return false;
  }
  if (content === ctx.state.lastAppliedContent) {
    return false;
  }
  contentEl.innerHTML = content;
  ctx.state.lastAppliedContent = content;
  return true;
}

// サーバー生成済みTOC HTMLだけを #toc に反映する境界。
function applySanitizedTocHtml(tocEl, toc) {
  if (!tocEl || typeof toc !== 'string') {
    return false;
  }
  if (normalizeTocHtml(tocEl.innerHTML) === normalizeTocHtml(toc)) {
    return false;
  }
  tocEl.innerHTML = toc;
  return true;
}

function applyValidatedUpdateHtml(ctx, targets, validation) {
  var safeData = validation.safeData;
  return {
    contentChanged: applySanitizedContentHtml(ctx, targets.contentEl, safeData.content),
    tocChanged: applySanitizedTocHtml(targets.tocEl, safeData.toc)
  };
}
```

- [ ] **Step 2: Include the new file before `content.js`**

In `src/template/assets/inline_script.rs`, replace the `TEMPLATE` definition with:

```rust
const TEMPLATE: &str = concat!(
    "(function() {\n",
    include_str!("js/bootstrap.js"),
    "\n",
    include_str!("js/selection.js"),
    "\n",
    include_str!("js/content-renderer.js"),
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

- [ ] **Step 3: Run the exposure test and verify it passes**

Run:

```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts
```

Expected: PASS. The extractor sees the new internal declarations, and production still exposes none of them because the bundle is wrapped in the existing IIFE.

- [ ] **Step 4: Run Rust tests for inline asset/CSP coverage**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS. Existing template/CSP tests continue to derive hashes from `inline_js()`.

- [ ] **Step 5: Commit**

```bash
git add tests/e2e/update_content_exposure.spec.ts src/template/assets/js/content-renderer.js src/template/assets/inline_script.rs
git commit -m "refactor: content renderer境界を追加"
```

## Task 3: Delegate `updateContent` Contract And HTML Application

**Files:**
- Modify: `src/template/assets/js/content.js`
- Test: `tests/e2e/memo_jump.spec.ts`
- Test: `tests/e2e/update_content_exposure.spec.ts`

- [ ] **Step 1: Remove `normalizeTocHtml` from `content.js`**

Delete this function from `src/template/assets/js/content.js` because Task 2 moved it to `content-renderer.js`:

```js
function normalizeTocHtml(html) {
  return (html || '').replace(/>\s+</g, '><').trim();
}
```

- [ ] **Step 2: Replace the first half of `updateContent`**

In `src/template/assets/js/content.js`, replace the start of `updateContent` through the direct `innerHTML` assignments with this code:

```js
// サーバーサイドでサニタイズ済みのHTMLを反映する
// XSS防止: src/renderer/render.rs で raw/inline HTML event を破棄済み
let updateContent = function updateContent(data, options) {
  options = options || {};
  var validation = validateUpdatePayload(data);
  var safeData = validation.safeData;

  if (validation.hasContractViolation) {
    logUpdatePayloadContractViolation(validation);
  }

  if (appContext.state.pendingUpdateTimer) {
    clearTimeout(appContext.state.pendingUpdateTimer);
    appContext.state.pendingUpdateTimer = null;
  }
  appContext.state.pendingUpdate = null;
  var scrollY = window.scrollY;
  var scrollMode = options.scrollMode || 'preserve';
  var preservedActiveTocId = getCurrentActiveTocId();
  var contentEl = document.getElementById('content');
  var tocEl = document.getElementById('toc');

  applyValidatedUpdateHtml(appContext, {
    contentEl: contentEl,
    tocEl: tocEl
  }, validation);
```

The rest of `updateContent`, starting with:

```js
  setupTocTracking();
```

must remain in the same order as before.

At the end of `updateContent`, replace:

```js
  if (!hasContractViolation && appContext.websocket) {
    appContext.websocket.rememberAppliedLiveUpdate(safeData);
  }
};
```

with:

```js
  if (!validation.hasContractViolation && appContext.websocket) {
    appContext.websocket.rememberAppliedLiveUpdate(safeData);
  }
};
```

- [ ] **Step 3: Run the contract and cache E2E tests**

Run:

```bash
npm run test:e2e -- tests/e2e/memo_jump.spec.ts
```

Expected: PASS, including:
- `updateContentはdata.content/toc欠落時に契約違反warnを出す`
- `data.contentが変わるとupdateContentは再描画される (cache invariantの逆方向)`
- `同じdata.contentでの2回目updateContentは.jump-highlightを消さない`

- [ ] **Step 4: Run exposure E2E again**

Run:

```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts
```

Expected: PASS. `updateContent` remains E2E-hook-only and renderer internals remain private to the IIFE.

- [ ] **Step 5: Run static `innerHTML` boundary check**

Run:

```bash
rg -n "innerHTML\\s*=" src/template/assets/js
```

Expected output includes `content-renderer.js` for `#content` / `#toc` assignments. Remaining matches are acceptable only for other DOM regions:

```text
src/template/assets/js/content-renderer.js:...:  contentEl.innerHTML = content;
src/template/assets/js/content-renderer.js:...:  tocEl.innerHTML = toc;
src/template/assets/js/content.js:...:  appContext.elements.documentSearchResultsEl.innerHTML = '';
src/template/assets/js/content.js:...:  appContext.elements.documentSearchResultsEl.innerHTML = '';
src/template/assets/js/memo.js:...:  appContext.elements.memoPreviewEl.innerHTML = data.html;
```

There must be no `contentEl.innerHTML =` or `tocEl.innerHTML =` match in `content.js`.

- [ ] **Step 6: Commit**

```bash
git add src/template/assets/js/content.js
git commit -m "refactor: updateContentのHTML反映をrenderer境界へ委譲"
```

## Task 4: Update TODO And Run Final Verification

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Update the browser JS TODO item**

In `docs/todo/TODO.md`, replace the browser JS Medium Priority item body with this text:

```markdown
- [ ] ブラウザ JS の責務境界を小モジュールへ分割する
  - ファイル: `src/template/assets/js/{bootstrap,content,content-renderer,fetch,memo,selection,sidebar,websocket}.js`, `src/template/assets/inline_script.rs`
  - 現状: `docs/superpowers/plans/2026-04-30-browser-js-deglobalization.md` の実行で production の `window` 露出は IIFE と `appContext` 集約により解消済み。E2E用内部操作も `window.__MV_E2E__ === true` 時の `markdownViewTestHooks` に限定した。さらに `content-renderer.js` で `updateContent` の payload 契約、契約違反 warn、`#content` / `#toc` への sanitize 済み HTML 反映、TOC HTML 正規化を明示境界へ切り出した。一方、`content.js` は検索、リンク解決、履歴、スクロール、引用ジャンプ、描画後副作用をまだまとめて扱う巨大ファイルのままで、controller API と依存境界は未整理
  - 対応: 次の分割単位を `document-search` / `directory-search`、`navigation` / `link-resolution`、`createContentController(ctx, deps)` の順で切る。`innerHTML` 使用箇所は引き続き信頼境界を明示し、検索やメモを削る、または純プレビューモードへ戻すことは非目標
  - 理由: 問題は「機能が多いこと」ではなく、workspace として成長した中核機能群の境界がブラウザ JS 内で十分に表現されていないこと。`content-renderer` により最重要の XSS 信頼境界は狭まったが、巨大ファイルと暗黙の `appContext` 依存が残ると将来の入力経路追加で状態遷移を壊しやすい
```

- [ ] **Step 2: Run formatting/lint/test verification**

Run:

```bash
./verify.sh
```

Expected: PASS. This covers Rust formatting, clippy, and Rust tests.

- [ ] **Step 3: Run targeted E2E verification**

Run:

```bash
npm run test:e2e -- tests/e2e/update_content_exposure.spec.ts tests/e2e/memo_jump.spec.ts
```

Expected: PASS.

- [ ] **Step 4: Run final static boundary check**

Run:

```bash
rg -n "innerHTML\\s*=" src/template/assets/js
```

Expected: `#content` and `#toc` assignment matches are only in `src/template/assets/js/content-renderer.js`. `documentSearchResultsEl.innerHTML` and `memoPreviewEl.innerHTML` may remain because they target different DOM regions.

- [ ] **Step 5: Commit**

```bash
git add docs/todo/TODO.md
git commit -m "docs: ブラウザJS分割TODOをcontent renderer後に整理"
```

## Completion Report Requirements

Final report must include:

- Changed files and rough line impact.
- Affected dependent files: `inline_script.rs` affects inline JS bundle and CSP hash generation; `content.js` affects live update, file fetch, WebSocket buffered update, memo jump, search sync, and TOC sync.
- Verification results for `./verify.sh`, targeted E2E, and `rg -n "innerHTML\\s*=" src/template/assets/js`.
- Security note: `innerHTML` remains intentional and is limited to server-sanitized HTML boundary for `#content` / `#toc`; memo preview remains a separate existing trust boundary.
- Residual risks: search/navigation/controller decomposition remains future work; E2E covers key update paths but does not exhaust every browser scroll/history edge case.
