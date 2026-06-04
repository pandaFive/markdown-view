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
