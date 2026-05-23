# Inline Browser JS TypeScript Build Implementation Plan

> **Historical record:** This plan has been executed. Do not re-run the embedded commands, task steps, or commit instructions unless a new explicit user request reopens this work.
>
> **Original agentic workflow note:** For a fresh implementation of this plan, use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans as advisory workflow guidance. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the embedded browser JavaScript assets to TypeScript sources generated through Cargo build, without committing generated JavaScript.

**Architecture:** TypeScript files in `src/template/assets/ts/` become the source of truth. A new `build.rs` runs the existing npm toolchain, emits JavaScript into Cargo `OUT_DIR`, writes a Rust manifest of `include_str!` fragments, and `inline_script.rs` keeps the existing concatenation, sentinel replacement, and security scanner contracts.

**Tech Stack:** Rust 2021 build script, Cargo `OUT_DIR`, npm scripts, TypeScript compiler, browser DOM typings, existing tree-sitter JavaScript scanner tests, Playwright E2E.

---

## File Structure

- Create `build.rs`
  - Owns Cargo-to-npm bridge, `OUT_DIR/inline-js` generation, generated file validation, manifest generation, and `cargo:rerun-if-changed` output.
- Create `scripts/build-inline-js.mjs`
  - Owns Node-side validation of `MV_INLINE_JS_OUT_DIR` and invokes `tsc` with `--outDir`.
- Create `tsconfig.inline-js.json`
  - Browser-only TypeScript config for inline assets. Keep separate from E2E `tsconfig.json`.
- Create `src/template/assets/ts/*.ts`
  - TypeScript source equivalents of the current 12 browser JS files.
- Create `src/template/assets/ts/types.d.ts`
  - Shared browser runtime types for app context, payloads, responses, and test hooks.
- Delete `src/template/assets/js/*.js`
  - Generated JS is not committed.
- Modify `src/template/assets/inline_script.rs`
  - Replace direct `include_str!("js/*.js")` calls with generated manifest include while preserving `inline_js(max_file_size)`, sentinel replacement, and scanner tests.
- Modify `package.json`
  - Add `build:inline-js`, split E2E and inline typecheck scripts, keep integrated `typecheck`.
- Modify `verify.sh` only if script names require it
  - Current `npm run typecheck` can stay if `typecheck` remains the integrated script.
- Review `tests/e2e/globals.d.ts`
  - Keep E2E-facing hook types aligned with runtime hook shape.

## Task 1: Baseline Contract Checks

**Files:**
- Inspect: `src/template/assets/inline_script.rs`
- Inspect: `tests/e2e/update_content_exposure.spec.ts`
- Inspect: `tests/e2e/globals.d.ts`

- [ ] **Step 1: Run the current inline script contract tests**

Run:

```bash
cargo test --lib template::assets::inline_script
```

Expected: PASS. If this fails before changes, stop and fix the pre-existing failure separately.

- [ ] **Step 2: Run the current hook exposure E2E subset**

Run:

```bash
npm run typecheck
npx playwright test tests/e2e/update_content_exposure.spec.ts
```

Expected: PASS. This confirms production hook hiding and E2E hook exposure before migration.

- [ ] **Step 3: Record no code changes**

Run:

```bash
git status --short
```

Expected: Only existing docs branch commits are present; no new unstaged code changes from baseline checks.

## Task 2: Add TypeScript Build Configuration

**Files:**
- Create: `tsconfig.inline-js.json`
- Modify: `package.json`
- Create: `scripts/build-inline-js.mjs`

- [ ] **Step 1: Add browser TypeScript config**

Create `tsconfig.inline-js.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["DOM", "ES2022"],
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,
    "declaration": false,
    "sourceMap": false,
    "removeComments": false,
    "noEmitOnError": true
  },
  "include": ["src/template/assets/ts/**/*.ts"]
}
```

- [ ] **Step 2: Add npm scripts**

Modify `package.json` scripts to:

```json
{
  "scripts": {
    "test:e2e": "playwright test",
    "build:inline-js": "node scripts/build-inline-js.mjs",
    "typecheck:e2e": "tsc --noEmit",
    "typecheck:inline-js": "tsc -p tsconfig.inline-js.json --noEmit",
    "typecheck": "npm run typecheck:e2e && npm run typecheck:inline-js"
  }
}
```

Keep existing `engines` and `devDependencies` unchanged.

- [ ] **Step 3: Add Node build wrapper**

Create `scripts/build-inline-js.mjs`:

```javascript
import { spawnSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';

const outDir = process.env.MV_INLINE_JS_OUT_DIR;

if (!outDir) {
  console.error('MV_INLINE_JS_OUT_DIR is required.');
  process.exit(1);
}

mkdirSync(outDir, { recursive: true });

const result = spawnSync(
  process.platform === 'win32' ? 'npx.cmd' : 'npx',
  ['--no-install', 'tsc', '-p', 'tsconfig.inline-js.json', '--outDir', outDir],
  { stdio: 'inherit' }
);

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
```

- [ ] **Step 4: Verify missing env failure is explicit**

Run:

```bash
npm run build:inline-js
```

Expected: FAIL with `MV_INLINE_JS_OUT_DIR is required.`

- [ ] **Step 5: Commit config scaffold**

Run:

```bash
git add tsconfig.inline-js.json package.json scripts/build-inline-js.mjs
git commit -m "chore: インラインJSのTypeScriptビルド設定を追加"
```

## Task 3: Add Cargo Build Script

**Files:**
- Create: `build.rs`

- [ ] **Step 1: Add build script with fixed asset order**

Create `build.rs`:

```rust
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const INLINE_ASSETS: &[&str] = &[
    "bootstrap",
    "selection",
    "content-renderer",
    "content-enhancements",
    "content-navigation",
    "document-search",
    "directory-search",
    "content-controller",
    "memo",
    "fetch",
    "websocket",
    "sidebar",
];

fn main() {
    println!("cargo:rerun-if-changed=src/template/assets/ts");
    println!("cargo:rerun-if-changed=tsconfig.inline-js.json");
    println!("cargo:rerun-if-changed=package.json");
    println!("cargo:rerun-if-changed=package-lock.json");
    println!("cargo:rerun-if-changed=scripts/build-inline-js.mjs");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR should be set by Cargo"));
    let inline_js_dir = out_dir.join("inline-js");
    fs::create_dir_all(&inline_js_dir).expect("inline JS output directory should be created");

    run_inline_js_build(&inline_js_dir);
    write_manifest(&out_dir, &inline_js_dir);
}

fn run_inline_js_build(inline_js_dir: &Path) {
    let status = Command::new("npm")
        .args(["run", "build:inline-js"])
        .env("MV_INLINE_JS_OUT_DIR", inline_js_dir)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "failed to run npm. Install Node.js dependencies with `npm ci` before running Cargo: {error}"
            )
        });

    if !status.success() {
        panic!(
            "`npm run build:inline-js` failed. Run `npm ci` and then `npm run build:inline-js` for details."
        );
    }
}

fn write_manifest(out_dir: &Path, inline_js_dir: &Path) {
    let mut manifest = String::from("const GENERATED_TEMPLATE: &str = concat!(\n");

    for asset in INLINE_ASSETS {
        let path = inline_js_dir.join(format!("{asset}.js"));
        if !path.is_file() {
            panic!("inline JS asset was not generated: {}", path.display());
        }
        manifest.push_str("    include_str!(r#\"");
        manifest.push_str(&path.display().to_string());
        manifest.push_str("\"#),\n    \"\\n\",\n");
    }

    manifest.push_str(");\n");
    fs::write(out_dir.join("inline_script_manifest.rs"), manifest)
        .expect("inline script manifest should be written");
}
```

- [ ] **Step 2: Run Cargo build to confirm expected failure**

Run:

```bash
cargo test --lib template::assets::inline_script
```

Expected: FAIL because `src/template/assets/ts/**/*.ts` does not exist yet or `tsc` has no inputs. This failure proves `build.rs` is active.

- [ ] **Step 3: Commit build script**

Run:

```bash
git add build.rs
git commit -m "chore: CargoビルドでインラインJS生成を実行"
```

## Task 4: Move JS Sources to TypeScript Skeleton

**Files:**
- Create: `src/template/assets/ts/bootstrap.ts`
- Create: `src/template/assets/ts/selection.ts`
- Create: `src/template/assets/ts/content-renderer.ts`
- Create: `src/template/assets/ts/content-enhancements.ts`
- Create: `src/template/assets/ts/content-navigation.ts`
- Create: `src/template/assets/ts/document-search.ts`
- Create: `src/template/assets/ts/directory-search.ts`
- Create: `src/template/assets/ts/content-controller.ts`
- Create: `src/template/assets/ts/memo.ts`
- Create: `src/template/assets/ts/fetch.ts`
- Create: `src/template/assets/ts/websocket.ts`
- Create: `src/template/assets/ts/sidebar.ts`
- Create: `src/template/assets/ts/types.d.ts`

- [ ] **Step 1: Copy existing JS files to TS paths**

Run:

```bash
mkdir -p src/template/assets/ts
cp src/template/assets/js/bootstrap.js src/template/assets/ts/bootstrap.ts
cp src/template/assets/js/selection.js src/template/assets/ts/selection.ts
cp src/template/assets/js/content-renderer.js src/template/assets/ts/content-renderer.ts
cp src/template/assets/js/content-enhancements.js src/template/assets/ts/content-enhancements.ts
cp src/template/assets/js/content-navigation.js src/template/assets/ts/content-navigation.ts
cp src/template/assets/js/document-search.js src/template/assets/ts/document-search.ts
cp src/template/assets/js/directory-search.js src/template/assets/ts/directory-search.ts
cp src/template/assets/js/content-controller.js src/template/assets/ts/content-controller.ts
cp src/template/assets/js/memo.js src/template/assets/ts/memo.ts
cp src/template/assets/js/fetch.js src/template/assets/ts/fetch.ts
cp src/template/assets/js/websocket.js src/template/assets/ts/websocket.ts
cp src/template/assets/js/sidebar.js src/template/assets/ts/sidebar.ts
```

- [ ] **Step 2: Add shared declaration scaffold**

Create `src/template/assets/ts/types.d.ts`:

```typescript
interface Window {
  __MV_E2E__?: boolean;
  markdownViewTestHooks?: MarkdownViewTestHooks;
}

interface MarkdownViewTestHooks {
  [key: string]: unknown;
}
```

- [ ] **Step 3: Run inline typecheck to collect errors**

Run:

```bash
npm run typecheck:inline-js
```

Expected: FAIL with implicit `any`, nullability, and shared-global errors. Use this as the migration checklist.

- [ ] **Step 4: Commit skeleton**

Run:

```bash
git add src/template/assets/ts
git commit -m "refactor: インラインJSをTypeScriptソースへ複製"
```

## Task 5: Type Shared Runtime Contracts

**Files:**
- Modify: `src/template/assets/ts/types.d.ts`
- Reference: `tests/e2e/globals.d.ts`
- Reference: `src/server/files/search.rs`
- Reference: `src/template/message.rs`

- [ ] **Step 1: Replace declaration scaffold with runtime contracts**

Replace `src/template/assets/ts/types.d.ts` with:

```typescript
type MemoState = 'ready' | 'degraded';
type LiveStatusState = 'connected' | 'disconnected' | 'error';

interface SearchResult {
  file: string;
  line: number;
  snippet: string;
}

interface SearchResponse {
  results: SearchResult[];
  searched_files: number;
  searched_bytes: number;
  truncated: boolean;
  truncated_reasons: string[];
}

interface ContentUpdatePayload {
  content?: string;
  toc?: string;
  file?: string | null;
  refresh?: boolean;
  memo_refresh?: boolean;
}

interface MemoResponse {
  content: string;
  html: string;
  memo_state?: MemoState;
}

interface MarkdownViewElements {
  contentEl: HTMLElement | null;
  tocEl: HTMLElement | null;
  memoEditorEl: HTMLTextAreaElement | null;
  memoPreviewEl: HTMLElement | null;
  documentSearchInputEl: HTMLInputElement | null;
  documentSearchResultsEl: HTMLElement | null;
}

interface MarkdownViewAppContext {
  currentFile: string | null;
  isDirectoryMode: boolean;
  maxFileSizeMb: number;
  elements: MarkdownViewElements;
  sidebar: Record<string, unknown>;
  memo: Record<string, unknown>;
  search: Record<string, unknown>;
}

interface MarkdownViewTestHooks {
  [key: string]: unknown;
}

interface Window {
  __MV_E2E__?: boolean;
  markdownViewTestHooks?: MarkdownViewTestHooks;
}
```

- [ ] **Step 2: Run typecheck and keep errors focused**

Run:

```bash
npm run typecheck:inline-js
```

Expected: FAIL, but errors should now point to concrete function parameters and DOM nullability rather than missing top-level shared types.

- [ ] **Step 3: Commit shared types**

Run:

```bash
git add src/template/assets/ts/types.d.ts
git commit -m "refactor: インラインJSの共有型を定義"
```

## Task 6: Type Small Assets First

**Files:**
- Modify: `src/template/assets/ts/selection.ts`
- Modify: `src/template/assets/ts/content-renderer.ts`
- Modify: `src/template/assets/ts/bootstrap.ts`

- [ ] **Step 1: Type event and DOM boundaries in `selection.ts`**

Apply these patterns in `selection.ts`:

```typescript
document.addEventListener('mousedown', function(event: MouseEvent) {
  void event;
});

function getSelectionText(): string {
  return window.getSelection()?.toString() ?? '';
}
```

Keep existing behavior and names; only add parameter and return types and null-safe DOM handling.

- [ ] **Step 2: Type sanitized HTML renderer boundaries**

In `content-renderer.ts`, keep the existing allowed sinks and add explicit string parameters:

```typescript
function normalizeTocHtml(html: string): string {
  return html.trim();
}

function applyRenderedContent(contentEl: HTMLElement, tocEl: HTMLElement, content: string, toc: string): void {
  contentEl.innerHTML = content;
  if (normalizeTocHtml(tocEl.innerHTML) === normalizeTocHtml(toc)) {
    return;
  }
  tocEl.innerHTML = toc;
}
```

Use the existing local function names if they differ; do not add new `innerHTML` write sites.

- [ ] **Step 3: Type app context creation in `bootstrap.ts`**

Ensure the app context object is typed:

```typescript
function createAppContext(): MarkdownViewAppContext {
  return {
    currentFile: null,
    isDirectoryMode: false,
    maxFileSizeMb: __MAX_FILE_SIZE_MB__,
    elements: {
      contentEl: document.getElementById('content'),
      tocEl: document.getElementById('toc'),
      memoEditorEl: document.getElementById('memo-editor') as HTMLTextAreaElement | null,
      memoPreviewEl: document.getElementById('memo-preview'),
      documentSearchInputEl: document.getElementById('document-search-input') as HTMLInputElement | null,
      documentSearchResultsEl: document.getElementById('document-search-results')
    },
    sidebar: {},
    memo: {},
    search: {}
  };
}
```

If the existing context has more fields, type them in `MarkdownViewAppContext` rather than deleting them.

- [ ] **Step 4: Run focused typecheck**

Run:

```bash
npm run typecheck:inline-js
```

Expected: FAIL remains until larger files are typed, but these three files should no longer produce errors.

- [ ] **Step 5: Commit small typed assets**

Run:

```bash
git add src/template/assets/ts/selection.ts src/template/assets/ts/content-renderer.ts src/template/assets/ts/bootstrap.ts src/template/assets/ts/types.d.ts
git commit -m "refactor: 小規模インラインJSに型を追加"
```

## Task 7: Type Content, Search, Memo, WebSocket, and Sidebar Assets

**Files:**
- Modify: `src/template/assets/ts/content-enhancements.ts`
- Modify: `src/template/assets/ts/content-navigation.ts`
- Modify: `src/template/assets/ts/document-search.ts`
- Modify: `src/template/assets/ts/directory-search.ts`
- Modify: `src/template/assets/ts/content-controller.ts`
- Modify: `src/template/assets/ts/memo.ts`
- Modify: `src/template/assets/ts/fetch.ts`
- Modify: `src/template/assets/ts/websocket.ts`
- Modify: `src/template/assets/ts/sidebar.ts`
- Modify: `src/template/assets/ts/types.d.ts`

- [ ] **Step 1: Type shared helper signatures**

For each top-level function, add parameter and return types using existing behavior. Use these patterns:

```typescript
function requireElement(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (!element) {
    throw new Error(`Required element is missing: ${id}`);
  }
  return element;
}

function optionalInput(id: string): HTMLInputElement | null {
  return document.getElementById(id) as HTMLInputElement | null;
}
```

Only introduce helpers where the current file already assumes the element is mandatory. Keep optional UI paths optional.

- [ ] **Step 2: Type fetch responses**

Use explicit guards before consuming JSON:

```typescript
async function readJsonResponse<T>(response: Response): Promise<T> {
  return (await response.json()) as T;
}
```

Apply `SearchResponse`, `MemoResponse`, and `ContentUpdatePayload` to the current fetch/update call sites.

- [ ] **Step 3: Type WebSocket messages conservatively**

Use `unknown` at parse boundaries, then narrow:

```typescript
function parseWsMessage(raw: string): ContentUpdatePayload | null {
  const parsed: unknown = JSON.parse(raw);
  if (!parsed || typeof parsed !== 'object') {
    return null;
  }
  return parsed as ContentUpdatePayload;
}
```

Do not trust network payloads because TypeScript assertions are not runtime validation.

- [ ] **Step 4: Type E2E hooks without broadening exposure**

Keep the existing guard:

```typescript
if (window.__MV_E2E__ !== true) return;
window.markdownViewTestHooks = {
  // existing hook properties only
};
```

Update `MarkdownViewTestHooks` with the actual hook names present in `sidebar.ts` and other files. Do not expose hooks outside this guard.

- [ ] **Step 5: Run inline typecheck to completion**

Run:

```bash
npm run typecheck:inline-js
```

Expected: PASS.

- [ ] **Step 6: Commit full TypeScript typing**

Run:

```bash
git add src/template/assets/ts
git commit -m "refactor: インラインブラウザJSをTypeScript化"
```

## Task 8: Switch Rust Inline Script to Generated Manifest

**Files:**
- Modify: `src/template/assets/inline_script.rs`

- [ ] **Step 1: Replace static template source**

Change the top of `src/template/assets/inline_script.rs` from direct `include_str!("js/*.js")` concat to:

```rust
include!(concat!(env!("OUT_DIR"), "/inline_script_manifest.rs"));

const TEMPLATE: &str = concat!(
    "(function() {\n",
    GENERATED_TEMPLATE,
    "startMarkdownViewApp();\n",
    "}());\n",
);
```

Keep `inline_js(max_file_size)`, `file_size_display_mb`, and all scanner tests in the same file.

- [ ] **Step 2: Run focused Rust test**

Run:

```bash
cargo test --lib template::assets::inline_script
```

Expected: PASS. If sink allowlist assertions fail only because generated JS formatting changed, update the expected source strings to the generated JS while keeping the same allowed sink meanings.

- [ ] **Step 3: Commit Rust include switch**

Run:

```bash
git add src/template/assets/inline_script.rs
git commit -m "refactor: 生成済みインラインJSをRustへ埋め込む"
```

## Task 9: Remove Committed JavaScript Assets

**Files:**
- Delete: `src/template/assets/js/bootstrap.js`
- Delete: `src/template/assets/js/selection.js`
- Delete: `src/template/assets/js/content-renderer.js`
- Delete: `src/template/assets/js/content-enhancements.js`
- Delete: `src/template/assets/js/content-navigation.js`
- Delete: `src/template/assets/js/document-search.js`
- Delete: `src/template/assets/js/directory-search.js`
- Delete: `src/template/assets/js/content-controller.js`
- Delete: `src/template/assets/js/memo.js`
- Delete: `src/template/assets/js/fetch.js`
- Delete: `src/template/assets/js/websocket.js`
- Delete: `src/template/assets/js/sidebar.js`

- [ ] **Step 1: Delete old JS files**

Run:

```bash
git rm src/template/assets/js/bootstrap.js \
  src/template/assets/js/selection.js \
  src/template/assets/js/content-renderer.js \
  src/template/assets/js/content-enhancements.js \
  src/template/assets/js/content-navigation.js \
  src/template/assets/js/document-search.js \
  src/template/assets/js/directory-search.js \
  src/template/assets/js/content-controller.js \
  src/template/assets/js/memo.js \
  src/template/assets/js/fetch.js \
  src/template/assets/js/websocket.js \
  src/template/assets/js/sidebar.js
```

- [ ] **Step 2: Verify no Rust source includes old path**

Run:

```bash
rg -n 'assets/js|include_str!\("js/' src tests docs
```

Expected: No production references to `src/template/assets/js/*.js`. Historical docs may mention the old path; update only active docs if they now contradict the build.

- [ ] **Step 3: Commit deletion**

Run:

```bash
git add src/template/assets/js
git commit -m "refactor: 生成対象の旧JavaScript資産を削除"
```

## Task 10: Verification and Documentation Updates

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Modify if needed: `README.md`
- Modify if needed: `AGENTS.md` is not expected

- [ ] **Step 1: Run full typecheck and build path**

Run:

```bash
inline_js_out_dir="$(mktemp -d)"
MV_INLINE_JS_OUT_DIR="$inline_js_out_dir" npm run build:inline-js
npm run typecheck
cargo test --lib template::assets::inline_script
npx playwright test tests/e2e/update_content_exposure.spec.ts
```

Expected: PASS.

- [ ] **Step 2: Run required repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 3: Run E2E verification**

Run:

```bash
./verify.sh --e2e
```

Expected: PASS. If browser dependencies are missing, install them through the project’s existing Playwright setup and rerun, or report the missing system dependency as residual risk.

- [ ] **Step 4: Ensure generated JS is not tracked**

Run:

```bash
git status --short
rg -n 'src/template/assets/js/.*\.js' .gitignore docs/todo/BACKLOG.md docs/superpowers/specs docs/superpowers/plans
```

Expected: No generated `.js` under `src/template/assets/js/` is present. `OUT_DIR` generated files should not appear in `git status`.

- [ ] **Step 5: Update BACKLOG completion entry**

Move the BACKLOG item `インラインブラウザJS の TS 化` to Done with this wording:

```markdown
- [x] インラインブラウザJS の TS 化
  - 完了根拠: `src/template/assets/ts/*.ts` を正ソースにし、`build.rs` が `MV_INLINE_JS_OUT_DIR` 付きの `npm run build:inline-js` 経由で Cargo `OUT_DIR` 配下へ生成した JS を `inline_script.rs` へ埋め込む構成にした。生成 `.js` はリポジトリに保持せず、`npm run typecheck` で E2E とインライン JS の両方を検査する。既存の結合順序、`__MAX_FILE_SIZE_MB__` sentinel 置換、CSP hash、`innerHTML` sink allowlist、E2E hook production 非公開契約は維持し、`MV_INLINE_JS_OUT_DIR="$(mktemp -d)" npm run build:inline-js`、`./verify.sh`、`./verify.sh --e2e` で確認した
```

- [ ] **Step 6: Commit docs update**

Run:

```bash
git add docs/todo/BACKLOG.md README.md
git commit -m "docs: インラインJS TypeScript化の完了根拠を記録"
```

If `README.md` is unchanged, omit it from `git add`.

## Final Review Checklist

Status note: This plan has been executed. The checklist below records the completed final review state; `docs/todo/BACKLOG.md` is the source of truth for the Done entry.

- [x] `src/template/assets/ts/*.ts` exists and is the source of truth.
- [x] `src/template/assets/js/*.js` no longer exists.
- [x] No generated `.js` is tracked.
- [x] `build.rs` invokes only the fixed npm script via `Command`.
- [x] `MV_INLINE_JS_OUT_DIR` points under Cargo `OUT_DIR`.
- [x] `inline_script.rs` still wraps assets in the IIFE and calls `startMarkdownViewApp();`.
- [x] `innerHTML` allowlist did not gain new sinks.
- [x] `window.markdownViewTestHooks` remains guarded by `window.__MV_E2E__ === true`.
- [x] `npm run typecheck` passes.
- [x] `./verify.sh` passes.
- [x] `./verify.sh --e2e` passes or residual risk is reported with the exact failure.

## Self-Review Notes

- Spec coverage: Build script, TypeScript source of truth, no generated JS commit, strict typecheck, security boundaries, tests, rollback, and BACKLOG completion are covered by Tasks 2-10.
- Placeholder scan: This plan intentionally avoids open-ended placeholders; each task has concrete files, commands, and expected outcomes.
- Type consistency: Shared runtime names are introduced in Task 5 before being used in later TypeScript tasks.
