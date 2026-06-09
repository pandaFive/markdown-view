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
const ALLOCATOR_PROFILES = new Map([
  ['default', {}],
  ['arena1', { MALLOC_ARENA_MAX: '1' }],
  ['arena2', { MALLOC_ARENA_MAX: '2' }],
]);
const ALLOCATOR_ENV_KEYS = [
  'MALLOC_ARENA_MAX',
  'MALLOC_MMAP_THRESHOLD_',
  'MALLOC_TRIM_THRESHOLD_',
  'MALLOC_TOP_PAD_',
  'MALLOC_MMAP_MAX_',
  'GLIBC_TUNABLES',
  'LD_PRELOAD',
];
const MEASURED_SERVER_ENV_ALLOWLIST = [
  'PATH',
  'HOME',
  'TMPDIR',
  'TMP',
  'TEMP',
  'LANG',
  'LC_ALL',
  'LC_CTYPE',
  'RUST_BACKTRACE',
];
const DEFAULT_ALLOCATOR_PROFILES = ['default'];
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
  if (options.selfTest) {
    runSanitizationSelfTest();
    console.log(options.selfTestSanitizationAlias ? 'sanitization self-test: ok' : 'self-test: ok');
    return Promise.resolve(0);
  }
  return runMeasurement(options);
}

function parseArgs(argv) {
  const options = {
    help: false,
    selfTest: false,
    selfTestSanitizationAlias: false,
    timeline: false,
    strict: false,
    smoke: false,
    keepTemp: false,
    port: DEFAULT_PORT,
    query: DEFAULT_QUERY,
    modes: ['dev'],
    fixtures: ['prefix'],
    runs: ['cold'],
    fixtureScale: 'short',
    fixtureDensities: ['dense'],
    settledDelaysMs: [1000, 5000],
    allocatorProfiles: [...DEFAULT_ALLOCATOR_PROFILES],
    output: 'json',
  };
  const explicitMatrixOptions = new Set();

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--help' || arg === '-h') {
      options.help = true;
    } else if (arg === '--self-test' || arg === '--self-test-sanitization') {
      options.selfTest = true;
      options.selfTestSanitizationAlias = arg === '--self-test-sanitization';
    } else if (arg === '--timeline') {
      explicitMatrixOptions.add(arg);
      options.timeline = true;
    } else if (arg === '--strict') {
      options.strict = true;
    } else if (arg === '--smoke') {
      options.smoke = true;
      options.fixtureScale = 'short';
      options.modes = ['dev'];
      options.fixtures = ['prefix'];
      options.runs = ['cold'];
      options.allocatorProfiles = [...DEFAULT_ALLOCATOR_PROFILES];
      options.fixtureDensities = ['dense'];
      options.settledDelaysMs = [1000, 5000];
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
    } else if (arg === '--fixture-density') {
      explicitMatrixOptions.add(arg);
      options.fixtureDensities = parseFixtureDensities(readValue(argv, ++index, arg));
    } else if (arg === '--settled-delays') {
      explicitMatrixOptions.add(arg);
      options.settledDelaysMs = parseSettledDelays(readValue(argv, ++index, arg));
    } else if (arg === '--allocator-profiles') {
      explicitMatrixOptions.add(arg);
      options.allocatorProfiles = parseAllocatorProfiles(readValue(argv, ++index, arg));
    } else if (arg === '--output') {
      options.output = parseChoice(readValue(argv, ++index, arg), ['json'], arg);
    } else {
      throw new Error(`unknown option: ${arg}`);
    }
  }

  if (options.smoke && explicitMatrixOptions.size > 0) {
    throw new Error('--smoke cannot be combined with --timeline, --modes, --fixtures, --runs, --fixture-scale, --fixture-density, --settled-delays, or --allocator-profiles');
  }
  if (!options.timeline && explicitMatrixOptions.has('--fixture-density')) {
    throw new Error('--fixture-density requires --timeline');
  }
  if (!options.timeline && explicitMatrixOptions.has('--settled-delays')) {
    throw new Error('--settled-delays requires --timeline');
  }
  if (!options.timeline && options.strict) {
    throw new Error('--strict requires --timeline');
  }
  if (!options.timeline && options.fixtureDensities.join(',') !== 'dense') {
    throw new Error('--fixture-density requires --timeline');
  }
  if (!options.timeline && options.settledDelaysMs.join(',') !== '1000,5000') {
    throw new Error('--settled-delays requires --timeline');
  }
  const hasSparseDensity = options.fixtureDensities.includes('sparse');
  const hasNonPrefixFixture = options.fixtures.some((fixtureKind) => fixtureKind !== 'prefix');
  if (hasSparseDensity && hasNonPrefixFixture) {
    throw new Error('sparse density is only supported with --fixtures prefix');
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

function parseFixtureDensities(raw) {
  if (raw === '') {
    throw new Error('--fixture-density requires a value');
  }
  if (raw === 'dense') {
    return ['dense'];
  }
  if (raw === 'sparse') {
    return ['sparse'];
  }
  if (raw === 'dense,sparse') {
    return ['dense', 'sparse'];
  }
  throw new Error('--fixture-density must be dense, sparse, or dense,sparse');
}

function parseSettledDelays(raw) {
  if (raw === '') {
    throw new Error('--settled-delays requires a value');
  }
  if (raw === '1s,5s') {
    return [1000, 5000];
  }
  if (raw === '1s,5s,15s') {
    return [1000, 5000, 15000];
  }
  throw new Error('--settled-delays must be 1s,5s or 1s,5s,15s');
}

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

function allocatorProfileEnv(profileName) {
  const env = ALLOCATOR_PROFILES.get(profileName);
  if (!env) {
    throw new Error(`unsupported allocator profile: ${profileName}`);
  }
  return { ...env };
}

function allocatorProfileReport(profileName, baseEnv = process.env) {
  const env = allocatorProfileEnv(profileName);
  return {
    name: profileName,
    env,
    scrubbedAllocatorEnvKeys: scrubbedAllocatorEnvKeys(baseEnv, env),
    overriddenAllocatorEnvKeys: overriddenAllocatorEnvKeys(baseEnv, env),
  };
}

function buildMeasuredServerEnv(allocatorEnv, baseEnv = process.env) {
  const serverEnv = {};
  for (const key of MEASURED_SERVER_ENV_ALLOWLIST) {
    if (Object.prototype.hasOwnProperty.call(baseEnv, key)) {
      serverEnv[key] = baseEnv[key];
    }
  }
  return {
    ...serverEnv,
    ...allocatorEnv,
  };
}

function scrubbedAllocatorEnvKeys(baseEnv = process.env, allocatorEnv = {}) {
  return ALLOCATOR_ENV_KEYS.filter((key) => (
    Object.prototype.hasOwnProperty.call(baseEnv, key)
    && !Object.prototype.hasOwnProperty.call(allocatorEnv, key)
  ));
}

function overriddenAllocatorEnvKeys(baseEnv = process.env, allocatorEnv = {}) {
  return ALLOCATOR_ENV_KEYS.filter((key) => (
    Object.prototype.hasOwnProperty.call(baseEnv, key)
    && Object.prototype.hasOwnProperty.call(allocatorEnv, key)
  ));
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
  --self-test                    Run local contract checks without starting the server.
  --self-test-sanitization       Alias for --self-test.
  --smoke                        Run a short dev/prefix/cold measurement.
                                 Cannot be combined with --timeline, --modes, --fixtures, --runs, --fixture-scale, --fixture-density, --settled-delays, or --allocator-profiles.
  --timeline                     Run opt-in timeline diagnostics with request/body/settled snapshots.
  --strict                       Timeline reports only: exit non-zero unless acceptance status is full.
  --modes dev,release            Build modes to measure. Default: dev.
  --fixtures prefix,multifile,fallback
                                 Fixture kinds to measure. Default: prefix.
  --runs cold,warm               Run kinds to measure. Default: cold.
  --fixture-scale short,full     Fixture size. Default: short.
  --fixture-density dense,sparse Timeline-only fixture density. sparse is prefix-only; dense works for all fixture kinds. Default: dense.
  --settled-delays 1s,5s         Timeline-only settled snapshots. Must exactly match 1s,5s or 1s,5s,15s.
  --allocator-profiles default,arena1,arena2
                                 Allocator profiles for measured server process. Default: default.
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
  assert.deepEqual(parseArgs(['--allocator-profiles', 'default,arena1']).allocatorProfiles, ['default', 'arena1']);
  assert.deepEqual(parseArgs(['--allocator-profiles', 'arena2']).allocatorProfiles, ['arena2']);
  assert.deepEqual(allocatorProfileEnv('default'), {});
  assert.deepEqual(allocatorProfileEnv('arena1'), { MALLOC_ARENA_MAX: '1' });
  assert.deepEqual(allocatorProfileReport('arena2', {}), {
    name: 'arena2',
    env: { MALLOC_ARENA_MAX: '2' },
    scrubbedAllocatorEnvKeys: [],
    overriddenAllocatorEnvKeys: [],
  });
  const inheritedEnvFixture = Object.fromEntries(
    MEASURED_SERVER_ENV_ALLOWLIST.map((key) => [key, `allowed-${key}`])
  );
  const allocatorEnvFixture = Object.fromEntries(
    ALLOCATOR_ENV_KEYS.map((key) => [key, `allocator-${key}`])
  );
  assert.deepEqual(
    buildMeasuredServerEnv({}, {
      ...inheritedEnvFixture,
      ...allocatorEnvFixture,
      GITHUB_TOKEN: 'secret-token',
      HTTP_PROXY: 'http://proxy.example',
      HTTPS_PROXY: 'https://proxy.example',
      CI: 'true',
      SSH_AUTH_SOCK: '/tmp/ssh-agent.sock',
    }),
    inheritedEnvFixture
  );
  assert.deepEqual(
    buildMeasuredServerEnv({ MALLOC_ARENA_MAX: '1' }, {
      PATH: '/usr/bin',
      MALLOC_ARENA_MAX: '8',
    }),
    { PATH: '/usr/bin', MALLOC_ARENA_MAX: '1' }
  );
  assert.deepEqual(
    allocatorProfileReport('default', {
      PATH: '/usr/bin',
      ...allocatorEnvFixture,
      GITHUB_TOKEN: 'secret-token',
    }),
    {
      name: 'default',
      env: {},
      scrubbedAllocatorEnvKeys: ALLOCATOR_ENV_KEYS,
      overriddenAllocatorEnvKeys: [],
    }
  );
  assert.deepEqual(
    allocatorProfileReport('arena1', {
      PATH: '/usr/bin',
      ...allocatorEnvFixture,
      GITHUB_TOKEN: 'secret-token',
    }),
    {
      name: 'arena1',
      env: { MALLOC_ARENA_MAX: '1' },
      scrubbedAllocatorEnvKeys: ALLOCATOR_ENV_KEYS.filter((key) => key !== 'MALLOC_ARENA_MAX'),
      overriddenAllocatorEnvKeys: ['MALLOC_ARENA_MAX'],
    }
  );
  assert.deepEqual(buildMeasurementContext({
    smoke: false,
    fixtureScale: 'short',
    modes: ['dev'],
    fixtures: ['prefix'],
    runs: ['cold'],
    allocatorProfiles: ['default'],
    port: 65535,
  }).measuredServerEnvPolicy, {
    inheritedEnvKeys: ['PATH', 'HOME', 'TMPDIR', 'TMP', 'TEMP', 'LANG', 'LC_ALL', 'LC_CTYPE', 'RUST_BACKTRACE'],
    allocatorEnvScrubTargetKeys: ALLOCATOR_ENV_KEYS,
    reportPolicy: 'reports allocator env key names needed for interpretation, never parent env values',
  });
  assert.throws(
    () => allocatorProfileEnv('unsupported'),
    /unsupported allocator profile/
  );
  assert.throws(
    () => parseArgs(['--allocator-profiles', 'jemalloc']),
    /--allocator-profiles must be one of/
  );
  assert.throws(
    () => parseArgs(['--allocator-profiles', '']),
    /requires at least one value/
  );

  assert.equal(parseArgs(['--self-test']).selfTest, true);
  assert.equal(parseArgs(['--self-test-sanitization']).selfTest, true);
  assert.equal(parseArgs(['--self-test-sanitization']).selfTestSanitizationAlias, true);
  assert.equal(parseArgs(['--timeline']).timeline, true);
  assert.equal(parseArgs(['--timeline', '--strict']).strict, true);
  assert.deepEqual(parseArgs(['--timeline', '--fixture-density', 'dense']).fixtureDensities, ['dense']);
  assert.deepEqual(parseArgs(['--timeline', '--fixture-density', 'sparse']).fixtureDensities, ['sparse']);
  assert.deepEqual(parseArgs(['--timeline', '--fixture-density', 'dense,sparse']).fixtureDensities, ['dense', 'sparse']);
  assert.deepEqual(parseArgs(['--timeline', '--settled-delays', '1s,5s']).settledDelaysMs, [1000, 5000]);
  assert.deepEqual(parseArgs(['--timeline', '--settled-delays', '1s,5s,15s']).settledDelaysMs, [1000, 5000, 15000]);
  assert.throws(() => parseArgs(['--smoke', '--timeline']), /--smoke cannot be combined/);
  assert.throws(() => parseArgs(['--smoke', '--fixture-density', 'dense']), /--smoke cannot be combined/);
  assert.throws(() => parseArgs(['--smoke', '--settled-delays', '1s,5s']), /--smoke cannot be combined/);
  assert.throws(() => parseArgs(['--strict']), /--strict requires --timeline/);
  assert.throws(() => parseArgs(['--fixture-density', 'dense']), /--fixture-density requires --timeline/);
  assert.throws(() => parseArgs(['--settled-delays', '1s,5s']), /--settled-delays requires --timeline/);
  assert.throws(() => parseArgs(['--timeline', '--fixture-density', 'sparse,dense']), /--fixture-density must be dense, sparse, or dense,sparse/);
  assert.throws(() => parseArgs(['--timeline', '--fixture-density', 'dense,dense']), /--fixture-density must be dense, sparse, or dense,sparse/);
  assert.throws(() => parseArgs(['--timeline', '--fixture-density', '']), /requires a value/);
  assert.throws(() => parseArgs(['--timeline', '--fixtures', 'multifile', '--fixture-density', 'sparse']), /sparse density is only supported with --fixtures prefix/);
  assert.throws(() => parseArgs(['--timeline', '--fixtures', 'fallback', '--fixture-density', 'dense,sparse']), /sparse density is only supported with --fixtures prefix/);
  assert.throws(() => parseArgs(['--timeline', '--settled-delays', '5s,1s']), /--settled-delays must be 1s,5s or 1s,5s,15s/);
  assert.throws(() => parseArgs(['--timeline', '--settled-delays', '15s']), /--settled-delays must be 1s,5s or 1s,5s,15s/);

  const matrixOptionPairs = [
    ['--modes', 'release'],
    ['--fixtures', 'prefix'],
    ['--runs', 'cold'],
    ['--fixture-scale', 'short'],
    ['--allocator-profiles', 'default'],
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
  assert.deepEqual(smokeOptions.allocatorProfiles, ['default']);

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
  assert.deepEqual(
    summarizeProcCompleteness([{
      status: { available: true, VmRSS: 1000, RssFile: 100, RssShmem: 0 },
      smapsRollup: { available: true },
      maps: { available: true, anonymousKb: 0, fileBackedKb: 0, heapKb: 0, stackKb: 0, otherSpecialKb: 0 },
    }]),
    {
      procComplete: false,
      partialMeasurementReasons: [
        'status:missing_field:RssAnon',
        'smapsRollup:missing_field:Anonymous',
        'maps:missing_field:mappingCount',
      ],
    }
  );

  const eventStarted = namedSnapshot('request_started', 0, {
    status: { available: true, VmRSS: 1000, RssAnon: 800, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 790 },
    maps: { available: true, mappingCount: 1 },
  }, 'event', null);
  const samplePeak = namedSnapshot('sample_0000', 80, {
    status: { available: true, VmRSS: 5000, RssAnon: 4500, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 4480 },
    maps: { available: true, mappingCount: 1 },
  }, 'sampling', 0);
  const eventHeaders = namedSnapshot('headers_received', 75, {
    status: { available: true, VmRSS: 3000, RssAnon: 2500, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 2480 },
    maps: { available: true, mappingCount: 1 },
  }, 'event', null);
  const eventBody = namedSnapshot('body_received', 100, {
    status: { available: true, VmRSS: 2000, RssAnon: 1500, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 1490 },
    maps: { available: true, mappingCount: 1 },
  }, 'event', null);
  const bodyReceivedFromCallback = bodyReceivedSnapshotFromCallback(123.456, {
    status: { available: true, VmRSS: 2100, RssAnon: 1600, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 1590 },
    maps: { available: true, mappingCount: 1 },
  });
  assert.equal(bodyReceivedFromCallback.name, 'body_received');
  assert.equal(bodyReceivedFromCallback.elapsedMs, 123.456);
  assert.equal(bodyReceivedFromCallback.capturedFrom, 'event');
  const eventSettled = namedSnapshot('settled_1s', 1100, {
    status: { available: true, VmRSS: 1600, RssAnon: 1200, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 1190 },
    maps: { available: true, mappingCount: 1 },
  }, 'event', null);
  const timeline = buildTimelineReport({
    eventSnapshots: [eventStarted, eventHeaders, eventBody, eventSettled],
    sampleSnapshots: [samplePeak],
    settledDelaysMs: [1000],
    bodyReadStartElapsedMs: 76,
    bodyReadEndElapsedMs: 100,
    responseBytes: 1234,
  });
  assert.equal(timeline.peaks.requestPeakRss.name, 'sample_0000');
  assert.equal(timeline.peaks.requestPeakAnon.name, 'sample_0000');
  assert.equal(timeline.peaks.bodyDrainPeakRss.name, 'sample_0000');
  assert.equal(timeline.peaks.bodyDrainPeakAnon.name, 'sample_0000');
  assert.equal(timeline.derived.request_started_to_headers_delta_kb, 1700);
  assert.equal(timeline.derived.headers_to_body_delta_kb, -1000);
  assert.equal(timeline.derived.headers_to_body_peak_delta_kb, 2000);
  assert.equal(timeline.derived.body_peak_to_body_received_delta_kb, 3000);
  assert.deepEqual(timeline.derived.settledComparisons, [{
    settledDelayMs: 1000,
    settledSnapshotName: 'settled_1s',
    peak_to_settled_delta_kb: 3300,
    peak_to_settled_ratio: 3.75,
  }]);
  const baselinePeakTimeline = buildTimelineReport({
    eventSnapshots: [
      namedSnapshot('request_started', 0, {
        status: { available: true, VmRSS: 5000, RssAnon: 4500, RssFile: 100, RssShmem: 0 },
        smapsRollup: { available: true, Anonymous: 4480 },
        maps: { available: true, mappingCount: 1 },
      }, 'event', null),
      eventHeaders,
      eventBody,
      eventSettled,
    ],
    sampleSnapshots: [],
    settledDelaysMs: [1000],
    bodyReadStartElapsedMs: 76,
    bodyReadEndElapsedMs: 100,
    responseBytes: 1234,
  });
  assert.equal(baselinePeakTimeline.peaks.requestPeakAnon.name, 'request_started');
  assert.equal(baselinePeakTimeline.derived.settledComparisons[0].excludedReason, 'peak_is_request_baseline');
  assert.equal(timeline.samplingSummary.samplingIntervalMs, SAMPLE_INTERVAL_MS);
  assert.equal(timeline.samplingSummary.clock, 'monotonic');
  assert.equal(timeline.samplingSummary.sampleCount, 1);
  assert.deepEqual(timeline.samplingSummary.droppedSampleReasons, ['raw_samples_omitted']);

  const sampleBeforeHeaders = namedSnapshot('sample_before_headers', 60, {
    status: { available: true, VmRSS: 9000, RssAnon: 8800, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 8780 },
    maps: { available: true, mappingCount: 1 },
  }, 'sampling', 0);
  const sampleAfterHeaders = namedSnapshot('sample_after_headers', 90, {
    status: { available: true, VmRSS: 4200, RssAnon: 4100, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 4080 },
    maps: { available: true, mappingCount: 1 },
  }, 'sampling', 1);
  const bodyDrainTimeline = buildTimelineReport({
    eventSnapshots: [eventStarted, eventHeaders, eventBody, eventSettled],
    sampleSnapshots: [sampleBeforeHeaders, sampleAfterHeaders],
    settledDelaysMs: [1000],
    bodyReadStartElapsedMs: 76,
    bodyReadEndElapsedMs: 100,
    responseBytes: 1234,
  });
  assert.equal(bodyDrainTimeline.peaks.bodyDrainPeakAnon.name, 'sample_after_headers');
  const partialSample = namedSnapshot('sample_missing_maps', 80, {
    status: { available: true, VmRSS: 5000, RssAnon: 4500, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 4480 },
    maps: { available: false, reason: 'missing' },
  }, 'sampling', 0);
  const partialSampleTimeline = buildTimelineReport({
    eventSnapshots: [eventStarted, eventHeaders, eventBody, eventSettled],
    sampleSnapshots: [partialSample],
    settledDelaysMs: [1000],
    bodyReadStartElapsedMs: 76,
    bodyReadEndElapsedMs: 100,
    responseBytes: 1234,
  });
  assert.equal(partialSampleTimeline.peaks.requestPeakRss.name, 'sample_missing_maps');
  assert.deepEqual(procCompletenessForSnapshots([eventStarted, eventHeaders, eventBody, eventSettled, partialSample]), {
    procComplete: false,
    partialMeasurementReasons: ['maps:missing'],
  });
  assert.deepEqual(statusForProcCompleteness(procCompletenessForSnapshots([
    eventStarted,
    eventHeaders,
    eventBody,
    eventSettled,
    partialSample,
  ])), {
    status: 'partial',
    partialMeasurementReasons: ['maps:missing'],
    decisionExcludedReason: 'partial_proc_measurement',
  });
  assert.deepEqual(statusForProcCompleteness({ procComplete: true, partialMeasurementReasons: [] }), { status: 'ok' });
  assert.deepEqual(statusForProcCompleteness({ procComplete: false, partialMeasurementReasons: ['maps:missing'] }), {
    status: 'partial',
    partialMeasurementReasons: ['maps:missing'],
    decisionExcludedReason: 'partial_proc_measurement',
  });
  assert.equal(measurementExitCode({ failed: false, strict: false, reports: [{ status: 'partial' }] }), 0);
  assert.equal(measurementExitCode({ failed: false, strict: true, reports: [{ status: 'partial' }] }), 1);
  const fixtureFailureReport = failedScenarioReport({
    fixtureKind: 'prefix',
    fixtureDensity: 'dense',
    scenarioId: 'prefix-short-dense',
    mode: 'dev',
    runKind: 'cold',
    allocatorProfile: 'default',
    error: new Error('fixture failed: synthetic'),
    errorCode: 'fixture_failed',
  });
  assert.equal(fixtureFailureReport.status, 'failed');
  assert.equal(fixtureFailureReport.scenarioError.errorCode, 'fixture_failed');
  assert.equal(Object.hasOwn(fixtureFailureReport.scenarioError, 'rawStderr'), false);
  assert.equal(Object.hasOwn(fixtureFailureReport, 'errorKind'), false);
  const cleanupFailureReport = failedScenarioReport({
    fixtureKind: 'prefix',
    fixtureDensity: 'dense',
    scenarioId: 'prefix-short-dense',
    mode: 'dev',
    runKind: 'cold',
    allocatorProfile: 'default',
    error: attachCleanupFailure(new Error('fixture failed: synthetic'), 12345),
    errorCode: 'fixture_failed',
  });
  assert.equal(cleanupFailureReport.scenarioError.cleanupFailed, true);
  assert.equal(cleanupFailureReport.scenarioError.cleanupErrorKind, 'server_stop_failed');
  assert.equal(cleanupFailureReport.scenarioError.cleanupMessage, 'server stop failed for pid 12345');
  assert.equal(Object.hasOwn(cleanupFailureReport, 'cleanupFailed'), false);
  const legacyFailureReport = failedScenarioReport({
    fixtureKind: 'prefix',
    fixtureDensity: 'dense',
    scenarioId: 'prefix-short-dense',
    mode: 'dev',
    runKind: 'cold',
    allocatorProfile: 'default',
    error: new Error('legacy failure'),
    errorCode: 'measurement_failed',
    includeLegacyErrorFields: true,
  });
  assert.equal(legacyFailureReport.errorKind, 'measurement_failed');
  assert.equal(legacyFailureReport.sanitizedMessage, 'legacy failure');
  assert.equal(legacyFailureReport.scenarioError.errorCode, 'measurement_failed');
  assert.deepEqual(
    scenarioWithCleanupFailure({
      mode: 'release',
      runKind: 'cold',
      allocatorProfile: allocatorProfileReport('default'),
      status: 'ok',
      timeline,
    }, 12345),
    {
      mode: 'release',
      runKind: 'cold',
      allocatorProfile: allocatorProfileReport('default'),
      status: 'partial',
      timeline,
      cleanupFailed: true,
      cleanupErrorKind: 'server_stop_failed',
      cleanupMessage: 'server stop failed for pid 12345',
      decisionExcludedReason: 'server_cleanup_failed',
    }
  );
  const summaryReports = [
    {
      status: 'ok',
      fixtureKind: 'prefix',
      fixtureDensity: 'dense',
      scenarioId: 'prefix-full-dense',
      mode: 'release',
      runKind: 'cold',
      allocatorProfile: { name: 'default' },
      timeline: {
        snapshots: [
          { name: 'settled_1s', status: { available: true, RssAnon: 5000 } },
          { name: 'settled_5s', status: { available: true, RssAnon: 4500 } },
        ],
        derived: {
          settledComparisons: [
            { settledDelayMs: 1000, settledSnapshotName: 'settled_1s', peak_to_settled_delta_kb: 1000, peak_to_settled_ratio: 1.2 },
            { settledDelayMs: 5000, settledSnapshotName: 'settled_5s', peak_to_settled_delta_kb: 1500, peak_to_settled_ratio: 1.33 },
          ],
          headers_to_body_peak_delta_kb: 700,
        },
      },
    },
    {
      status: 'ok',
      fixtureKind: 'prefix',
      fixtureDensity: 'dense',
      scenarioId: 'prefix-full-dense',
      mode: 'release',
      runKind: 'cold',
      allocatorProfile: { name: 'arena1' },
      timeline: {
        snapshots: [
          { name: 'settled_1s', status: { available: true, RssAnon: 3000 } },
          { name: 'settled_5s', status: { available: true, RssAnon: 2500 } },
        ],
        derived: {
          settledComparisons: [
            { settledDelayMs: 1000, settledSnapshotName: 'settled_1s', peak_to_settled_delta_kb: 800, peak_to_settled_ratio: 1.26 },
            { settledDelayMs: 5000, settledSnapshotName: 'settled_5s', peak_to_settled_delta_kb: 1100, peak_to_settled_ratio: 1.44 },
          ],
          headers_to_body_peak_delta_kb: 500,
        },
      },
    },
  ];
  const reportSummary = buildReportSummary(summaryReports, { fixtureScale: 'full' });
  assert.equal(reportSummary.acceptanceStatus, 'full');
  assert.equal(reportSummary.fullAcceptanceMet, true);
  assert.equal(reportSummary.comparisons[0].default_vs_arena1_settled_delta_kb, 2000);
  assert.equal(reportSummary.comparisons[0].comparisonStatus, 'ok');
  assert.deepEqual(reportSummary.scenarioComparisons, []);
  const shortDevSummary = buildReportSummary(summaryReports.map((report) => ({
    ...structuredClone(report),
    mode: 'dev',
    scenarioId: 'prefix-short-dense',
  })), { fixtureScale: 'short' });
  assert.equal(shortDevSummary.acceptanceStatus, 'partial');
  assert.equal(shortDevSummary.fullAcceptanceMet, false);
  const missingArenaSummary = buildReportSummary([summaryReports[0]], { fixtureScale: 'full' });
  assert.equal(missingArenaSummary.acceptanceStatus, 'partial');
  assert.equal(missingArenaSummary.fullAcceptanceMet, false);
  const partialComparisonReports = structuredClone(summaryReports);
  delete partialComparisonReports[1].timeline.snapshots[0].status.RssAnon;
  const partialComparisonSummary = buildReportSummary(partialComparisonReports, { fixtureScale: 'full' });
  assert.equal(partialComparisonSummary.acceptanceStatus, 'partial');
  assert.equal(partialComparisonSummary.fullAcceptanceMet, false);
  const arena2OnlySummary = buildReportSummary([
    {
      ...summaryReports[0],
      allocatorProfile: { name: 'arena2' },
    },
  ]);
  assert.equal(arena2OnlySummary.acceptanceStatus, 'partial');
  assert.equal(arena2OnlySummary.fullAcceptanceMet, false);
  assert.deepEqual(arena2OnlySummary.acceptanceReasons, ['missing_required_allocator_comparison']);
  assert.equal(buildReportSummary([{ status: 'ok' }]).acceptanceStatus, 'not_applicable');
  assert.equal(buildReportSummary([{ status: 'ok' }]).fullAcceptanceMet, false);
  assert.equal(buildReportSummary([{ status: 'ok' }]).decisionExcludedReason, 'timeline_not_requested');
  assert.equal(buildReportSummary([{ status: 'partial' }]).acceptanceStatus, 'partial');
  assert.equal(buildReportSummary([{ status: 'partial' }]).fullAcceptanceMet, false);
  assert.equal(buildReportSummary([{ status: 'failed' }]).acceptanceStatus, 'failed');
  assert.equal(buildReportSummary([{ status: 'failed' }]).fullAcceptanceMet, false);
  const scenarioComparisonReports = [
    ...summaryReports,
    {
      ...structuredClone(summaryReports[0]),
      scenarioId: 'prefix-full-sparse',
      fixtureDensity: 'sparse',
      timeline: {
        ...structuredClone(summaryReports[0].timeline),
        derived: {
          settledComparisons: [
            { settledDelayMs: 1000, settledSnapshotName: 'settled_1s', peak_to_settled_delta_kb: 3000, peak_to_settled_ratio: 1.8 },
            { settledDelayMs: 5000, settledSnapshotName: 'settled_5s', peak_to_settled_delta_kb: 2500, peak_to_settled_ratio: 1.7 },
          ],
          headers_to_body_peak_delta_kb: 1200,
        },
      },
    },
    {
      ...structuredClone(summaryReports[1]),
      scenarioId: 'prefix-full-sparse',
      fixtureDensity: 'sparse',
      timeline: {
        ...structuredClone(summaryReports[1].timeline),
        derived: {
          settledComparisons: [
            { settledDelayMs: 1000, settledSnapshotName: 'settled_1s', peak_to_settled_delta_kb: 1700, peak_to_settled_ratio: 1.5 },
            { settledDelayMs: 5000, settledSnapshotName: 'settled_5s', peak_to_settled_delta_kb: 1600, peak_to_settled_ratio: 1.4 },
          ],
          headers_to_body_peak_delta_kb: 700,
        },
      },
    },
  ];
  const scenarioSummary = buildReportSummary(scenarioComparisonReports, { fixtureScale: 'full' });
  assert.equal(scenarioSummary.scenarioComparisons.length, 6);
  assert.equal(scenarioSummary.scenarioComparisons[0].comparisonStatus, 'ok');
  assert.equal(scenarioSummary.scenarioComparisons[2].metric, 'headers_to_body_peak_delta_kb');
  const partialScenarioReports = structuredClone(scenarioComparisonReports);
  delete partialScenarioReports[2].timeline.derived.headers_to_body_peak_delta_kb;
  const partialScenarioSummary = buildReportSummary(partialScenarioReports, { fixtureScale: 'full' });
  assert.equal(partialScenarioSummary.acceptanceStatus, 'partial');
  assert.equal(partialScenarioSummary.fullAcceptanceMet, false);
  assert.deepEqual(partialScenarioSummary.acceptanceReasons, ['scenario_comparison_not_ok']);
  const baselineExcludedScenarioReports = structuredClone(scenarioComparisonReports);
  baselineExcludedScenarioReports[0].timeline.derived.settledComparisons[0].excludedReason = 'peak_is_request_baseline';
  const baselineExcludedScenarioSummary = buildReportSummary(baselineExcludedScenarioReports, { fixtureScale: 'full' });
  assert.equal(baselineExcludedScenarioSummary.acceptanceStatus, 'partial');
  assert.equal(baselineExcludedScenarioSummary.fullAcceptanceMet, false);
  assert.equal(baselineExcludedScenarioSummary.scenarioComparisons[0].comparisonStatus, 'skipped');
  assert.equal(baselineExcludedScenarioSummary.scenarioComparisons[0].excludedReason, 'base_peak_is_request_baseline');
  assert.deepEqual(baselineExcludedScenarioSummary.acceptanceReasons, ['scenario_comparison_not_ok']);
  const cleanupPartialSummary = buildReportSummary([{ status: 'partial', decisionExcludedReason: 'server_cleanup_failed' }]);
  assert.deepEqual(cleanupPartialSummary.acceptanceReasons, ['report_partial', 'server_cleanup_failed']);
  const procPartialSummary = buildReportSummary([{
    status: 'partial',
    decisionExcludedReason: 'partial_proc_measurement',
    partialMeasurementReasons: ['maps:missing'],
  }]);
  assert.deepEqual(procPartialSummary.acceptanceReasons, ['report_partial', 'partial_proc_measurement', 'maps:missing']);
  assert.equal(
    buildScenarioComparisonEntry({
      base: summaryReports[0],
      compare: summaryReports[0],
      baseScenarioId: 'prefix-full-dense',
      compareScenarioId: 'prefix-full-sparse',
      metric: 'peak_to_settled_delta_kb',
      settledDelayMs: 1000,
      settledSnapshotName: 'settled_1s',
      baseMetric: null,
      compareMetric: 1,
    }).excludedReason,
    'missing_or_invalid_base_metric'
  );
  assert.equal(
    buildScenarioComparisonEntry({
      base: summaryReports[0],
      compare: summaryReports[0],
      baseScenarioId: 'prefix-full-dense',
      compareScenarioId: 'prefix-full-sparse',
      metric: 'peak_to_settled_delta_kb',
      settledDelayMs: 1000,
      settledSnapshotName: 'settled_1s',
      baseMetric: 1,
      compareMetric: null,
    }).excludedReason,
    'missing_or_invalid_compare_metric'
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
  assert.equal(context.cliOptions.timeline, false);
  assert.equal(context.cliOptions.strict, false);
  assert.deepEqual(context.cliOptions.fixtureDensities, ['dense']);
  assert.deepEqual(context.cliOptions.settledDelaysMs, [1000, 5000]);
  const allocatorContext = buildMeasurementContext(parseArgs(['--allocator-profiles', 'default,arena1']));
  assert.deepEqual(allocatorContext.cliOptions.allocatorProfiles, ['default', 'arena1']);

  assert.deepEqual(readMapsSummaryFromText('invalid maps line'), {
    available: false,
    reason: 'parse_failed',
    malformedLineCount: 1,
    mappingCount: 0,
  });

  const root = createFixtureRoot();
  try {
    const prefixDense = createFixture(root, 'prefix', 'short', 'dense');
    assert.equal(prefixDense.fixtureKind, 'prefix');
    assert.equal(prefixDense.fixtureDensity, 'dense');
    assert.equal(prefixDense.scenarioId, 'prefix-short-dense');
    assert.equal(prefixDense.fileCount, 1);
    assert.ok(prefixDense.bytes > 0);
    assert.equal(prefixDense.maskedWorkspace.startsWith(MASKED_TEMP), true);

    const prefixSparse = createFixture(root, 'prefix', 'short', 'sparse');
    assert.equal(prefixSparse.fixtureKind, 'prefix');
    assert.equal(prefixSparse.fixtureDensity, 'sparse');
    assert.equal(prefixSparse.scenarioId, 'prefix-short-sparse');
    assert.equal(prefixSparse.fileCount, 1);
    assert.ok(prefixSparse.bytes > 0);
    assert.ok(prefixSparse.bytes >= Math.floor(prefixDense.bytes * 0.95));
    assert.ok(prefixSparse.bytes <= Math.ceil(prefixDense.bytes * 1.05));
    const sparseFiles = collectMarkdownFiles(prefixSparse.workspace);
    assert.equal(sparseFiles.length, 1);
    const sparseNeedleCount = (readFileSync(sparseFiles[0], 'utf8').match(/needle/g) ?? []).length;
    assert.ok(sparseNeedleCount >= 101);

    const multifile = createFixture(root, 'multifile', 'short', 'dense');
    assert.equal(multifile.fixtureKind, 'multifile');
    assert.equal(multifile.fixtureDensity, 'dense');
    assert.equal(multifile.scenarioId, 'multifile-short-dense');
    assert.equal(multifile.fileCount, 8);
    assert.ok(multifile.bytes > 0);

    const fallback = createFixture(root, 'fallback', 'short', 'dense');
    assert.equal(fallback.fixtureKind, 'fallback');
    assert.equal(fallback.fixtureDensity, 'dense');
    assert.equal(fallback.scenarioId, 'fallback-short-dense');
    assert.equal(fallback.fileCount, 1);
    assert.ok(fallback.bytes > 0);

    const fullFallback = createFixture(root, 'fallback', 'full', 'dense');
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

function createFixture(root, fixtureKind, scale, density = 'dense') {
  const workspace = path.join(root, `${fixtureKind}-${scale}-${density}-workspace`);
  mkdirSync(workspace, { recursive: true });

  if (fixtureKind === 'prefix') {
    return createPrefixFixture(workspace, scale, density);
  }
  if (fixtureKind === 'multifile' && density === 'dense') {
    return createMultifileFixture(workspace, scale, density);
  }
  if (fixtureKind === 'fallback' && density === 'dense') {
    return createFallbackFixture(workspace, scale, density);
  }
  throw new Error(`unsupported fixture combination: ${fixtureKind}/${scale}/${density}`);
}

function createPrefixFixture(workspace, scale, density = 'dense') {
  const repeatCount = scale === 'full' ? 180_000 : 1_200;
  const filePath = path.join(workspace, 'prefix.md');
  const paragraphs = [];
  if (density === 'dense') {
    for (let index = 0; index < repeatCount; index += 1) {
      paragraphs.push(`needle paragraph ${index}`);
    }
  } else if (density === 'sparse') {
    for (let index = 0; index < repeatCount; index += 1) {
      const marker = index % Math.max(1, Math.floor(repeatCount / 120)) === 0 ? 'needle' : 'filler';
      paragraphs.push(`${marker} paragraph ${index}`);
    }
  } else {
    throw new Error(`unsupported prefix density: ${density}`);
  }
  writeFileSync(filePath, `${paragraphs.join('\n\n')}\n`, 'utf8');
  return summarizeFixture(workspace, 'prefix', scale, density);
}

function createMultifileFixture(workspace, scale, density = 'dense') {
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
  return summarizeFixture(workspace, 'multifile', scale, density);
}

function createFallbackFixture(workspace, scale, density = 'dense') {
  const repeatCount = scale === 'full' ? 300_000 : 4_000;
  const filePath = path.join(workspace, 'fallback.md');
  const chunk = 'needle_inside_single_large_block ';
  writeFileSync(filePath, `# fallback\n\n${chunk.repeat(repeatCount)}\n`, 'utf8');
  return summarizeFixture(workspace, 'fallback', scale, density);
}

function summarizeFixture(workspace, fixtureKind, scale, density = 'dense') {
  const files = collectMarkdownFiles(workspace);
  const bytes = files.reduce((sum, filePath) => sum + statSync(filePath).size, 0);
  return {
    fixtureKind,
    fixtureDensity: density,
    scenarioId: `${fixtureKind}-${scale}-${density}`,
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

function namedSnapshot(name, elapsedMs, procSnapshot, capturedFrom = 'event', sampleIndex = null) {
  return {
    name,
    elapsedMs,
    capturedFrom,
    sampleIndex,
    status: procSnapshot.status,
    smapsRollup: procSnapshot.smapsRollup,
    maps: procSnapshot.maps,
    procComplete: summarizeProcCompleteness([procSnapshot]).procComplete,
  };
}

function bodyReceivedSnapshotFromCallback(bodyReadEndElapsedMs, procSnapshot) {
  return namedSnapshot('body_received', bodyReadEndElapsedMs, procSnapshot, 'event', null);
}

function clonePeakSnapshot(snapshot, phase, metric) {
  if (!snapshot) {
    return null;
  }
  return {
    phase,
    metric,
    capturedFrom: snapshot.capturedFrom,
    sampleIndex: snapshot.sampleIndex,
    name: snapshot.name,
    elapsedMs: snapshot.elapsedMs,
    status: snapshot.status,
    smapsRollup: snapshot.smapsRollup,
    maps: snapshot.maps,
    procComplete: snapshot.procComplete,
  };
}

function pickPeakSnapshot(snapshots, fieldName) {
  const candidates = snapshots.filter((snapshot) => Number.isFinite(snapshot?.status?.[fieldName]));
  if (candidates.length === 0) {
    return null;
  }
  return candidates.reduce((best, candidate) => (
    candidate.status[fieldName] > best.status[fieldName] ? candidate : best
  ));
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

async function runMeasuredScenario(options, fixture, mode, runKind, allocatorProfile) {
  const allocator = allocatorProfileReport(allocatorProfile);
  const server = await startServer({
    mode,
    workspace: fixture.workspace,
    port: options.port,
    allocatorEnv: allocator.env,
  });
  let scenarioError = null;
  let scenarioResult = null;
  try {
    await waitForServer(server, options.port);
    let warmupResponse = null;
    if (runKind === 'warm') {
      warmupResponse = await requestSearch(options.port, options.query, REQUEST_TIMEOUT_MS);
      validateScenarioResult(fixture.fixtureKind, warmupResponse);
    }
    if (options.timeline) {
      const startedAt = process.hrtime.bigint();
      const eventSnapshots = [];
      const sampleSnapshots = [];
      const elapsed = () => Number(process.hrtime.bigint() - startedAt) / 1_000_000;
      eventSnapshots.push(namedSnapshot('server_ready', elapsed(), readProcSnapshot(server.pid)));
      const requestStartedElapsedMs = elapsed();
      eventSnapshots.push(namedSnapshot('request_started', requestStartedElapsedMs, readProcSnapshot(server.pid)));
      let headersCaptured = false;
      let bodyReceivedCaptured = false;
      const requestPromise = trackPromise(requestSearchTimeline(
        options.port,
        options.query,
        REQUEST_TIMEOUT_MS,
        () => {
          headersCaptured = true;
          eventSnapshots.push(namedSnapshot('headers_received', elapsed(), readProcSnapshot(server.pid)));
        },
        ({ bodyReadEndElapsedMs }) => {
          bodyReceivedCaptured = true;
          eventSnapshots.push(bodyReceivedSnapshotFromCallback(
            requestStartedElapsedMs + bodyReadEndElapsedMs,
            readProcSnapshot(server.pid)
          ));
        }
      ));
      let sampleIndex = 0;
      while (!isPromiseSettled(requestPromise) && sampleSnapshots.length < MAX_SAMPLES) {
        sampleSnapshots.push(namedSnapshot(
          `sample_${String(sampleIndex).padStart(4, '0')}`,
          elapsed(),
          readProcSnapshot(server.pid),
          'sampling',
          sampleIndex
        ));
        sampleIndex += 1;
        const samplingWaitResult = await Promise.race([
          requestPromise.then(
            () => 'request_settled',
            () => 'request_settled'
          ),
          delay(SAMPLE_INTERVAL_MS).then(() => 'sample_interval_elapsed'),
        ]);
        if (samplingWaitResult === 'request_settled') {
          break;
        }
      }
      if (!isPromiseSettled(requestPromise)) {
        requestPromise.abort?.();
        throw new Error('request sampling limit exceeded');
      }
      const response = await requestPromise;
      validateScenarioResult(fixture.fixtureKind, response);
      const bodyReadStartElapsedMs = requestStartedElapsedMs + response.bodyReadStartElapsedMs;
      const bodyReadEndElapsedMs = requestStartedElapsedMs + response.bodyReadEndElapsedMs;
      if (!headersCaptured) {
        eventSnapshots.push(namedSnapshot('headers_received', bodyReadStartElapsedMs, readProcSnapshot(server.pid)));
      }
      if (!bodyReceivedCaptured) {
        eventSnapshots.push(bodyReceivedSnapshotFromCallback(bodyReadEndElapsedMs, readProcSnapshot(server.pid)));
      }
      for (const delayMs of options.settledDelaysMs) {
        const elapsedSinceBodyReceived = elapsed() - bodyReadEndElapsedMs;
        if (elapsedSinceBodyReceived < delayMs) {
          await delay(delayMs - elapsedSinceBodyReceived);
        }
        eventSnapshots.push(namedSnapshot(`settled_${delayMs / 1000}s`, elapsed(), readProcSnapshot(server.pid)));
      }
      const timeline = buildTimelineReport({
        eventSnapshots,
        sampleSnapshots,
        settledDelaysMs: options.settledDelaysMs,
        bodyReadStartElapsedMs,
        bodyReadEndElapsedMs,
        responseBytes: response.responseBytes,
      });
      const responseReport = { ...response, bodyReadStartElapsedMs, bodyReadEndElapsedMs };
      const procCompleteness = procCompletenessForSnapshots([...eventSnapshots, ...sampleSnapshots]);
      scenarioResult = {
        mode,
        runKind,
        allocatorProfile: allocator,
        pid: server.pid,
        elapsedMs: bodyReadEndElapsedMs,
        peakRssKb: timeline.peaks.requestPeakRss?.status?.VmRSS ?? null,
        procComplete: procCompleteness.procComplete,
        partialMeasurementReasons: procCompleteness.partialMeasurementReasons,
        ...statusForProcCompleteness(procCompleteness),
        timeline,
        ...(warmupResponse ? { warmupResponse } : {}),
        response: responseReport,
      };
      return scenarioResult;
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
    scenarioResult = {
      mode,
      runKind,
      allocatorProfile: allocator,
      pid: server.pid,
      elapsedMs: Number(endedAt - startedAt) / 1_000_000,
      peakRssKb: peakRssKb(snapshots),
      procComplete: procCompleteness.procComplete,
      partialMeasurementReasons: procCompleteness.partialMeasurementReasons,
      ...statusForProcCompleteness(procCompleteness),
      before,
      after,
      settled,
      ...(warmupResponse ? { warmupResponse } : {}),
      response,
    };
    return scenarioResult;
  } catch (error) {
    scenarioError = normalizeError(error);
    throw scenarioError;
  } finally {
    const stopped = await stopServer(server);
    if (!stopped) {
      if (scenarioError) {
        attachCleanupFailure(scenarioError, server.pid);
      } else if (scenarioResult) {
        scenarioWithCleanupFailure(scenarioResult, server.pid);
      } else {
        throw attachCleanupFailure(new Error('server stop failed'), server.pid);
      }
    }
  }
}

function scenarioWithCleanupFailure(scenario, pid) {
  return Object.assign(scenario, {
    status: 'partial',
    cleanupFailed: true,
    cleanupErrorKind: 'server_stop_failed',
    cleanupMessage: `server stop failed for pid ${pid}`,
    decisionExcludedReason: 'server_cleanup_failed',
  });
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

async function startServer({ mode, workspace, port, allocatorEnv = {} }) {
  await assertPortAvailable(port);
  const binaryPath = ensureBuiltBinary(mode);
  const child = spawn(binaryPath, [workspace, '--port', String(port), '--no-open'], {
    cwd: process.cwd(),
    env: buildMeasuredServerEnv(allocatorEnv),
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

function requestSearchTimeline(port, query, timeoutMs, onHeaders, onBodyReceived) {
  const controller = new AbortController();
  const timeout = setTimeout(() => {
    controller.abort();
  }, timeoutMs);
  const startedAt = process.hrtime.bigint();
  const promise = (async () => {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/api/search?q=${encodeURIComponent(query)}`, {
        headers: { Host: `127.0.0.1:${port}` },
        signal: controller.signal,
      });
      onHeaders?.(response);
      const bodyReadStartElapsedMs = Number(process.hrtime.bigint() - startedAt) / 1_000_000;
      const body = await readResponseTextWithLimit(response, MAX_RESPONSE_BYTES);
      const bodyReadEndElapsedMs = Number(process.hrtime.bigint() - startedAt) / 1_000_000;
      onBodyReceived?.({ bodyReadEndElapsedMs });
      const summary = summarizeSearchResponse(response, body, query);
      return { ...summary, bodyReadStartElapsedMs, bodyReadEndElapsedMs };
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
    const probes = {
      status: {
        probe: snapshot?.status,
        requiredFields: ['VmRSS', 'RssAnon', 'RssFile', 'RssShmem'],
      },
      smapsRollup: {
        probe: snapshot?.smapsRollup,
        requiredFields: ['Anonymous'],
      },
      maps: {
        probe: snapshot?.maps,
        requiredFields: ['mappingCount'],
      },
    };
    for (const [probeName, { probe, requiredFields }] of Object.entries(probes)) {
      if (probe && probe.available === false) {
        reasons.add(probe.code ? `${probeName}:${probe.reason}:${probe.code}` : `${probeName}:${probe.reason}`);
        continue;
      }
      for (const fieldName of requiredFields) {
        if (!Number.isFinite(probe?.[fieldName])) {
          reasons.add(`${probeName}:missing_field:${fieldName}`);
        }
      }
    }
  }
  return {
    procComplete: reasons.size === 0,
    partialMeasurementReasons: [...reasons],
  };
}

function snapshotByName(snapshots, name) {
  return snapshots.find((snapshot) => snapshot.name === name) ?? null;
}

function samplesInWindow(sampleSnapshots, startSnapshot, endSnapshot) {
  if (!startSnapshot || !endSnapshot) {
    return [];
  }
  return sampleSnapshots.filter((snapshot) => (
    Number.isFinite(snapshot.elapsedMs)
    && snapshot.elapsedMs >= startSnapshot.elapsedMs
    && snapshot.elapsedMs <= endSnapshot.elapsedMs
  ));
}

function deltaKb(left, right) {
  return Number.isFinite(left) && Number.isFinite(right) ? left - right : null;
}

function ratioOrNull(numerator, denominator) {
  return Number.isFinite(numerator) && Number.isFinite(denominator) && denominator > 0
    ? numerator / denominator
    : null;
}

function buildTimelineReport({
  eventSnapshots,
  sampleSnapshots,
  settledDelaysMs,
  bodyReadStartElapsedMs,
  bodyReadEndElapsedMs,
  responseBytes,
}) {
  const snapshots = [...eventSnapshots].sort((left, right) => left.elapsedMs - right.elapsedMs);
  const requestStarted = snapshotByName(snapshots, 'request_started');
  const headersReceived = snapshotByName(snapshots, 'headers_received');
  const bodyReceived = snapshotByName(snapshots, 'body_received');
  const requestWindowSamples = samplesInWindow(sampleSnapshots, requestStarted, bodyReceived);
  const bodyDrainWindowSamples = samplesInWindow(sampleSnapshots, headersReceived, bodyReceived);
  const requestWindow = [requestStarted, ...requestWindowSamples, headersReceived, bodyReceived].filter(Boolean);
  const bodyDrainWindow = [headersReceived, ...bodyDrainWindowSamples, bodyReceived].filter(Boolean);
  const requestPeakRss = pickPeakSnapshot(requestWindow, 'VmRSS');
  const requestPeakAnon = pickPeakSnapshot(requestWindow, 'RssAnon');
  const bodyDrainPeakRss = pickPeakSnapshot(bodyDrainWindow, 'VmRSS');
  const bodyDrainPeakAnon = pickPeakSnapshot(bodyDrainWindow, 'RssAnon');
  const settledComparisons = settledDelaysMs.map((delayMs) => {
    const settledSnapshotName = `settled_${delayMs / 1000}s`;
    const settled = snapshotByName(snapshots, settledSnapshotName);
    const peakAnon = requestPeakAnon?.status?.RssAnon;
    const settledAnon = settled?.status?.RssAnon;
    const peak_to_settled_delta_kb = deltaKb(peakAnon, settledAnon);
    const peak_to_settled_ratio = ratioOrNull(peakAnon, settledAnon);
    const entry = { settledDelayMs: delayMs, settledSnapshotName, peak_to_settled_delta_kb, peak_to_settled_ratio };
    if (requestPeakAnon?.name === 'request_started') {
      entry.excludedReason = 'peak_is_request_baseline';
    } else if (peak_to_settled_delta_kb === null || peak_to_settled_ratio === null) {
      entry.excludedReason = 'missing_or_invalid_peak_or_settled_anon';
    }
    return entry;
  });
  return {
    settledDelaysMs,
    snapshots,
    peaks: {
      requestPeakRss: clonePeakSnapshot(requestPeakRss, 'request', 'VmRSS'),
      requestPeakAnon: clonePeakSnapshot(requestPeakAnon, 'request', 'RssAnon'),
      bodyDrainPeakRss: clonePeakSnapshot(bodyDrainPeakRss, 'body_drain', 'VmRSS'),
      bodyDrainPeakAnon: clonePeakSnapshot(bodyDrainPeakAnon, 'body_drain', 'RssAnon'),
    },
    samplingSummary: {
      samplingIntervalMs: SAMPLE_INTERVAL_MS,
      clock: 'monotonic',
      sampleCount: sampleSnapshots.length,
      maxSamples: MAX_SAMPLES,
      missedSampleReasons: [],
      droppedSampleReasons: sampleSnapshots.length > 0 ? ['raw_samples_omitted'] : [],
    },
    bodyReadStartElapsedMs,
    bodyReadEndElapsedMs,
    responseBytes,
    derived: {
      settledComparisons,
      request_started_to_headers_delta_kb: deltaKb(headersReceived?.status?.RssAnon, requestStarted?.status?.RssAnon),
      headers_to_body_delta_kb: deltaKb(bodyReceived?.status?.RssAnon, headersReceived?.status?.RssAnon),
      headers_to_body_peak_delta_kb: deltaKb(bodyDrainPeakAnon?.status?.RssAnon, headersReceived?.status?.RssAnon),
      body_peak_to_body_received_delta_kb: deltaKb(bodyDrainPeakAnon?.status?.RssAnon, bodyReceived?.status?.RssAnon),
    },
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

function statusForProcCompleteness(procCompleteness) {
  if (procCompleteness.procComplete) {
    return { status: 'ok' };
  }
  return {
    status: 'partial',
    partialMeasurementReasons: procCompleteness.partialMeasurementReasons,
    decisionExcludedReason: 'partial_proc_measurement',
  };
}

function scenarioErrorFields(error, errorCode = classifyError(error)) {
  const fields = {
    errorCode,
    sanitizedMessage: sanitizeProcessOutput(errorMessage(error)),
    redactedContext: {},
  };
  if (error && error.cleanupFailed === true) {
    fields.cleanupFailed = true;
    fields.cleanupErrorKind = sanitizeProcessOutput(error.cleanupErrorKind ?? 'server_stop_failed');
    fields.cleanupMessage = sanitizeProcessOutput(error.cleanupMessage ?? '');
  }
  return fields;
}

function failedScenarioReport({
  fixtureKind,
  fixtureDensity,
  scenarioId,
  mode,
  runKind,
  allocatorProfile,
  error,
  errorCode,
  includeLegacyErrorFields = false,
}) {
  const scenarioError = scenarioErrorFields(error, errorCode);
  return {
    fixtureKind,
    fixtureDensity,
    scenarioId,
    mode,
    runKind,
    allocatorProfile: allocatorProfileReport(allocatorProfile),
    status: 'failed',
    ...(includeLegacyErrorFields ? errorReportFields(error) : {}),
    scenarioError,
  };
}

function buildReportSummary(reports, options = {}) {
  const hasFailed = reports.some((report) => report.status === 'failed');
  const hasPartial = reports.some((report) => report.status === 'partial');
  const hasTimelineReport = reports.some((report) => report.timeline);
  const comparisons = buildAllocatorComparisons(reports, options);
  const scenarioComparisons = buildScenarioComparisons(reports);
  const hasIncompleteRequiredAllocatorComparison = comparisons.some((comparison) => (
    isRequiredAllocatorComparison(comparison)
    && comparison.comparisonStatus !== 'ok'
  ));
  const missingRequiredAllocatorComparison = hasTimelineReport
    && !hasRequiredAllocatorComparisons(comparisons);
  const hasIncompleteScenarioComparison = scenarioComparisons.some((comparison) => (
    comparison.comparisonStatus !== 'ok'
  ));
  const acceptanceReasons = [];
  if (!hasTimelineReport && !hasFailed && !hasPartial) {
    acceptanceReasons.push('timeline_not_requested');
  }
  if (hasPartial) {
    acceptanceReasons.push('report_partial');
    for (const report of reports) {
      if (report.status !== 'partial') {
        continue;
      }
      if (typeof report.decisionExcludedReason === 'string') {
        acceptanceReasons.push(report.decisionExcludedReason);
      }
      for (const reason of report.partialMeasurementReasons ?? []) {
        acceptanceReasons.push(reason);
      }
    }
  }
  if (hasIncompleteRequiredAllocatorComparison) {
    acceptanceReasons.push('required_allocator_comparison_not_ok');
  }
  if (missingRequiredAllocatorComparison) {
    acceptanceReasons.push('missing_required_allocator_comparison');
  }
  if (hasIncompleteScenarioComparison) {
    acceptanceReasons.push('scenario_comparison_not_ok');
  }
  const acceptanceStatus = hasFailed
    ? 'failed'
    : hasPartial || hasIncompleteRequiredAllocatorComparison || missingRequiredAllocatorComparison || hasIncompleteScenarioComparison
      ? 'partial'
      : !hasTimelineReport
      ? 'not_applicable'
      : 'full';
  const summary = {
    comparisons,
    scenarioComparisons,
    acceptanceStatus,
    fullAcceptanceMet: acceptanceStatus === 'full',
  };
  if (acceptanceReasons.length > 0) {
    summary.acceptanceReasons = acceptanceReasons;
  }
  if (acceptanceStatus === 'not_applicable') {
    summary.decisionExcludedReason = 'timeline_not_requested';
  }
  return summary;
}

function isRequiredAllocatorComparison(comparison) {
  return comparison.baseProfile === 'default'
    && comparison.compareProfile === 'arena1'
    && comparison.mode === 'release'
    && comparison.runKind === 'cold'
    && comparison.fixtureKind === 'prefix'
    && comparison.fixtureScale === 'full'
    && comparison.fixtureDensity === 'dense'
    && comparison.scenarioId === 'prefix-full-dense'
    && typeof comparison.settledSnapshotName === 'string'
    && comparison.settledSnapshotName.startsWith('settled_');
}

function hasRequiredAllocatorComparisons(comparisons) {
  return ['settled_1s', 'settled_5s'].every((settledSnapshotName) => (
    comparisons.some((comparison) => (
      isRequiredAllocatorComparison(comparison)
      && comparison.settledSnapshotName === settledSnapshotName
      && comparison.comparisonStatus === 'ok'
    ))
  ));
}

function buildAllocatorComparisons(reports, options = {}) {
  const grouped = new Map();
  for (const report of reports) {
    const profile = report.allocatorProfile?.name;
    if (!report.timeline || !profile) {
      continue;
    }
    const key = [
      report.mode,
      report.runKind,
      report.fixtureKind,
      report.fixtureDensity,
      report.scenarioId,
    ].join('|');
    const group = grouped.get(key) ?? [];
    group.push(report);
    grouped.set(key, group);
  }

  const comparisons = [];
  for (const group of grouped.values()) {
    const base = group.find((report) => report.allocatorProfile.name === 'default') ?? null;
    const compare = group.find((report) => report.allocatorProfile.name === 'arena1') ?? null;
    const representative = base ?? compare;
    const delayEntries = allocatorComparisonDelayEntries(base, compare);
    for (const { settledDelayMs, settledSnapshotName } of delayEntries) {
      const entry = {
        mode: representative.mode,
        runKind: representative.runKind,
        fixtureKind: representative.fixtureKind,
        fixtureScale: options.fixtureScale ?? null,
        fixtureDensity: representative.fixtureDensity,
        scenarioId: representative.scenarioId,
        settledDelayMs,
        settledSnapshotName,
        baseProfile: 'default',
        compareProfile: 'arena1',
        missingProfiles: [],
      };
      if (!base) {
        entry.comparisonStatus = 'skipped';
        entry.missingProfiles.push('default');
        entry.excludedReason = 'missing_default_profile';
      } else if (!compare) {
        entry.comparisonStatus = 'skipped';
        entry.missingProfiles.push('arena1');
        entry.excludedReason = 'missing_arena1_profile';
      } else if (base.status === 'failed' || compare.status === 'failed') {
        entry.comparisonStatus = 'skipped';
        entry.excludedReason = 'profile_report_failed';
      } else if (base.status !== 'ok' || compare.status !== 'ok') {
        entry.comparisonStatus = 'partial';
        entry.excludedReason = 'profile_report_not_ok';
      } else {
        const baseSnapshot = snapshotByName(base.timeline.snapshots ?? [], settledSnapshotName);
        const compareSnapshot = snapshotByName(compare.timeline.snapshots ?? [], settledSnapshotName);
        const delta = deltaKb(baseSnapshot?.status?.RssAnon, compareSnapshot?.status?.RssAnon);
        if (delta === null) {
          entry.comparisonStatus = 'partial';
          entry.excludedReason = 'missing_settled_anon';
        } else {
          entry.comparisonStatus = 'ok';
          entry.default_vs_arena1_settled_delta_kb = delta;
        }
      }
      comparisons.push(entry);
    }
  }
  return comparisons;
}

function allocatorComparisonDelayEntries(base, compare) {
  const entries = new Map();
  for (const report of [base, compare]) {
    for (const settled of report?.timeline?.derived?.settledComparisons ?? []) {
      entries.set(`${settled.settledDelayMs}|${settled.settledSnapshotName}`, {
        settledDelayMs: settled.settledDelayMs,
        settledSnapshotName: settled.settledSnapshotName,
      });
    }
    for (const snapshot of report?.timeline?.snapshots ?? []) {
      const match = /^settled_(\d+)s$/.exec(snapshot.name);
      if (match) {
        const settledDelayMs = Number(match[1]) * 1000;
        entries.set(`${settledDelayMs}|${snapshot.name}`, { settledDelayMs, settledSnapshotName: snapshot.name });
      }
    }
  }
  return [...entries.values()].sort((left, right) => left.settledDelayMs - right.settledDelayMs);
}

function buildScenarioComparisons(reports) {
  const pairs = [
    ['prefix-full-dense', 'prefix-full-sparse'],
    ['prefix-full-dense', 'multifile-full-dense'],
  ];
  const comparisons = [];
  for (const [baseScenarioId, compareScenarioId] of pairs) {
    for (const base of reports.filter((report) => report.scenarioId === baseScenarioId && report.timeline)) {
      const compare = reports.find((report) => (
        report.scenarioId === compareScenarioId
        && report.mode === base.mode
        && report.runKind === base.runKind
        && report.allocatorProfile?.name === base.allocatorProfile?.name
        && report.timeline
      ));
      if (!compare) {
        continue;
      }
      for (const settled of base.timeline.derived?.settledComparisons ?? []) {
        const compareSettled = compare.timeline.derived?.settledComparisons?.find((entry) => (
          entry.settledDelayMs === settled.settledDelayMs
          && entry.settledSnapshotName === settled.settledSnapshotName
        ));
        comparisons.push(buildScenarioComparisonEntry({
          base,
          compare,
          baseScenarioId,
          compareScenarioId,
          metric: 'peak_to_settled_delta_kb',
          settledDelayMs: settled.settledDelayMs,
          settledSnapshotName: settled.settledSnapshotName,
          baseMetric: settled.peak_to_settled_delta_kb,
          compareMetric: compareSettled?.peak_to_settled_delta_kb,
          baseExcludedReason: settled.excludedReason,
          compareExcludedReason: compareSettled?.excludedReason,
        }));
      }
      comparisons.push(buildScenarioComparisonEntry({
        base,
        compare,
        baseScenarioId,
        compareScenarioId,
        metric: 'headers_to_body_peak_delta_kb',
        settledDelayMs: null,
        settledSnapshotName: null,
        baseMetric: base.timeline.derived?.headers_to_body_peak_delta_kb,
        compareMetric: compare.timeline.derived?.headers_to_body_peak_delta_kb,
      }));
    }
  }
  return comparisons;
}

function buildScenarioComparisonEntry({
  base,
  compare,
  baseScenarioId,
  compareScenarioId,
  metric,
  settledDelayMs,
  settledSnapshotName,
  baseMetric,
  compareMetric,
  baseExcludedReason = null,
  compareExcludedReason = null,
}) {
  const entry = {
    comparisonType: 'scenario',
    mode: base.mode,
    runKind: base.runKind,
    allocatorProfile: base.allocatorProfile.name,
    baseScenarioId,
    compareScenarioId,
    metric,
    settledDelayMs,
    settledSnapshotName,
  };
  if (base.status === 'failed' || compare.status === 'failed') {
    return {
      ...entry,
      deltaKb: null,
      ratio: null,
      comparisonStatus: 'skipped',
      excludedReason: 'scenario_report_failed',
    };
  }
  if (base.status !== 'ok' || compare.status !== 'ok') {
    return {
      ...entry,
      deltaKb: null,
      ratio: null,
      comparisonStatus: 'partial',
      excludedReason: 'scenario_report_not_ok',
    };
  }
  const metricExcludedReason = scenarioMetricExcludedReason(baseExcludedReason, compareExcludedReason);
  if (metricExcludedReason) {
    return {
      ...entry,
      deltaKb: null,
      ratio: null,
      comparisonStatus: 'skipped',
      excludedReason: metricExcludedReason,
    };
  }
  const delta = deltaKb(compareMetric, baseMetric);
  const ratio = ratioOrNull(compareMetric, baseMetric);
  if (delta === null || ratio === null) {
    const excludedReason = !Number.isFinite(baseMetric)
      ? 'missing_or_invalid_base_metric'
      : 'missing_or_invalid_compare_metric';
    return {
      ...entry,
      deltaKb: null,
      ratio: null,
      comparisonStatus: 'partial',
      excludedReason,
    };
  }
  return {
    ...entry,
    deltaKb: delta,
    ratio,
    comparisonStatus: 'ok',
  };
}

function scenarioMetricExcludedReason(baseExcludedReason, compareExcludedReason) {
  const baseReason = normalizeExcludedReason(baseExcludedReason);
  const compareReason = normalizeExcludedReason(compareExcludedReason);
  if (baseReason && compareReason) {
    return baseReason === compareReason
      ? `base_and_compare_${baseReason}`
      : `base_${baseReason}_compare_${compareReason}`;
  }
  if (baseReason) {
    return `base_${baseReason}`;
  }
  if (compareReason) {
    return `compare_${compareReason}`;
  }
  return null;
}

function normalizeExcludedReason(reason) {
  return typeof reason === 'string' && reason.length > 0 ? reason : null;
}

async function runMeasurement(options) {
  const root = createFixtureRoot();
  try {
    const reports = [];
    let failed = false;
    for (const fixtureKind of options.fixtures) {
      for (const fixtureDensity of options.fixtureDensities) {
        let fixture;
        try {
          fixture = createFixture(root, fixtureKind, options.fixtureScale, fixtureDensity);
        } catch (error) {
          failed = true;
          for (const mode of options.modes) {
            for (const allocatorProfile of options.allocatorProfiles) {
              for (const runKind of options.runs) {
                reports.push(failedScenarioReport({
                  fixtureKind,
                  fixtureDensity,
                  scenarioId: `${fixtureKind}-${options.fixtureScale}-${fixtureDensity}`,
                  mode,
                  runKind,
                  allocatorProfile,
                  error,
                  errorCode: 'fixture_failed',
                  includeLegacyErrorFields: !options.timeline,
                }));
              }
            }
          }
          continue;
        }
        for (const mode of options.modes) {
          for (const allocatorProfile of options.allocatorProfiles) {
            for (const runKind of options.runs) {
              const baseReport = {
                fixtureKind,
                fixtureDensity,
                scenarioId: fixture.scenarioId,
                fixture: {
                  fileCount: fixture.fileCount,
                  bytes: fixture.bytes,
                  workspace: fixture.maskedWorkspace,
                },
                mode,
                runKind,
                allocatorProfile: allocatorProfileReport(allocatorProfile),
              };
              try {
                const scenario = await runMeasuredScenario(options, fixture, mode, runKind, allocatorProfile);
                reports.push({
                  ...baseReport,
                  status: scenario.status ?? 'ok',
                  ...scenario,
                });
              } catch (error) {
                failed = true;
                reports.push({
                  ...baseReport,
                  status: 'failed',
                  ...(!options.timeline ? errorReportFields(error) : {}),
                  scenarioError: scenarioErrorFields(error),
                });
              }
            }
          }
        }
      }
    }
    const reportSummary = buildReportSummary(reports, options);
    console.log(JSON.stringify(sanitizeReport({
      measurementContext: buildMeasurementContext(options),
      tempRoot: root,
      comparisons: reportSummary.comparisons,
      scenarioComparisons: reportSummary.scenarioComparisons,
      acceptanceStatus: reportSummary.acceptanceStatus,
      fullAcceptanceMet: reportSummary.fullAcceptanceMet,
      ...(reportSummary.acceptanceReasons ? { acceptanceReasons: reportSummary.acceptanceReasons } : {}),
      ...(reportSummary.decisionExcludedReason ? { decisionExcludedReason: reportSummary.decisionExcludedReason } : {}),
      reports,
    }), null, 2));
    return measurementExitCode({ failed, strict: options.strict, reportSummary });
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

function procCompletenessForSnapshots(snapshots) {
  return summarizeProcCompleteness(snapshots.map((snapshot) => ({
    status: snapshot.status,
    smapsRollup: snapshot.smapsRollup,
    maps: snapshot.maps,
  })));
}

function measurementExitCode({ failed, strict, reports, reportSummary }) {
  const summary = reportSummary ?? buildReportSummary(reports ?? []);
  if (failed || summary.acceptanceStatus === 'failed') {
    return 1;
  }
  if (strict && summary.acceptanceStatus !== 'full') {
    return 1;
  }
  return 0;
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
      timeline: options.timeline,
      strict: options.strict,
      fixtureScale: options.fixtureScale,
      fixtureDensities: options.fixtureDensities,
      settledDelaysMs: options.settledDelaysMs,
      modes: options.modes,
      fixtures: options.fixtures,
      runs: options.runs,
      allocatorProfiles: options.allocatorProfiles,
      port: options.port,
    },
    measuredServerEnvPolicy: {
      inheritedEnvKeys: [...MEASURED_SERVER_ENV_ALLOWLIST],
      allocatorEnvScrubTargetKeys: [...ALLOCATOR_ENV_KEYS],
      reportPolicy: 'reports allocator env key names needed for interpretation, never parent env values',
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
