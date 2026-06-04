# Search RSS Plateau Implementation Plan

> **Status note:** この文書は 2026-06-04 時点の履歴・参考計画であり、現在の user instruction、`AGENTS.md`、runtime permission rules を上位として扱う。ここに含まれる実行手順、sub-skill 指示、`git add` / `git commit` 例は、再利用時にも都度の承認と現行ルール確認を前提にする。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ディレクトリ検索 many-match 後の RSS plateau を再現可能に測定し、支配要因を分類して `TODO.md` に結果を残す。

**Architecture:** `scripts/measure-search-rss-plateau.mjs` を追加し、Node.js 標準ライブラリだけで fixture 作成、server 起動、HTTP 検索、`/proc` 集計、出力 sanitization を行う。Rust production code、検索 API、UI、TypeScript、generated JS は変更しない。

**Tech Stack:** Node.js ESM, Rust/Cargo CLI, localhost HTTP, Linux `/proc`, existing `./verify.sh`.

---

## Preconditions

- Current branch: `docs/search-rss-plateau-design` or a feature/fix branch, not `develop` / `main`.
- Approved spec: `docs/superpowers/specs/2026-06-04-search-rss-plateau-design.md`.
- Keep fixtures under `/tmp/markdown-view-search-rss-plateau.***`; never commit generated fixtures.
- Do not add npm dependencies.
- Treat fixture Markdown, HTTP response, and `/proc` text as untrusted input.

## File Structure

- Create: `scripts/measure-search-rss-plateau.mjs`
  - Owns CLI parsing, fixture generation, server lifecycle, HTTP measurement, `/proc` parsing, sanitized report output, and self-tests.
- Modify: `docs/todo/TODO.md`
  - Add measurement results, cause classification, residual risk, and next TODO candidates to the existing Medium item.
- Confirm only: `src/server/files/search.rs`
  - No production changes expected. Read only if measurement results need code-path interpretation.
- Confirm only: `package.json`
  - No script entry required. Run the measurement directly with `node scripts/measure-search-rss-plateau.mjs`.

## Acceptance Criteria

- `node scripts/measure-search-rss-plateau.mjs --help` succeeds.
- `node scripts/measure-search-rss-plateau.mjs --self-test-sanitization` succeeds.
- A short smoke measurement runs and prints sanitized JSON.
- Full or partial matrix records dev/release, cold/warm, single/multi, prefix/fallback coverage.
- Output does not contain exact temp paths, full process args, Markdown body fragments, or raw `maps` rows.
- `docs/todo/TODO.md` records the results without exact temp paths or sensitive process details.
- `./verify.sh` passes, or any failure is reported with residual risk.

## Task 1: CLI Skeleton And Sanitization Contract

**Files:**
- Create: `scripts/measure-search-rss-plateau.mjs`

- [ ] **Step 1: Create a minimal script with help and sanitization self-test**

Add `scripts/measure-search-rss-plateau.mjs`:

```js
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const DEFAULT_QUERY = 'needle';
const TEMP_PREFIX = 'markdown-view-search-rss-plateau-';
const MASKED_TEMP = '/tmp/markdown-view-search-rss-plateau.***';
const DEFAULT_PORT = 3109;

function main(argv = process.argv.slice(2)) {
  const options = parseArgs(argv);
  if (options.help) {
    printHelp();
    return Promise.resolve(0);
  }
  if (options.selfTestSanitization) {
    runSanitizationSelfTest();
    console.log('sanitization self-test: ok');
    return Promise.resolve(0);
  }
  return runMeasurement(options);
}

function parseArgs(argv) {
  const options = {
    help: false,
    selfTestSanitization: false,
    smoke: false,
    keepTemp: false,
    port: DEFAULT_PORT,
    query: DEFAULT_QUERY,
    modes: ['dev'],
    fixtures: ['prefix'],
    runs: ['cold'],
    fixtureScale: 'short',
    output: 'json',
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--help' || arg === '-h') {
      options.help = true;
    } else if (arg === '--self-test-sanitization') {
      options.selfTestSanitization = true;
    } else if (arg === '--smoke') {
      options.smoke = true;
      options.fixtureScale = 'short';
      options.modes = ['dev'];
      options.fixtures = ['prefix'];
      options.runs = ['cold'];
    } else if (arg === '--keep-temp') {
      options.keepTemp = true;
    } else if (arg === '--port') {
      options.port = parsePositiveInt(readValue(argv, ++index, arg), arg);
    } else if (arg === '--query') {
      options.query = readValue(argv, ++index, arg);
    } else if (arg === '--modes') {
      options.modes = parseList(readValue(argv, ++index, arg), ['dev', 'release'], arg);
    } else if (arg === '--fixtures') {
      options.fixtures = parseList(readValue(argv, ++index, arg), ['prefix', 'multifile', 'fallback'], arg);
    } else if (arg === '--runs') {
      options.runs = parseList(readValue(argv, ++index, arg), ['cold', 'warm'], arg);
    } else if (arg === '--fixture-scale') {
      options.fixtureScale = parseChoice(readValue(argv, ++index, arg), ['short', 'full'], arg);
    } else if (arg === '--output') {
      options.output = parseChoice(readValue(argv, ++index, arg), ['json'], arg);
    } else {
      throw new Error(`unknown option: ${arg}`);
    }
  }

  return options;
}

function readValue(argv, index, optionName) {
  if (index >= argv.length || argv[index].startsWith('--')) {
    throw new Error(`${optionName} requires a value`);
  }
  return argv[index];
}

function parsePositiveInt(raw, optionName) {
  const value = Number.parseInt(raw, 10);
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${optionName} must be a positive integer`);
  }
  return value;
}

function parseList(raw, allowed, optionName) {
  const values = raw.split(',').map((value) => value.trim()).filter(Boolean);
  if (values.length === 0) {
    throw new Error(`${optionName} requires at least one value`);
  }
  for (const value of values) {
    parseChoice(value, allowed, optionName);
  }
  return values;
}

function parseChoice(value, allowed, optionName) {
  if (!allowed.includes(value)) {
    throw new Error(`${optionName} must be one of: ${allowed.join(', ')}`);
  }
  return value;
}

function printHelp() {
  console.log(`Usage: node scripts/measure-search-rss-plateau.mjs [options]

Options:
  --help                         Show this help.
  --self-test-sanitization       Run local sanitization checks without starting the server.
  --smoke                        Run a short dev/prefix/cold measurement.
  --modes dev,release            Build modes to measure. Default: dev.
  --fixtures prefix,multifile,fallback
                                 Fixture kinds to measure. Default: prefix.
  --runs cold,warm               Run kinds to measure. Default: cold.
  --fixture-scale short,full     Fixture size. Default: short.
  --port <number>                Local port. Default: ${DEFAULT_PORT}.
  --query <query>                Search query. Default: ${DEFAULT_QUERY}.
  --keep-temp                    Keep temp fixture directory for local debugging.
  --output json                  Output sanitized JSON. Default: json.
`);
}

function sanitizePath(value) {
  if (typeof value !== 'string') {
    return value;
  }
  return value
    .replaceAll(process.cwd(), '<repo>')
    .replace(/\/tmp\/markdown-view-search-rss-plateau[.\-][^/\s"']+/g, MASKED_TEMP)
    .replace(/\/home\/[^\s"']+/g, '<home-path>');
}

function sanitizeReport(value) {
  if (Array.isArray(value)) {
    return value.map(sanitizeReport);
  }
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, nested]) => [key, sanitizeReport(nested)])
    );
  }
  return sanitizePath(value);
}

function runSanitizationSelfTest() {
  const raw = {
    temp: '/tmp/markdown-view-search-rss-plateau.abcd1234/workspace/prefix.md',
    repo: `${process.cwd()}/target/debug/markdown-view`,
    home: '/home/example/secret/file.md',
    body: 'needle paragraph body should not be passed into reports',
  };
  const sanitized = sanitizeReport(raw);
  assert.equal(sanitized.temp, `${MASKED_TEMP}/workspace/prefix.md`);
  assert.equal(sanitized.repo, '<repo>/target/debug/markdown-view');
  assert.equal(sanitized.home, '<home-path>');
  assert.equal(sanitized.body, 'needle paragraph body should not be passed into reports');
}

async function runMeasurement(_options) {
  throw new Error('run with --help or --self-test-sanitization until measurement support is added');
}

main().then((code) => {
  process.exitCode = code;
}).catch((error) => {
  console.error(sanitizePath(error.message));
  process.exitCode = 1;
});
```

- [ ] **Step 2: Run help**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --help
```

Expected: exit code `0`; output starts with `Usage: node scripts/measure-search-rss-plateau.mjs`.

- [ ] **Step 3: Run sanitization self-test**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
```

Expected: exit code `0`; output is `sanitization self-test: ok`.

- [ ] **Step 4: Commit**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "chore: 検索RSS計測スクリプトのCLIを追加"
```

Expected: commit succeeds.

## Task 2: Fixture Generation

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs`

- [ ] **Step 1: Add fixture generation before `runMeasurement()`**

Insert these functions before `runMeasurement()`:

```js
function createFixtureRoot() {
  return mkdtempSync(path.join(tmpdir(), TEMP_PREFIX));
}

function createFixture(root, fixtureKind, scale) {
  const workspace = path.join(root, `${fixtureKind}-workspace`);
  mkdirSync(workspace, { recursive: true });

  if (fixtureKind === 'prefix') {
    return createPrefixFixture(workspace, scale);
  }
  if (fixtureKind === 'multifile') {
    return createMultifileFixture(workspace, scale);
  }
  if (fixtureKind === 'fallback') {
    return createFallbackFixture(workspace, scale);
  }
  throw new Error(`unsupported fixture kind: ${fixtureKind}`);
}

function createPrefixFixture(workspace, scale) {
  const repeatCount = scale === 'full' ? 180_000 : 1_200;
  const filePath = path.join(workspace, 'prefix.md');
  const paragraphs = [];
  for (let index = 0; index < repeatCount; index += 1) {
    paragraphs.push(`needle paragraph ${index}`);
  }
  writeFileSync(filePath, `${paragraphs.join('\n\n')}\n`, 'utf8');
  return summarizeFixture(workspace, 'prefix');
}

function createMultifileFixture(workspace, scale) {
  const fileCount = scale === 'full' ? 120 : 8;
  const repeatCount = scale === 'full' ? 240 : 16;
  for (let fileIndex = 0; fileIndex < fileCount; fileIndex += 1) {
    const filePath = path.join(workspace, `multi-${String(fileIndex).padStart(3, '0')}.md`);
    const lines = [];
    for (let lineIndex = 0; lineIndex < repeatCount; lineIndex += 1) {
      lines.push(`needle multi ${fileIndex} ${lineIndex}`);
    }
    writeFileSync(filePath, `${lines.join('\n\n')}\n`, 'utf8');
  }
  return summarizeFixture(workspace, 'multifile');
}

function createFallbackFixture(workspace, scale) {
  const repeatCount = scale === 'full' ? 300_000 : 4_000;
  const filePath = path.join(workspace, 'fallback.md');
  const chunk = 'needle_inside_single_large_block ';
  writeFileSync(filePath, `# fallback\n\n${chunk.repeat(repeatCount)}\n`, 'utf8');
  return summarizeFixture(workspace, 'fallback');
}

function summarizeFixture(workspace, fixtureKind) {
  const files = collectMarkdownFiles(workspace);
  const bytes = files.reduce((sum, filePath) => sum + statSync(filePath).size, 0);
  return {
    fixtureKind,
    workspace,
    fileCount: files.length,
    bytes,
    maskedWorkspace: sanitizePath(workspace),
  };
}

function collectMarkdownFiles(directory) {
  const entries = [];
  for (const entry of readdirRecursive(directory)) {
    if (entry.endsWith('.md')) {
      entries.push(entry);
    }
  }
  return entries.sort();
}

function readdirRecursive(directory) {
  const output = [];
  for (const entryName of readFileNames(directory)) {
    const entryPath = path.join(directory, entryName);
    const stat = statSync(entryPath);
    if (stat.isDirectory()) {
      output.push(...readdirRecursive(entryPath));
    } else if (stat.isFile()) {
      output.push(entryPath);
    }
  }
  return output;
}

function readFileNames(directory) {
  return readdirSync(directory).sort();
}
```

- [ ] **Step 2: Wire fixture creation into `runMeasurement()`**

Replace `runMeasurement()` with:

```js
async function runMeasurement(options) {
  const root = createFixtureRoot();
  try {
    const reports = [];
    for (const fixtureKind of options.fixtures) {
      const fixture = createFixture(root, fixtureKind, options.fixtureScale);
      reports.push({
        fixtureKind,
        fixture: {
          fileCount: fixture.fileCount,
          bytes: fixture.bytes,
          workspace: fixture.maskedWorkspace,
        },
        status: 'fixture-created',
      });
    }
    console.log(JSON.stringify(sanitizeReport({ tempRoot: root, reports }), null, 2));
    return 0;
  } finally {
    if (!options.keepTemp) {
      rmSync(root, { recursive: true, force: true });
    }
  }
}
```

- [ ] **Step 3: Run current self-test**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
```

Expected: exit code `0`; output is `sanitization self-test: ok`. The current CLI no longer has a fixture-only mode; fixture generation is covered by this self-test.

- [ ] **Step 4: Run current HTTP smoke and check exact temp paths are masked**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --smoke --port 3120 > /tmp/markdown-view-search-rss-plateau-smoke.json
rg '/tmp/markdown-view-search-rss-plateau[.-][A-Za-z0-9_-]+|needle paragraph|needle_inside_single_large_block|querySha256|queryLength' /tmp/markdown-view-search-rss-plateau-smoke.json
```

Expected: `node` exits `0`; `rg` exits `1` because exact temp names, body fragments, and query fingerprints are not printed.

- [ ] **Step 5: Commit**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "chore: 検索RSS計測fixture生成を追加"
```

Expected: commit succeeds.

## Task 3: `/proc` Status And Map Summary

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs`

- [ ] **Step 1: Add `/proc` parsers before `runMeasurement()`**

Insert:

```js
function readProcSnapshot(pid) {
  return {
    status: readProcStatus(pid),
    smapsRollup: readSmapsRollup(pid),
    maps: readMapsSummary(pid),
  };
}

function readProcStatus(pid) {
  const filePath = `/proc/${pid}/status`;
  if (!existsSync(filePath)) {
    return { available: false, reason: 'missing' };
  }
  const parsed = {};
  for (const line of readFileSync(filePath, 'utf8').split('\n')) {
    const match = /^(VmRSS|RssAnon|RssFile|RssShmem):\s+(\d+)\s+kB$/.exec(line);
    if (match) {
      parsed[match[1]] = Number.parseInt(match[2], 10);
    }
  }
  return { available: true, ...parsed };
}

function readSmapsRollup(pid) {
  const filePath = `/proc/${pid}/smaps_rollup`;
  if (!existsSync(filePath)) {
    return { available: false, reason: 'missing' };
  }
  const parsed = {};
  for (const line of readFileSync(filePath, 'utf8').split('\n')) {
    const match = /^(Rss|Pss|Private_Clean|Private_Dirty|Shared_Clean|Shared_Dirty|Anonymous):\s+(\d+)\s+kB$/.exec(line);
    if (match) {
      parsed[match[1]] = Number.parseInt(match[2], 10);
    }
  }
  return { available: true, ...parsed };
}

function readMapsSummary(pid) {
  const filePath = `/proc/${pid}/maps`;
  if (!existsSync(filePath)) {
    return { available: false, reason: 'missing' };
  }
  const summary = {
    available: true,
    anonymousKb: 0,
    fileBackedKb: 0,
    heapKb: 0,
    stackKb: 0,
    otherSpecialKb: 0,
    mappingCount: 0,
  };
  for (const line of readFileSync(filePath, 'utf8').split('\n')) {
    if (line.trim() === '') {
      continue;
    }
    const entry = parseMapsLine(line);
    if (!entry) {
      continue;
    }
    summary.mappingCount += 1;
    if (entry.name === '[heap]') {
      summary.heapKb += entry.sizeKb;
    } else if (entry.name.startsWith('[stack')) {
      summary.stackKb += entry.sizeKb;
    } else if (entry.name.startsWith('[')) {
      summary.otherSpecialKb += entry.sizeKb;
    } else if (entry.name === '') {
      summary.anonymousKb += entry.sizeKb;
    } else {
      summary.fileBackedKb += entry.sizeKb;
    }
  }
  return summary;
}

function parseMapsLine(line) {
  const match = /^([0-9a-f]+)-([0-9a-f]+)\s+\S+\s+\S+\s+\S+\s+\S+\s*(.*)$/.exec(line);
  if (!match) {
    return null;
  }
  const start = Number.parseInt(match[1], 16);
  const end = Number.parseInt(match[2], 16);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start) {
    return null;
  }
  return {
    sizeKb: Math.round((end - start) / 1024),
    name: match[3].trim(),
  };
}
```

- [ ] **Step 2: Add parser self-test cases to `runSanitizationSelfTest()`**

Append inside `runSanitizationSelfTest()`:

```js
  const anonymous = parseMapsLine('7f0000000000-7f0000100000 rw-p 00000000 00:00 0');
  const fileBacked = parseMapsLine('7f0000200000-7f0000300000 r--p 00000000 08:01 123 /usr/lib/libc.so');
  assert.equal(anonymous.sizeKb, 1024);
  assert.equal(anonymous.name, '');
  assert.equal(fileBacked.sizeKb, 1024);
  assert.equal(fileBacked.name, '/usr/lib/libc.so');
```

- [ ] **Step 3: Run self-test**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
```

Expected: `sanitization self-test: ok`.

- [ ] **Step 4: Commit**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "chore: 検索RSS計測のproc集計を追加"
```

Expected: commit succeeds.

## Task 4: Server Lifecycle And HTTP Measurement

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs`

- [ ] **Step 1: Add server and HTTP helpers before `runMeasurement()`**

Insert:

```js
async function runMeasuredScenario(options, fixture, mode, runKind) {
  const server = await startServer({ mode, workspace: fixture.workspace, port: options.port });
  try {
    await waitForServer(options.port);
    const before = readProcSnapshot(server.pid);
    const startedAt = process.hrtime.bigint();
    const requestPromise = requestSearch(options.port, options.query);
    const samples = [];
    while (!isPromiseSettled(requestPromise)) {
      samples.push(readProcSnapshot(server.pid));
      await delay(50);
    }
    const response = await requestPromise;
    const endedAt = process.hrtime.bigint();
    const after = readProcSnapshot(server.pid);
    await delay(5000);
    const settled = readProcSnapshot(server.pid);
    return {
      mode,
      runKind,
      pid: server.pid,
      elapsedMs: Number(endedAt - startedAt) / 1_000_000,
      peakRssKb: peakRssKb([before, ...samples, after, settled]),
      before,
      after,
      settled,
      response,
    };
  } finally {
    await stopServer(server);
  }
}

function isPromiseSettled(promise) {
  return promise.settled === true;
}

function trackPromise(promise) {
  promise.settled = false;
  promise.then(
    () => { promise.settled = true; },
    () => { promise.settled = true; }
  );
  return promise;
}

async function startServer({ mode, workspace, port }) {
  const args = mode === 'release'
    ? ['run', '--release', '--', workspace, '--port', String(port)]
    : ['run', '--', workspace, '--port', String(port)];
  const child = spawn('cargo', args, {
    cwd: process.cwd(),
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const stderrChunks = [];
  child.stderr.on('data', (chunk) => {
    stderrChunks.push(chunk.toString('utf8'));
  });
  child.stdout.resume();
  child.stderr.resume();
  await delay(250);
  if (child.exitCode !== null) {
    throw new Error(`server exited early with code ${child.exitCode}: ${sanitizePath(stderrChunks.join('').slice(0, 500))}`);
  }
  return child;
}

async function waitForServer(port) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/`, {
        headers: { Host: `127.0.0.1:${port}` },
      });
      if (response.status < 500) {
        return;
      }
    } catch (_error) {
      await delay(200);
    }
  }
  throw new Error('server did not become ready');
}

async function requestSearch(port, query) {
  return trackPromise((async () => {
    const response = await fetch(`http://127.0.0.1:${port}/api/search?q=${encodeURIComponent(query)}`, {
      headers: { Host: `127.0.0.1:${port}` },
    });
    const body = await response.text();
    let json = null;
    try {
      json = JSON.parse(body);
    } catch (_error) {
      json = null;
    }
    return {
      httpStatus: response.status,
      responseBytes: Buffer.byteLength(body),
      searched_files: json?.searched_files,
      searched_bytes: json?.searched_bytes,
      truncated: json?.truncated,
      truncated_reasons: json?.truncated_reasons,
      resultsLength: Array.isArray(json?.results) ? json.results.length : null,
    };
  })());
}

function peakRssKb(snapshots) {
  return Math.max(
    0,
    ...snapshots.map((snapshot) => snapshot?.status?.VmRSS ?? 0)
  );
}

async function stopServer(child) {
  if (!child || child.exitCode !== null) {
    return;
  }
  child.kill('SIGTERM');
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      return;
    }
    await delay(100);
  }
  child.kill('SIGKILL');
}
```

- [ ] **Step 2: Fix request tracking**

Replace this line in `runMeasuredScenario()`:

```js
    const requestPromise = requestSearch(options.port, options.query);
```

with:

```js
    const requestPromise = trackPromise(requestSearch(options.port, options.query));
```

Then replace `requestSearch()` with a plain promise-returning function:

```js
async function requestSearch(port, query) {
  const response = await fetch(`http://127.0.0.1:${port}/api/search?q=${encodeURIComponent(query)}`, {
    headers: { Host: `127.0.0.1:${port}` },
  });
  const body = await response.text();
  let json = null;
  try {
    json = JSON.parse(body);
  } catch (_error) {
    json = null;
  }
  return {
    httpStatus: response.status,
    responseBytes: Buffer.byteLength(body),
    searched_files: json?.searched_files,
    searched_bytes: json?.searched_bytes,
    truncated: json?.truncated,
    truncated_reasons: json?.truncated_reasons,
    resultsLength: Array.isArray(json?.results) ? json.results.length : null,
  };
}
```

- [ ] **Step 3: Wire scenarios into `runMeasurement()`**

Replace `runMeasurement()` with:

```js
async function runMeasurement(options) {
  const root = createFixtureRoot();
  try {
    const reports = [];
    for (const fixtureKind of options.fixtures) {
      const fixture = createFixture(root, fixtureKind, options.fixtureScale);
      for (const mode of options.modes) {
        for (const runKind of options.runs) {
          const scenario = await runMeasuredScenario(options, fixture, mode, runKind);
          reports.push({
            fixtureKind,
            fixture: {
              fileCount: fixture.fileCount,
              bytes: fixture.bytes,
              workspace: fixture.maskedWorkspace,
            },
            ...scenario,
          });
        }
      }
    }
    console.log(JSON.stringify(sanitizeReport({ tempRoot: root, reports }), null, 2));
    return 0;
  } finally {
    if (!options.keepTemp) {
      rmSync(root, { recursive: true, force: true });
    }
  }
}
```

- [ ] **Step 4: Run smoke measurement**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --smoke --port 3119
```

Expected: exit code `0`; JSON contains one report with `httpStatus: 200`, `truncated_reasons`, `peakRssKb`, `before`, `after`, and `settled`.

- [ ] **Step 5: Check sanitization in smoke output**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --smoke --port 3120 > /tmp/markdown-view-search-rss-plateau-smoke.json
rg '/tmp/markdown-view-search-rss-plateau[.-][A-Za-z0-9_-]+|/home/|target/debug/markdown-view .*--port|needle paragraph|needle_inside_single_large_block' /tmp/markdown-view-search-rss-plateau-smoke.json
```

Expected: the `node` command exits `0`; `rg` exits `1`. If the smoke run fails because the port is busy, rerun with a different port and record that in the final report.

- [ ] **Step 6: Commit**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "chore: 検索RSS計測のHTTP測定を追加"
```

Expected: commit succeeds.

## Task 5: Full Matrix Measurement And TODO Update

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: Run the measurement matrix**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --fixture-scale full --modes dev,release --fixtures prefix,multifile,fallback --runs cold,warm --port 3121
```

Expected: JSON report for 12 scenarios. If the environment cannot complete all scenarios, rerun smaller groups:

```bash
node scripts/measure-search-rss-plateau.mjs --fixture-scale full --modes release --fixtures prefix --runs cold,warm --port 3122
node scripts/measure-search-rss-plateau.mjs --fixture-scale full --modes release --fixtures fallback --runs cold,warm --port 3123
node scripts/measure-search-rss-plateau.mjs --fixture-scale full --modes dev --fixtures prefix,multifile --runs cold,warm --port 3124
```

- [ ] **Step 2: Classify the result**

Use these rules:

- If `RssAnon` and `smaps_rollup Anonymous` dominate while elapsed is low and response JSON is normal, classify as `glibc allocator arena / retained anonymous memory` or `WSL2 / proc characteristic` depending on whether repeated warm runs stabilize. Treat `maps.anonymousKb` as virtual address range context only; do not use it as the primary RSS plateau signal.
- If release `prefix` is low elapsed but `fallback` is materially slower or higher RSS, classify `安全境界なし巨大 block fallback` as a remaining performance path.
- If `before` to `settled` grows mainly after server start before search, classify `Tokio runtime / process initialization` as a contributor.
- If `smaps_rollup` is unavailable and `status` is insufficient, classify as `未特定` and list the missing measurement.

- [ ] **Step 3: Update `docs/todo/TODO.md` Medium item**

Replace the current Medium item body for `ディレクトリ検索 many-match の RSS plateau を切り分ける` with a concise result summary in this shape:

```markdown
- [ ] ディレクトリ検索 many-match の RSS plateau を切り分ける
  - 計測: 2026-06-04 に `scripts/measure-search-rss-plateau.mjs` で `/tmp/markdown-view-search-rss-plateau.***` fixture を使い、dev/release、cold/warm、prefix/multifile/fallback を測定した。HTTP response は `searched_files`、`searched_bytes`、`truncated=true`、`truncated_reasons=["result_limit"]` を維持し、実パス、full process args、本文断片、raw maps 行は記録していない。
  - 結果: release prefix は elapsed 0.02-0.04s、settled RSS は約 1.0GiB、`RssAnon` と anonymous maps が支配的だった。fallback は prefix より elapsed と RSS が高く、安全境界なし巨大 block が別経路として残ることを確認した。multifile は result-limit 停止で低 RSS を維持した。
  - 判断: prefix の低 elapsed と高 `RssAnon` から、検索アルゴリズム本体より glibc allocator arena / retained anonymous memory または WSL2 `/proc` 計測特性が支配的と判断する。fallback は安全境界なし巨大 block の性能リスクとして別途扱う。
  - 残件: allocator / WSL2 切り分けには同一 fixture を native Linux または allocator 設定変更で再測定する。fallback 改善は correctness を維持できる安全境界の追加設計が必要。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は弱めていない。
```

Do not paste raw JSON. Do not include exact temp paths, full command lines with process args, or Markdown body excerpts.

- [ ] **Step 4: Commit**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: 検索RSS plateau計測結果を記録"
```

Expected: commit succeeds.

## Task 6: Final Verification

**Files:**
- Verify: `scripts/measure-search-rss-plateau.mjs`
- Verify: `docs/todo/TODO.md`

- [ ] **Step 1: Run script checks**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --help
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
```

Expected: both commands exit `0`.

- [ ] **Step 2: Run final sanitization grep**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --smoke --port 3125 > /tmp/markdown-view-search-rss-plateau-smoke.json
rg '/tmp/markdown-view-search-rss-plateau[.-][A-Za-z0-9_-]+|/home/|target/(debug|release)/markdown-view .*--port|needle paragraph|needle_inside_single_large_block' /tmp/markdown-view-search-rss-plateau-smoke.json
```

Expected: the `node` command exits `0`; `rg` exits `1`.

- [ ] **Step 3: Run required verification**

Run:

```bash
./verify.sh
```

Expected: format, lint, and tests pass.

- [ ] **Step 4: Inspect git state**

Run:

```bash
git status --short --branch
git log --oneline -5
```

Expected: working tree clean; recent commits include the script, measurement result, and this plan/design history.

## Self-Review Notes

- Spec coverage: plan covers script creation, fixture matrix, `/proc` status/maps summaries, sanitization, TODO result update, and `./verify.sh`.
- Non-goals preserved: no Rust production change, no UI/TS/generated JS change, no external dependencies, no permanent telemetry.
- Security coverage: every report path uses sanitization; raw maps rows, exact temp paths, full args, and Markdown body fragments are excluded.
- Rollback path: remove `scripts/measure-search-rss-plateau.mjs` and revert `docs/todo/TODO.md` result update.
