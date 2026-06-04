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
  const explicitMatrixOptions = new Set();

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
      options.port = parsePort(readValue(argv, ++index, arg), arg);
    } else if (arg === '--query') {
      options.query = readValue(argv, ++index, arg);
    } else if (arg === '--modes') {
      explicitMatrixOptions.add(arg);
      options.modes = parseList(readValue(argv, ++index, arg), ['dev', 'release'], arg);
    } else if (arg === '--fixtures') {
      explicitMatrixOptions.add(arg);
      options.fixtures = parseList(readValue(argv, ++index, arg), ['prefix', 'multifile', 'fallback'], arg);
    } else if (arg === '--runs') {
      explicitMatrixOptions.add(arg);
      options.runs = parseList(readValue(argv, ++index, arg), ['cold', 'warm'], arg);
    } else if (arg === '--fixture-scale') {
      explicitMatrixOptions.add(arg);
      options.fixtureScale = parseChoice(readValue(argv, ++index, arg), ['short', 'full'], arg);
    } else if (arg === '--output') {
      options.output = parseChoice(readValue(argv, ++index, arg), ['json'], arg);
    } else {
      throw new Error(`unknown option: ${arg}`);
    }
  }

  if (options.smoke && explicitMatrixOptions.size > 0) {
    throw new Error('--smoke cannot be combined with --modes, --fixtures, --runs, or --fixture-scale');
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
  if (!/^\d+$/.test(raw)) {
    throw new Error(`${optionName} must be a positive integer`);
  }
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${optionName} must be a positive integer`);
  }
  return value;
}

function parsePort(raw, optionName) {
  const value = parsePositiveInt(raw, optionName);
  if (value > 65535) {
    throw new Error(`${optionName} must be between 1 and 65535`);
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
                                 Cannot be combined with --modes, --fixtures, --runs, or --fixture-scale.
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

  assert.throws(() => parseArgs(['--port', '123abc']), /positive integer/);
  assert.throws(() => parseArgs(['--port', '1.5']), /positive integer/);
  assert.throws(() => parseArgs(['--port', '0']), /positive integer/);
  assert.throws(() => parseArgs(['--port', '70000']), /between 1 and 65535/);
  assert.equal(parseArgs(['--port', '65535', '--self-test-sanitization']).port, 65535);

  const matrixOptionPairs = [
    ['--modes', 'release'],
    ['--fixtures', 'prefix'],
    ['--runs', 'cold'],
    ['--fixture-scale', 'short'],
  ];
  for (const [optionName, optionValue] of matrixOptionPairs) {
    assert.throws(
      () => parseArgs(['--smoke', optionName, optionValue]),
      /--smoke cannot be combined/
    );
    assert.throws(
      () => parseArgs([optionName, optionValue, '--smoke']),
      /--smoke cannot be combined/
    );
  }

  const smokeOptions = parseArgs(['--smoke']);
  assert.deepEqual(smokeOptions.modes, ['dev']);
  assert.deepEqual(smokeOptions.fixtures, ['prefix']);
  assert.deepEqual(smokeOptions.runs, ['cold']);
  assert.equal(smokeOptions.fixtureScale, 'short');

  const anonymous = parseMapsLine('7f0000000000-7f0000100000 rw-p 00000000 00:00 0');
  const fileBacked = parseMapsLine('7f0000200000-7f0000300000 r--p 00000000 08:01 123 /usr/lib/libc.so');
  assert.equal(anonymous.sizeKb, 1024);
  assert.equal(anonymous.name, '');
  assert.equal(fileBacked.sizeKb, 1024);
  assert.equal(fileBacked.name, '/usr/lib/libc.so');
  assert.equal(parseMapsLine('invalid maps line'), null);
  const missingProcFile = readProcFile(path.join(tmpdir(), `markdown-view-missing-proc-${process.pid}`));
  assert.equal(missingProcFile.ok, false);
  assert.equal(missingProcFile.reason, 'missing');

  const emptyStatus = parseProcKeyValues('not status', ['VmRSS']);
  assert.equal(emptyStatus.parsedFieldCount, 0);
  assert.deepEqual(emptyStatus.parsed, {});

  const root = createFixtureRoot();
  try {
    const prefix = createFixture(root, 'prefix', 'short');
    assert.equal(prefix.fixtureKind, 'prefix');
    assert.equal(prefix.fileCount, 1);
    assert.ok(prefix.bytes > 0);
    assert.equal(prefix.maskedWorkspace.startsWith(MASKED_TEMP), true);

    const multifile = createFixture(root, 'multifile', 'short');
    assert.equal(multifile.fixtureKind, 'multifile');
    assert.equal(multifile.fileCount, 8);
    assert.ok(multifile.bytes > 0);

    const fallback = createFixture(root, 'fallback', 'short');
    assert.equal(fallback.fixtureKind, 'fallback');
    assert.equal(fallback.fileCount, 1);
    assert.ok(fallback.bytes > 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

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
  const repeatCount = scale === 'full' ? 700_000 : 4_000;
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

function readProcSnapshot(pid) {
  return {
    status: readProcStatus(pid),
    smapsRollup: readSmapsRollup(pid),
    maps: readMapsSummary(pid),
  };
}

function readProcStatus(pid) {
  const filePath = `/proc/${pid}/status`;
  const procFile = readProcFile(filePath);
  if (!procFile.ok) {
    return unavailableProcResult(procFile);
  }
  const { parsed, parsedFieldCount } = parseProcKeyValues(
    procFile.text,
    ['VmRSS', 'RssAnon', 'RssFile', 'RssShmem']
  );
  if (parsedFieldCount === 0) {
    return { available: false, reason: 'parse_empty' };
  }
  return { available: true, parsedFieldCount, ...parsed };
}

function readSmapsRollup(pid) {
  const filePath = `/proc/${pid}/smaps_rollup`;
  const procFile = readProcFile(filePath);
  if (!procFile.ok) {
    return unavailableProcResult(procFile);
  }
  const { parsed, parsedFieldCount } = parseProcKeyValues(
    procFile.text,
    ['Rss', 'Pss', 'Private_Clean', 'Private_Dirty', 'Shared_Clean', 'Shared_Dirty', 'Anonymous']
  );
  if (parsedFieldCount === 0) {
    return { available: false, reason: 'parse_empty' };
  }
  return { available: true, parsedFieldCount, ...parsed };
}

function readMapsSummary(pid) {
  const filePath = `/proc/${pid}/maps`;
  const procFile = readProcFile(filePath);
  if (!procFile.ok) {
    return unavailableProcResult(procFile);
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
  for (const line of procFile.text.split('\n')) {
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

function readProcFile(filePath) {
  if (!existsSync(filePath)) {
    return { ok: false, reason: 'missing' };
  }
  try {
    return { ok: true, text: readFileSync(filePath, 'utf8') };
  } catch (error) {
    const code = error && typeof error.code === 'string' ? error.code : 'UNKNOWN';
    if (code === 'EACCES' || code === 'EPERM') {
      return { ok: false, reason: 'permission_denied', code };
    }
    return { ok: false, reason: 'read_failed', code };
  }
}

function unavailableProcResult(procFile) {
  const result = { available: false, reason: procFile.reason };
  if (procFile.code) {
    result.code = procFile.code;
  }
  return result;
}

function parseProcKeyValues(text, allowedKeys) {
  const allowed = new Set(allowedKeys);
  const parsed = {};
  let parsedFieldCount = 0;
  for (const line of text.split('\n')) {
    const match = /^([A-Za-z_]+):\s+(\d+)\s+kB$/.exec(line);
    if (match && allowed.has(match[1])) {
      parsed[match[1]] = Number.parseInt(match[2], 10);
      parsedFieldCount += 1;
    }
  }
  return { parsedFieldCount, parsed };
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

async function runMeasuredScenario(options, fixture, mode, runKind) {
  const server = await startServer({ mode, workspace: fixture.workspace, port: options.port });
  try {
    await waitForServer(options.port);
    const before = readProcSnapshot(server.pid);
    const startedAt = process.hrtime.bigint();
    const requestPromise = trackPromise(requestSearch(options.port, options.query));
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
    const stderr = sanitizeProcessOutput(stderrChunks.join('').slice(0, 500));
    throw new Error(`server exited early with code ${child.exitCode}: ${stderr}`);
  }
  return child;
}

function sanitizeProcessOutput(value) {
  return sanitizePath(value)
    .split('\n')
    .map((line) => line.replace(/Running `[^`]+`/, 'Running <server-command>'))
    .join('\n');
}

async function waitForServer(port) {
  const deadline = Date.now() + 60_000;
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

Promise.resolve().then(() => main()).then((code) => {
  process.exitCode = code;
}).catch((error) => {
  console.error(sanitizePath(error.message));
  process.exitCode = 1;
});
