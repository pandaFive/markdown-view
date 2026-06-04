import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { release as osRelease, tmpdir } from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const DEFAULT_QUERY = 'needle';
const TEMP_PREFIX = 'markdown-view-search-rss-plateau-';
const MASKED_TEMP = '/tmp/markdown-view-search-rss-plateau.***';
const DEFAULT_PORT = 3109;
const REQUEST_TIMEOUT_MS = 120_000;
const MAX_RESPONSE_BYTES = 2 * 1024 * 1024;
const SAMPLE_INTERVAL_MS = 50;
const MAX_SAMPLES = Math.ceil(REQUEST_TIMEOUT_MS / SAMPLE_INTERVAL_MS) + 20;
const EXPECTED_SEARCH_LIMITS = {
  max_results: 100,
  max_files: 1000,
  max_bytes: 64 * 1024 * 1024,
};
const REDACTED = '<redacted>';
const TEMP_ROOTS = new Set();
const SENSITIVE_REPORT_KEYS = new Set(['body', 'content', 'context', 'results', 'snippet', 'text']);

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
  let sanitized = value.replaceAll(process.cwd(), '<repo>');
  for (const root of TEMP_ROOTS) {
    sanitized = sanitized.replaceAll(root, MASKED_TEMP);
  }
  return sanitized
    .replace(/(?:\/[^\s/"']+)*\/markdown-view-search-rss-plateau[.\-][^/\s"']+/g, MASKED_TEMP)
    .replace(/\/home\/[^\s"']+/g, '<home-path>');
}

function sanitizeReport(value) {
  if (Array.isArray(value)) {
    return value.map(sanitizeReport);
  }
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, nested]) => [
        key,
        SENSITIVE_REPORT_KEYS.has(key.toLowerCase()) ? REDACTED : sanitizeReport(nested),
      ])
    );
  }
  return sanitizePath(value);
}

function runSanitizationSelfTest() {
  const raw = {
    temp: '/tmp/markdown-view-search-rss-plateau.abcd1234/workspace/prefix.md',
    hyphenTemp: '/tmp/markdown-view-search-rss-plateau-abcd1234/workspace/prefix.md',
    customTemp: '/var/tmp/markdown-view-search-rss-plateau-xyz789/workspace/prefix.md',
    repo: `${process.cwd()}/target/debug/markdown-view`,
    home: '/home/example/secret/file.md',
    body: 'needle paragraph body should not be passed into reports',
    nested: {
      snippet: 'needle paragraph 42',
      results: [{ context: 'needle_inside_single_large_block secret' }],
    },
  };
  const sanitized = sanitizeReport(raw);
  assert.equal(sanitized.temp, `${MASKED_TEMP}/workspace/prefix.md`);
  assert.equal(sanitized.hyphenTemp, `${MASKED_TEMP}/workspace/prefix.md`);
  assert.equal(sanitized.customTemp, `${MASKED_TEMP}/workspace/prefix.md`);
  assert.equal(sanitized.repo, '<repo>/target/debug/markdown-view');
  assert.equal(sanitized.home, '<home-path>');
  assert.equal(sanitized.body, '<redacted>');
  assert.equal(sanitized.nested.snippet, '<redacted>');
  assert.equal(sanitized.nested.results, '<redacted>');

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
  assert.equal(peakRssKb([{ status: { available: false, reason: 'missing' } }]), null);
  assert.deepEqual(
    summarizeProcCompleteness([{
      status: { available: false, reason: 'permission_denied', code: 'EACCES' },
      smapsRollup: { available: false, reason: 'missing' },
      maps: { available: false, reason: 'parse_empty' },
    }]),
    {
      procComplete: false,
      partialMeasurementReasons: [
        'status:permission_denied:EACCES',
        'smapsRollup:missing',
        'maps:parse_empty',
      ],
    }
  );

  assert.throws(
    () => summarizeSearchResponse({ ok: false, status: 500 }, '{}'),
    /search request failed/
  );
  assert.throws(
    () => summarizeSearchResponse({ ok: true, status: 200 }, 'not json'),
    /search response was not valid JSON/
  );
  assert.throws(
    () => summarizeSearchResponse({ ok: true, status: 200 }, '{"results":[]}', 'needle'),
    /search response missing query/
  );
  assert.throws(
    () => summarizeSearchResponse(
      { ok: true, status: 200 },
      JSON.stringify({
        query: 'other',
        results: [],
        searched_files: 1,
        skipped_files: 0,
        searched_bytes: 123,
        truncated: false,
        truncated_reasons: [],
        limits: EXPECTED_SEARCH_LIMITS,
      }),
      'needle'
    ),
    /search response query mismatch/
  );
  assert.throws(
    () => summarizeSearchResponse(
      { ok: true, status: 200 },
      JSON.stringify({
        query: 'needle',
        results: [],
        searched_files: 1,
        skipped_files: 0,
        searched_bytes: 123,
        truncated: false,
        truncated_reasons: [],
        limits: { max_results: 99, max_files: 1000, max_bytes: 64 * 1024 * 1024 },
      }),
      'needle'
    ),
    /search response limits mismatch/
  );
  const validSearchBody = JSON.stringify({
    query: 'needle',
    results: [{ title: 'hidden' }],
    searched_files: 1,
    skipped_files: 0,
    searched_bytes: 123,
    truncated: true,
    truncated_reasons: ['result_limit'],
    limits: EXPECTED_SEARCH_LIMITS,
  });
  assert.deepEqual(
    summarizeSearchResponse({ ok: true, status: 200 }, validSearchBody, 'needle'),
    {
      httpStatus: 200,
      responseBytes: Buffer.byteLength(validSearchBody),
      searched_files: 1,
      skipped_files: 0,
      searched_bytes: 123,
      truncated: true,
      truncated_reasons: ['result_limit'],
      resultsLength: 1,
    }
  );
  assert.doesNotThrow(() => validateScenarioResult('prefix', summarizeSearchResponse(
    { ok: true, status: 200 },
    JSON.stringify({
      query: 'needle',
      results: new Array(100).fill({ title: 'hidden' }),
      searched_files: 1,
      skipped_files: 0,
      searched_bytes: 123,
      truncated: true,
      truncated_reasons: ['result_limit'],
      limits: EXPECTED_SEARCH_LIMITS,
    }),
    'needle'
  )));
  assert.throws(
    () => validateScenarioResult('fallback', {
      searched_files: 0,
      searched_bytes: 0,
      resultsLength: 0,
      truncated_reasons: [],
    }),
    /invalid scenario/
  );
  assert.equal(
    outputIndicatesPortFallback('[markdown-view] ポート 3109 は使用中のため、空きポート 3110 を使用します'),
    true
  );
  assert.equal(outputServerUrlPort('URL: http://127.0.0.1:3125'), 3125);
  assert.equal(outputServerUrlPort('URL: http://127.0.0.1:3126'), 3126);
  assert.equal(outputServerUrlPort('ready'), null);
  assert.equal(isPortUnavailableError(Object.assign(new Error('busy'), { code: 'EADDRINUSE' })), true);
  assert.equal(isPortUnavailableError(Object.assign(new Error('denied'), { code: 'EACCES' })), true);
  const context = buildMeasurementContext(parseArgs(['--query', 'needle']));
  assert.equal(Object.hasOwn(context.cliOptions, 'queryLength'), false);
  assert.equal(Object.hasOwn(context.cliOptions, 'querySha256'), false);

  assert.deepEqual(readMapsSummaryFromText('invalid maps line'), {
    available: false,
    reason: 'parse_failed',
    malformedLineCount: 1,
    mappingCount: 0,
  });

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

    const fullFallback = createFixture(root, 'fallback', 'full');
    assert.equal(fullFallback.fixtureKind, 'fallback');
    assert.equal(fullFallback.fileCount, 1);
    assert.ok(fullFallback.bytes > fallback.bytes * 10);
    assert.ok(fullFallback.bytes < 10 * 1024 * 1024);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }

  const cleanupError = attachCleanupFailure(new Error('measurement failed'), 12345);
  assert.equal(cleanupError.cleanupFailed, true);
  assert.equal(cleanupError.cleanupErrorKind, 'server_stop_failed');
  assert.match(cleanupError.cleanupMessage, /server stop failed/);
  const cleanupReport = errorReportFields(cleanupError);
  assert.deepEqual(cleanupReport, {
    errorKind: 'measurement_failed',
    sanitizedMessage: 'measurement failed',
    cleanupFailed: true,
    cleanupErrorKind: 'server_stop_failed',
    cleanupMessage: 'server stop failed for pid 12345',
  });
}

function createFixtureRoot() {
  const root = mkdtempSync(path.join(tmpdir(), TEMP_PREFIX));
  TEMP_ROOTS.add(root);
  return root;
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
  return readMapsSummaryFromText(procFile.text);
}

function readMapsSummaryFromText(text) {
  const summary = {
    available: true,
    anonymousKb: 0,
    fileBackedKb: 0,
    heapKb: 0,
    stackKb: 0,
    otherSpecialKb: 0,
    mappingCount: 0,
    malformedLineCount: 0,
  };
  for (const line of text.split('\n')) {
    if (line.trim() === '') {
      continue;
    }
    const entry = parseMapsLine(line);
    if (!entry) {
      summary.malformedLineCount += 1;
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
  if (summary.mappingCount === 0) {
    return {
      available: false,
      reason: summary.malformedLineCount > 0 ? 'parse_failed' : 'parse_empty',
      malformedLineCount: summary.malformedLineCount,
      mappingCount: summary.mappingCount,
    };
  }
  if (summary.malformedLineCount > 0) {
    return {
      available: false,
      reason: 'parse_failed',
      malformedLineCount: summary.malformedLineCount,
      mappingCount: summary.mappingCount,
    };
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
  let scenarioError = null;
  try {
    await waitForServer(server, options.port);
    let warmupResponse = null;
    if (runKind === 'warm') {
      warmupResponse = await requestSearch(options.port, options.query, REQUEST_TIMEOUT_MS);
      validateScenarioResult(fixture.fixtureKind, warmupResponse);
    }
    const before = readProcSnapshot(server.pid);
    const startedAt = process.hrtime.bigint();
    const requestPromise = trackPromise(requestSearch(options.port, options.query, REQUEST_TIMEOUT_MS));
    const samples = [];
    while (!isPromiseSettled(requestPromise) && samples.length < MAX_SAMPLES) {
      samples.push(readProcSnapshot(server.pid));
      await delay(SAMPLE_INTERVAL_MS);
    }
    if (!isPromiseSettled(requestPromise)) {
      requestPromise.abort?.();
      throw new Error('request sampling limit exceeded');
    }
    const response = await requestPromise;
    validateScenarioResult(fixture.fixtureKind, response);
    const endedAt = process.hrtime.bigint();
    const after = readProcSnapshot(server.pid);
    await delay(5000);
    const settled = readProcSnapshot(server.pid);
    const snapshots = [before, ...samples, after, settled];
    const procCompleteness = summarizeProcCompleteness(snapshots);
    return {
      mode,
      runKind,
      pid: server.pid,
      elapsedMs: Number(endedAt - startedAt) / 1_000_000,
      peakRssKb: peakRssKb(snapshots),
      procComplete: procCompleteness.procComplete,
      partialMeasurementReasons: procCompleteness.partialMeasurementReasons,
      before,
      after,
      settled,
      ...(warmupResponse ? { warmupResponse } : {}),
      response,
    };
  } catch (error) {
    scenarioError = normalizeError(error);
    throw scenarioError;
  } finally {
    const stopped = await stopServer(server);
    if (!stopped) {
      if (scenarioError) {
        attachCleanupFailure(scenarioError, server.pid);
      } else {
        throw new Error(`server stop failed for pid ${server.pid}`);
      }
    }
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
  await assertPortAvailable(port);
  const binaryPath = ensureBuiltBinary(mode);
  const child = spawn(binaryPath, [workspace, '--port', String(port), '--no-open'], {
    cwd: process.cwd(),
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const stdoutChunks = [];
  const stderrChunks = [];
  let startError = null;
  child.on('error', (error) => {
    startError = error;
  });
  child.stderr.on('data', (chunk) => {
    stderrChunks.push(chunk.toString('utf8'));
  });
  child.stdout.on('data', (chunk) => {
    stdoutChunks.push(chunk.toString('utf8'));
  });
  child.stderr.resume();
  await delay(250);
  if (startError) {
    const code = startError && typeof startError.code === 'string' ? startError.code : 'UNKNOWN';
    throw new Error(`server spawn failed: ${code}`);
  }
  if (isChildExited(child)) {
    const stderr = sanitizeProcessOutput(stderrChunks.join('').slice(0, 500));
    throw new Error(`server exited early with code ${child.exitCode ?? child.signalCode ?? 'UNKNOWN'}: ${stderr}`);
  }
  child.outputText = () => `${stdoutChunks.join('')}${stderrChunks.join('')}`;
  return child;
}

function assertPortAvailable(port) {
  return new Promise((resolve, reject) => {
    const server = createServer();
    const cleanup = () => {
      server.removeAllListeners();
    };
    server.once('error', (error) => {
      cleanup();
      const code = error && typeof error.code === 'string' ? error.code : 'UNKNOWN';
      const wrapped = new Error(`measurement port ${port} is not available: ${code}`);
      wrapped.code = code;
      reject(wrapped);
    });
    server.once('listening', () => {
      server.close((error) => {
        cleanup();
        if (error) {
          reject(error);
        } else {
          resolve();
        }
      });
    });
    server.listen(port, '127.0.0.1');
  });
}

function ensureBuiltBinary(mode) {
  const buildArgs = mode === 'release' ? ['build', '--release'] : ['build'];
  const result = spawnSync('cargo', buildArgs, {
    cwd: process.cwd(),
    encoding: 'utf8',
    maxBuffer: 10 * 1024 * 1024,
  });
  if (result.status !== 0) {
    const output = sanitizeProcessOutput(`${result.stdout ?? ''}${result.stderr ?? ''}`.slice(0, 500));
    throw new Error(`build failed with code ${result.status ?? 'UNKNOWN'}: ${output}`);
  }
  return path.join(process.cwd(), mode === 'release' ? 'target/release/markdown-view' : 'target/debug/markdown-view');
}

function sanitizeProcessOutput(value) {
  return sanitizePath(value)
    .split('\n')
    .map((line) => line.replace(/Running `[^`]+`/, 'Running <server-command>'))
    .join('\n');
}

async function waitForServer(child, port) {
  const deadline = Date.now() + 60_000;
  let lastProbeError = null;
  while (Date.now() < deadline) {
    const output = child.outputText?.() ?? '';
    if (outputIndicatesPortFallback(output)) {
      throw new Error('server used a fallback port; choose an unused measurement port');
    }
    const outputPort = outputServerUrlPort(output);
    if (outputPort !== null && outputPort !== port) {
      throw new Error(`server bound unexpected port ${outputPort}; expected ${port}`);
    }
    if (isChildExited(child)) {
      throw new Error(`server exited before readiness with code ${child.exitCode ?? child.signalCode ?? 'UNKNOWN'}`);
    }
    if (outputPort === port) {
      try {
        await requestSearch(port, '', 2_000);
        if (!isChildExited(child)) {
          return;
        }
      } catch (error) {
        if (errorMessage(error).includes('fallback port')) {
          throw error;
        }
        lastProbeError = error;
      }
    }
    await delay(200);
  }
  const output = sanitizeProcessOutput((child.outputText?.() ?? '').slice(-500));
  const probe = lastProbeError ? ` last probe: ${lastProbeError.message}` : '';
  throw new Error(`server did not become ready:${probe} ${output}`);
}

function outputServerUrlPort(output) {
  const plainOutput = output.replace(/\x1b\[[0-9;]*m/g, '');
  const match = /URL:\s+http:\/\/127\.0\.0\.1:(\d+)/.exec(plainOutput);
  if (!match) {
    return null;
  }
  const port = Number.parseInt(match[1], 10);
  return Number.isFinite(port) ? port : null;
}

function isPortUnavailableError(error) {
  return error && (error.code === 'EADDRINUSE' || error.code === 'EACCES' || error.code === 'EPERM');
}

function outputIndicatesPortFallback(output) {
  return /ポート\s+\d+\s+は使用中のため、空きポート\s+\d+\s+を使用します/.test(output);
}

function requestSearch(port, query, timeoutMs) {
  const controller = new AbortController();
  const timeout = setTimeout(() => {
    controller.abort();
  }, timeoutMs);
  const promise = (async () => {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/api/search?q=${encodeURIComponent(query)}`, {
        headers: { Host: `127.0.0.1:${port}` },
        signal: controller.signal,
      });
      const body = await readResponseTextWithLimit(response, MAX_RESPONSE_BYTES);
      return summarizeSearchResponse(response, body, query);
    } finally {
      clearTimeout(timeout);
    }
  })();
  promise.abort = () => controller.abort();
  return promise;
}

async function readResponseTextWithLimit(response, byteLimit) {
  if (!response.body) {
    const body = await response.text();
    if (Buffer.byteLength(body) > byteLimit) {
      throw new Error(`search response exceeded ${byteLimit} bytes`);
    }
    return body;
  }
  const reader = response.body.getReader();
  const chunks = [];
  let total = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) {
        break;
      }
      total += value.byteLength;
      if (total > byteLimit) {
        await reader.cancel();
        throw new Error(`search response exceeded ${byteLimit} bytes`);
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }
  return Buffer.concat(chunks.map((chunk) => Buffer.from(chunk))).toString('utf8');
}

function validateScenarioResult(fixtureKind, response) {
  const reasons = [];
  if (!(response.searched_files > 0)) {
    reasons.push('searched_files must be greater than 0');
  }
  if (!(response.searched_bytes > 0)) {
    reasons.push('searched_bytes must be greater than 0');
  }
  if (response.resultsLength !== EXPECTED_SEARCH_LIMITS.max_results) {
    reasons.push(`resultsLength must be ${EXPECTED_SEARCH_LIMITS.max_results}`);
  }
  if (!response.truncated_reasons.includes('result_limit')) {
    reasons.push('truncated_reasons must include result_limit');
  }
  if (reasons.length > 0) {
    throw new Error(`invalid scenario for ${fixtureKind}: ${reasons.join('; ')}`);
  }
}

function summarizeSearchResponse(response, body, expectedQuery) {
  const responseBytes = Buffer.byteLength(body);
  if (responseBytes > MAX_RESPONSE_BYTES) {
    throw new Error(`search response exceeded ${MAX_RESPONSE_BYTES} bytes`);
  }
  if (!response.ok) {
    throw new Error(`search request failed with status ${response.status}`);
  }
  let json;
  try {
    json = JSON.parse(body);
  } catch (_error) {
    throw new Error('search response was not valid JSON');
  }
  const expectedFields = [
    ['query', (value) => typeof value === 'string'],
    ['searched_files', Number.isFinite],
    ['skipped_files', Number.isFinite],
    ['searched_bytes', Number.isFinite],
    ['truncated', (value) => typeof value === 'boolean'],
    ['truncated_reasons', Array.isArray],
    ['results', Array.isArray],
    ['limits', (value) => value && typeof value === 'object'],
  ];
  for (const [fieldName, predicate] of expectedFields) {
    if (!predicate(json?.[fieldName])) {
      throw new Error(`search response missing ${fieldName}`);
    }
  }
  if (json.query !== expectedQuery) {
    throw new Error('search response query mismatch');
  }
  for (const [key, expectedValue] of Object.entries(EXPECTED_SEARCH_LIMITS)) {
    if (json.limits[key] !== expectedValue) {
      throw new Error(`search response limits mismatch: ${key}`);
    }
  }
  return {
    httpStatus: response.status,
    responseBytes,
    searched_files: json.searched_files,
    skipped_files: json.skipped_files,
    searched_bytes: json.searched_bytes,
    truncated: json.truncated,
    truncated_reasons: json.truncated_reasons,
    resultsLength: json.results.length,
  };
}

function peakRssKb(snapshots) {
  const rssValues = snapshots
    .map((snapshot) => snapshot?.status?.VmRSS)
    .filter((value) => Number.isFinite(value));
  if (rssValues.length === 0) {
    return null;
  }
  return Math.max(...rssValues);
}

function summarizeProcCompleteness(snapshots) {
  const reasons = new Set();
  for (const snapshot of snapshots) {
    for (const [probeName, probe] of Object.entries({
      status: snapshot?.status,
      smapsRollup: snapshot?.smapsRollup,
      maps: snapshot?.maps,
    })) {
      if (probe && probe.available === false) {
        reasons.add(probe.code ? `${probeName}:${probe.reason}:${probe.code}` : `${probeName}:${probe.reason}`);
      }
    }
  }
  return {
    procComplete: reasons.size === 0,
    partialMeasurementReasons: [...reasons],
  };
}

async function stopServer(child) {
  if (!child || isChildExited(child)) {
    return true;
  }
  child.kill('SIGTERM');
  if (await waitForChildExit(child, 5_000)) {
    return true;
  }
  child.kill('SIGKILL');
  return waitForChildExit(child, 1_000);
}

async function waitForChildExit(child, timeoutMs) {
  if (!child || isChildExited(child)) {
    return true;
  }
  return new Promise((resolve) => {
    const cleanup = () => {
      clearTimeout(timeout);
      child.off('exit', onExit);
      child.off('close', onExit);
      child.off('error', onExit);
    };
    const onExit = () => {
      cleanup();
      resolve(true);
    };
    const timeout = setTimeout(() => {
      cleanup();
      resolve(false);
    }, timeoutMs);
    child.once('exit', onExit);
    child.once('close', onExit);
    child.once('error', onExit);
  });
}

function isChildExited(child) {
  return child.exitCode !== null || child.signalCode !== null;
}

async function runMeasurement(options) {
  const root = createFixtureRoot();
  try {
    const reports = [];
    let failed = false;
    for (const fixtureKind of options.fixtures) {
      const fixture = createFixture(root, fixtureKind, options.fixtureScale);
      for (const mode of options.modes) {
        for (const runKind of options.runs) {
          const baseReport = {
            fixtureKind,
            fixture: {
              fileCount: fixture.fileCount,
              bytes: fixture.bytes,
              workspace: fixture.maskedWorkspace,
            },
            mode,
            runKind,
          };
          try {
            const scenario = await runMeasuredScenario(options, fixture, mode, runKind);
            reports.push({
              ...baseReport,
              status: 'ok',
              ...scenario,
            });
          } catch (error) {
            failed = true;
            reports.push({
              ...baseReport,
              status: 'failed',
              ...errorReportFields(error),
            });
          }
        }
      }
    }
    console.log(JSON.stringify(sanitizeReport({
      measurementContext: buildMeasurementContext(options),
      tempRoot: root,
      reports,
    }), null, 2));
    return failed ? 1 : 0;
  } finally {
    if (!options.keepTemp) {
      rmSync(root, { recursive: true, force: true });
    }
  }
}

function classifyError(error) {
  const name = error && typeof error.name === 'string' ? error.name : 'Error';
  const message = errorMessage(error);
  if (name === 'AbortError') {
    return 'timeout';
  }
  if (isPortUnavailableError(error)) {
    return 'port_unavailable';
  }
  if (message.includes('fallback port')) {
    return 'port_fallback';
  }
  if (message.includes('invalid scenario')) {
    return 'invalid_scenario';
  }
  if (message.includes('server stop failed')) {
    return 'server_stop_failed';
  }
  if (message.includes('search request failed')) {
    return 'http_failed';
  }
  if (message.includes('search response')) {
    return 'schema_failed';
  }
  if (message.includes('server')) {
    return 'server_failed';
  }
  return 'measurement_failed';
}

function errorReportFields(error) {
  const report = {
    errorKind: classifyError(error),
    sanitizedMessage: sanitizeProcessOutput(errorMessage(error)),
  };
  if (error && error.cleanupFailed === true) {
    report.cleanupFailed = true;
    report.cleanupErrorKind = error.cleanupErrorKind ?? 'server_stop_failed';
    report.cleanupMessage = sanitizeProcessOutput(error.cleanupMessage ?? '');
  }
  return report;
}

function attachCleanupFailure(error, pid) {
  const target = normalizeError(error);
  target.cleanupFailed = true;
  target.cleanupErrorKind = 'server_stop_failed';
  target.cleanupMessage = `server stop failed for pid ${pid}`;
  return target;
}

function normalizeError(error) {
  return error && typeof error === 'object' ? error : new Error(errorMessage(error));
}

function errorMessage(error) {
  return error && typeof error.message === 'string' ? error.message : String(error);
}

function buildMeasurementContext(options) {
  return {
    node: process.version,
    platform: process.platform,
    arch: process.arch,
    osRelease: osRelease(),
    head: currentGitHead(),
    cliOptions: {
      smoke: options.smoke,
      fixtureScale: options.fixtureScale,
      modes: options.modes,
      fixtures: options.fixtures,
      runs: options.runs,
      port: options.port,
    },
  };
}

function currentGitHead() {
  const result = spawnSync('git', ['rev-parse', '--short=12', 'HEAD'], {
    cwd: process.cwd(),
    encoding: 'utf8',
    maxBuffer: 1024 * 1024,
  });
  if (result.status !== 0) {
    return 'unknown';
  }
  return result.stdout.trim();
}

Promise.resolve().then(() => main()).then((code) => {
  process.exitCode = code;
}).catch((error) => {
  console.error(sanitizePath(errorMessage(error)));
  process.exitCode = 1;
});
