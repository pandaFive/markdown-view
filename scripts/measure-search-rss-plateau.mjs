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

Promise.resolve().then(() => main()).then((code) => {
  process.exitCode = code;
}).catch((error) => {
  console.error(sanitizePath(error.message));
  process.exitCode = 1;
});
