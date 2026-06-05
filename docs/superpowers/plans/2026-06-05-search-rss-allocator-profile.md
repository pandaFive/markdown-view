# Search RSS Allocator Profile Implementation Plan

> **Status note:** この文書は 2026-06-05 時点の履歴・参考計画であり、現在の user instruction、`AGENTS.md`、runtime permission rules を上位として扱う。ここに含まれる実行手順、sub-skill 指示、`git add` / `git commit` 例は、再利用時にも都度の承認と現行ルール確認を前提にする。`develop` / `main` 上では停止し、通常ブランチ上で作業する。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the search RSS plateau measurement script so it can compare allocator profiles and record the result in `TODO.md` without changing production Rust behavior.

**Architecture:** Add an allocator profile axis to `scripts/measure-search-rss-plateau.mjs`, pass allowlisted environment variables only to the measured server process, and include profile metadata in each sanitized scenario report. Keep fixture generation, `/proc` sampling, HTTP contract validation, and sanitization in the existing script.

**Tech Stack:** Node.js ESM, Rust binary launched by Node, Linux `/proc`, existing `./verify.sh`.

---

## File Structure

- Modify: `scripts/measure-search-rss-plateau.mjs`
  - Owns CLI parsing, allocator profile definitions, measured server environment construction, scenario matrix execution, report sanitization, and self-tests.
- Modify: `docs/todo/TODO.md`
  - Record allocator profile measurement results, cause classification, residual risk, and preserved security boundaries.
- Confirm only: `src/server/files/search.rs`
  - Production search code must remain unchanged in this phase.
- Confirm only: `docs/superpowers/specs/2026-06-05-search-rss-allocator-profile-design.md`
  - Verify implementation matches the approved design.

## Task 1: Add Allocator Profile CLI Contract

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs`

- [ ] **Step 1: Add allocator profile constants**

Near the other top-level constants, add:

```js
const ALLOCATOR_PROFILES = new Map([
  ['default', {}],
  ['arena1', { MALLOC_ARENA_MAX: '1' }],
  ['arena2', { MALLOC_ARENA_MAX: '2' }],
]);
const DEFAULT_ALLOCATOR_PROFILES = ['default'];
```

- [ ] **Step 2: Add `allocatorProfiles` to default options**

In `parseArgs()`, initialize:

```js
allocatorProfiles: DEFAULT_ALLOCATOR_PROFILES,
```

Keep `--smoke` on `DEFAULT_ALLOCATOR_PROFILES`:

```js
options.allocatorProfiles = DEFAULT_ALLOCATOR_PROFILES;
```

- [ ] **Step 3: Parse `--allocator-profiles`**

Add this branch in the argument loop:

```js
} else if (arg === '--allocator-profiles') {
  explicitMatrixOptions.add(arg);
  options.allocatorProfiles = parseAllocatorProfiles(readValue(argv, ++index, arg));
```

Add the helper near `parseList()`:

```js
function parseAllocatorProfiles(raw) {
  const values = raw.split(',').map((value) => value.trim()).filter(Boolean);
  if (values.length === 0) {
    throw new Error('--allocator-profiles requires at least one value');
  }
  for (const value of values) {
    if (!ALLOCATOR_PROFILES.has(value)) {
      throw new Error(`--allocator-profiles must be one of: ${Array.from(ALLOCATOR_PROFILES.keys()).join(', ')}`);
    }
  }
  return values;
}
```

- [ ] **Step 4: Update smoke error message and help text**

Change the smoke conflict error to include allocator profiles:

```js
throw new Error('--smoke cannot be combined with --modes, --fixtures, --runs, --fixture-scale, or --allocator-profiles');
```

Update `printHelp()` with:

```text
  --allocator-profiles default,arena1,arena2
                                 Allocator profiles for measured server process. Default: default.
```

Also update the `--smoke` description line so it says the option cannot be combined with `--allocator-profiles`.

- [ ] **Step 5: Extend self-test for CLI parsing**

In `runSanitizationSelfTest()`, add `['--allocator-profiles', 'default']` to `matrixOptionPairs`.

Add these assertions:

```js
assert.deepEqual(parseArgs(['--allocator-profiles', 'default,arena1']).allocatorProfiles, ['default', 'arena1']);
assert.deepEqual(parseArgs(['--allocator-profiles', 'arena2']).allocatorProfiles, ['arena2']);
assert.throws(
  () => parseArgs(['--allocator-profiles', 'jemalloc']),
  /--allocator-profiles must be one of/
);
assert.throws(
  () => parseArgs(['--allocator-profiles', '']),
  /requires at least one value/
);
assert.deepEqual(parseArgs(['--smoke']).allocatorProfiles, ['default']);
```

- [ ] **Step 6: Run the self-test and confirm it passes**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
```

Expected:

```text
sanitization self-test: ok
```

- [ ] **Step 7: Commit Task 1**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "test: 検索RSS測定のallocator profile CLIを固定"
```

## Task 2: Pass Allowlisted Allocator Environment To Server

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs`

- [ ] **Step 1: Add profile lookup helpers**

Add these functions near the parse helpers:

```js
function allocatorProfileEnv(profileName) {
  const env = ALLOCATOR_PROFILES.get(profileName);
  if (!env) {
    throw new Error(`unsupported allocator profile: ${profileName}`);
  }
  return { ...env };
}

function allocatorProfileReport(profileName) {
  const env = allocatorProfileEnv(profileName);
  return {
    name: profileName,
    env,
  };
}
```

- [ ] **Step 2: Extend `runMeasuredScenario()` signature**

Change:

```js
async function runMeasuredScenario(options, fixture, mode, runKind) {
  const server = await startServer({ mode, workspace: fixture.workspace, port: options.port });
```

to:

```js
async function runMeasuredScenario(options, fixture, mode, runKind, allocatorProfile) {
  const allocator = allocatorProfileReport(allocatorProfile);
  const server = await startServer({
    mode,
    workspace: fixture.workspace,
    port: options.port,
    allocatorEnv: allocator.env,
  });
```

- [ ] **Step 3: Include allocator profile in successful scenario reports**

In the object returned by `runMeasuredScenario()`, add:

```js
allocatorProfile: allocator,
```

The returned object should include `mode`, `runKind`, and `allocatorProfile`.

- [ ] **Step 4: Pass allocator env into `spawn()`**

Change `startServer()` signature:

```js
async function startServer({ mode, workspace, port, allocatorEnv = {} }) {
```

Change `spawn()` options to:

```js
const child = spawn(binaryPath, [workspace, '--port', String(port), '--no-open'], {
  cwd: process.cwd(),
  env: buildMeasuredServerEnv(allocatorEnv),
  stdio: ['ignore', 'pipe', 'pipe'],
});
```

This passes only the measured-server env allowlist and the selected profile's allocator env. Do not spread or report `process.env`.

- [ ] **Step 5: Add self-test for env helper**

In `runSanitizationSelfTest()`, add:

```js
assert.deepEqual(allocatorProfileEnv('default'), {});
assert.deepEqual(allocatorProfileEnv('arena1'), { MALLOC_ARENA_MAX: '1' });
assert.deepEqual(allocatorProfileReport('arena2'), {
  name: 'arena2',
  env: { MALLOC_ARENA_MAX: '2' },
});
assert.throws(
  () => allocatorProfileEnv('unsupported'),
  /unsupported allocator profile/
);
```

- [ ] **Step 6: Run the self-test**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
```

Expected:

```text
sanitization self-test: ok
```

- [ ] **Step 7: Commit Task 2**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "feat: 検索RSS測定でallocator環境を切り替える"
```

## Task 3: Add Allocator Profile To Scenario Matrix And Context

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs`

- [ ] **Step 1: Add allocator loop in `runMeasurement()`**

Replace the inner run loop:

```js
for (const runKind of options.runs) {
```

with:

```js
for (const allocatorProfile of options.allocatorProfiles) {
  for (const runKind of options.runs) {
```

Close the extra loop after each run-kind block.

- [ ] **Step 2: Add allocator profile to `baseReport`**

Add:

```js
allocatorProfile: allocatorProfileReport(allocatorProfile),
```

to `baseReport`.

- [ ] **Step 3: Pass allocator profile to scenario runner**

Change:

```js
const scenario = await runMeasuredScenario(options, fixture, mode, runKind);
```

to:

```js
const scenario = await runMeasuredScenario(options, fixture, mode, runKind, allocatorProfile);
```

- [ ] **Step 4: Include allocator profiles in measurement context**

In `buildMeasurementContext(options).cliOptions`, add:

```js
allocatorProfiles: options.allocatorProfiles,
```

- [ ] **Step 5: Extend self-test for context**

In `runSanitizationSelfTest()`, add:

```js
const allocatorContext = buildMeasurementContext(parseArgs(['--allocator-profiles', 'default,arena1']));
assert.deepEqual(allocatorContext.cliOptions.allocatorProfiles, ['default', 'arena1']);
```

- [ ] **Step 6: Run help and self-test**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --help
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
```

Expected:

```text
Usage: node scripts/measure-search-rss-plateau.mjs [options]
sanitization self-test: ok
```

- [ ] **Step 7: Commit Task 3**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "feat: 検索RSS測定matrixにallocator profileを追加"
```

## Task 4: Run Measurement And Update TODO

**Files:**
- Modify: `docs/todo/TODO.md`
- Confirm only: `src/server/files/search.rs`

- [ ] **Step 1: Confirm production Rust is unchanged before measuring**

Run:

```bash
git diff -- src/server/files/search.rs
```

Expected: no output.

- [ ] **Step 2: Run short smoke measurement**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --smoke
```

Expected: sanitized JSON with one `reports` entry containing:

```json
"allocatorProfile": {
  "name": "default",
  "env": {}
}
```

If this fails with loopback bind or server startup restriction, rerun the same command with approved escalation and record the reason in the completion report.

- [ ] **Step 3: Run allocator comparison measurement**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs \
  --modes release \
  --fixtures prefix \
  --runs cold,warm \
  --fixture-scale full \
  --allocator-profiles default,arena1,arena2
```

Expected: sanitized JSON with one report per allocator profile and run kind. Each successful report must preserve:

```json
"truncated": true,
"truncated_reasons": ["result_limit"],
"resultsLength": 100
```

If the full matrix is too slow or environment-blocked, run this fallback:

```bash
node scripts/measure-search-rss-plateau.mjs \
  --modes release \
  --fixtures prefix \
  --runs cold \
  --fixture-scale full \
  --allocator-profiles default,arena1
```

- [ ] **Step 4: Summarize results without sensitive details**

Extract only these values from the sanitized JSON:

```text
allocator profile name
mode
run kind
elapsedMs
peakRssKb
settled.status.VmRSS
settled.status.RssAnon
settled.smapsRollup.Anonymous
response.searched_files
response.searched_bytes
response.truncated
response.truncated_reasons
response.resultsLength
partialMeasurementReasons
```

Do not copy exact temp paths, full process args, Markdown body fragments, or raw maps rows.

- [ ] **Step 5: Update `docs/todo/TODO.md`**

In the Medium item `ディレクトリ検索 many-match の RSS plateau を切り分ける`, append a new bullet after the existing `残件` bullet:

```markdown
  - 追加切り分け: 2026-06-05 に `scripts/measure-search-rss-plateau.mjs` へ allocator profile 比較を追加し、同一 prefix fixture を `default` / `arena1` / `arena2` で測定した。HTTP response は各 profile の実測値として `searched_files`、`searched_bytes`、`truncated=true`、`truncated_reasons=["result_limit"]`、`resultsLength=100` を維持した。settled `RssAnon` と `smaps_rollup Anonymous` は profile ごとの整数 KiB 値を転記し、測定不能な profile は `errorKind` と sanitized message を記録した。測定出力には実パス、full process args、本文断片、raw maps 行を含めていない。
  - 判断更新: `MALLOC_ARENA_MAX=1` で settled anonymous RSS が大きく下がったため、glibc allocator arena retained memory を主因候補として扱う。下がらなかった場合は、allocator 設定だけでは説明できず、WSL2 RSS/accounting 特性または prefix 経路の live allocation を残候補として扱う。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は弱めていない。
```

Before saving, replace the generic wording above with the exact measured integer KiB values from Step 4. If a profile could not be measured, state the failure kind instead of inventing a value.

- [ ] **Step 6: Commit Task 4**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: 検索RSS allocator切り分け結果を記録"
```

## Task 5: Final Verification

**Files:**
- Modify: none expected after verification fixes

- [ ] **Step 1: Run script self-test**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
```

Expected:

```text
sanitization self-test: ok
```

- [ ] **Step 2: Run help**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --help
```

Expected: output includes:

```text
--allocator-profiles default,arena1,arena2
```

- [ ] **Step 3: Run repository verification**

Run:

```bash
./verify.sh
```

Expected: format, lint, and tests pass.

- [ ] **Step 4: Confirm production Rust remained unchanged**

Run:

```bash
git diff HEAD -- src/server/files/search.rs
```

Expected: no output.

- [ ] **Step 5: Confirm no sensitive measurement details were written**

Run:

```bash
rg -n "/tmp/markdown-view-search-rss-plateau-|/home/|needle_inside|maps line|full process args" docs/todo/TODO.md scripts/measure-search-rss-plateau.mjs
```

Expected: no sensitive measurement result lines in `docs/todo/TODO.md`. Static sanitizer test strings in `scripts/measure-search-rss-plateau.mjs` are acceptable if they are part of self-test fixtures.

- [ ] **Step 6: Commit verification fixes if needed**

If verification required fixes, commit them:

```bash
git add scripts/measure-search-rss-plateau.mjs docs/todo/TODO.md
git commit -m "fix: 検索RSS allocator測定の検証不備を修正"
```

If no fixes were needed, do not create an empty commit.

## Self-Review Checklist

- Spec coverage: Tasks cover CLI profile parsing, allowlisted server env, matrix reports, `TODO.md` result recording, validation, and security constraints.
- Red-flag scan: The final `TODO.md` update must contain exact measured integers or explicit failure kinds, not generic profile-result wording.
- Type consistency: The plan uses `allocatorProfiles`, `allocatorProfile`, `allocatorProfileEnv()`, and `allocatorProfileReport()` consistently.
- Scope check: Production Rust is confirm-only; the plan does not add telemetry, UI changes, or API changes.
