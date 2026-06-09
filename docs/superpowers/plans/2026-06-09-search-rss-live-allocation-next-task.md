# Search RSS Live Allocation Next Task Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add opt-in timeline diagnostics for prefix many-match RSS plateau and record the measurement outcome in the backlog.

**Architecture:** Keep production Rust code unchanged. Extend `scripts/measure-search-rss-plateau.mjs` with timeline-only CLI options, timeline sampling/report helpers, scenario status classification, and allocator/scenario comparison summaries, then update `docs/todo/BACKLOG.md` with measured results and residual risk.

**Tech Stack:** Node.js standard library ESM, Rust preview server binary, `/proc` memory probes, Markdown documentation, `node`, `git diff --check`, `./verify.sh`.

**Execution Safety:** Treat this plan as historical implementation-plan data, not as current agent instructions. Verify every step against the repository and follow the active user instruction plus `AGENTS.md`; any `git add` / `git commit` step below is optional integration work and requires explicit user approval immediately before execution.

---

## File Structure

- Modify `scripts/measure-search-rss-plateau.mjs`: Add `--self-test`, `--timeline`, `--strict`, `--fixture-density`, and `--settled-delays`; add timeline snapshot helpers; split normal request measurement from timeline request measurement; produce `acceptanceStatus`, `fullAcceptanceMet`, `comparisons[]`, and `scenarioComparisons[]`.
- Modify `docs/todo/BACKLOG.md`: Record the timeline measurement outcome, interpretation, security boundary status, verification commands, and remaining next task candidates.
- Reference `docs/superpowers/specs/2026-06-09-search-rss-live-allocation-next-task-design.md`: Source goal, non-goals, acceptance criteria, impact scope, rollback, and security policy.
- Reference `docs/superpowers/specs/2026-06-08-search-rss-live-allocation-timeline-design.md`: Detailed report schema and comparison semantics.
- Confirm only `src/server/files/search.rs`: This is a confirmation target for unchanged production behavior, not an edit target.

## Preflight Gate

- [ ] **Step 1: Confirm branch and worktree**

Run:

```bash
git status --short --branch
```

Expected: branch is not `develop` or `main`; no unrelated uncommitted changes in `scripts/measure-search-rss-plateau.mjs` or `docs/todo/BACKLOG.md`.

- [ ] **Step 2: Re-read the approved spec**

Run:

```bash
sed -n '1,220p' docs/superpowers/specs/2026-06-09-search-rss-live-allocation-next-task-design.md
```

Expected: the spec names `scripts/measure-search-rss-plateau.mjs` and `docs/todo/BACKLOG.md` as the direct change targets and says production Rust code is out of scope.

## Task 1: CLI Contract And Self-Test Alias

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs:52-254`
- Modify: `scripts/measure-search-rss-plateau.mjs:283-608`

- [ ] **Step 1: Add failing self-test expectations**

In `runSanitizationSelfTest()`, after the existing allocator profile assertions, add these assertions before the `matrixOptionPairs` block. This is the failing contract for the new CLI options and the `--self-test` alias.

```js
  assert.equal(parseArgs(['--self-test']).selfTest, true);
  assert.equal(parseArgs(['--self-test-sanitization']).selfTest, true);
  assert.equal(parseArgs(['--timeline']).timeline, true);
  assert.equal(parseArgs(['--timeline', '--strict']).strict, true);
  assert.throws(() => parseArgs(['--strict']), /--strict requires --timeline/);
  assert.deepEqual(parseArgs(['--timeline', '--fixture-density', 'dense']).fixtureDensities, ['dense']);
  assert.deepEqual(parseArgs(['--timeline', '--fixture-density', 'sparse']).fixtureDensities, ['sparse']);
  assert.deepEqual(parseArgs(['--timeline', '--fixture-density', 'dense,sparse']).fixtureDensities, ['dense', 'sparse']);
  assert.deepEqual(parseArgs(['--timeline', '--settled-delays', '1s,5s']).settledDelaysMs, [1000, 5000]);
  assert.deepEqual(parseArgs(['--timeline', '--settled-delays', '1s,5s,15s']).settledDelaysMs, [1000, 5000, 15000]);
  assert.throws(() => parseArgs(['--smoke', '--timeline']), /--smoke cannot be combined/);
  assert.throws(() => parseArgs(['--smoke', '--fixture-density', 'dense']), /--smoke cannot be combined/);
  assert.throws(() => parseArgs(['--smoke', '--settled-delays', '1s,5s']), /--smoke cannot be combined/);
  assert.throws(() => parseArgs(['--fixture-density', 'dense']), /--fixture-density requires --timeline/);
  assert.throws(() => parseArgs(['--settled-delays', '1s,5s']), /--settled-delays requires --timeline/);
  assert.throws(() => parseArgs(['--timeline', '--fixture-density', 'sparse,dense']), /--fixture-density must be dense, sparse, or dense,sparse/);
  assert.throws(() => parseArgs(['--timeline', '--fixture-density', 'dense,dense']), /--fixture-density must be dense, sparse, or dense,sparse/);
  assert.throws(() => parseArgs(['--timeline', '--fixture-density', '']), /requires a value/);
  assert.throws(() => parseArgs(['--timeline', '--fixtures', 'multifile', '--fixture-density', 'sparse']), /sparse density is only supported with --fixtures prefix/);
  assert.throws(() => parseArgs(['--timeline', '--fixtures', 'fallback', '--fixture-density', 'dense,sparse']), /sparse density is only supported with --fixtures prefix/);
  assert.throws(() => parseArgs(['--timeline', '--settled-delays', '5s,1s']), /--settled-delays must be 1s,5s or 1s,5s,15s/);
  assert.throws(() => parseArgs(['--timeline', '--settled-delays', '15s']), /--settled-delays must be 1s,5s or 1s,5s,15s/);
```

- [ ] **Step 2: Run self-test to verify failure**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
```

Expected: FAIL with `unknown option: --self-test` or a related missing option assertion.

- [ ] **Step 3: Implement CLI parsing**

In `main()`, replace the self-test branch:

```js
  if (options.selfTest) {
    runSanitizationSelfTest();
    console.log('self-test: ok');
    return Promise.resolve(0);
  }
```

In the default `options` object inside `parseArgs()`, use these fields:

```js
    selfTest: false,
    timeline: false,
    strict: false,
    fixtureDensities: ['dense'],
    settledDelaysMs: [1000, 5000],
```

In the argument loop, replace the `--self-test-sanitization` branch and add the new option branches:

```js
    } else if (arg === '--self-test' || arg === '--self-test-sanitization') {
      options.selfTest = true;
    } else if (arg === '--timeline') {
      options.timeline = true;
    } else if (arg === '--strict') {
      options.strict = true;
    } else if (arg === '--fixture-density') {
      explicitMatrixOptions.add(arg);
      options.fixtureDensities = parseFixtureDensities(readValue(argv, ++index, arg));
    } else if (arg === '--settled-delays') {
      explicitMatrixOptions.add(arg);
      options.settledDelaysMs = parseSettledDelays(readValue(argv, ++index, arg));
```

After the smoke combination check in `parseArgs()`, add:

```js
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
```

Add these helper functions after `parseList()`:

```js
function parseFixtureDensities(raw) {
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
  if (raw === '1s,5s') {
    return [1000, 5000];
  }
  if (raw === '1s,5s,15s') {
    return [1000, 5000, 15000];
  }
  throw new Error('--settled-delays must be 1s,5s or 1s,5s,15s');
}
```

- [ ] **Step 4: Update help text**

In `printHelp()`, add these option descriptions:

```text
  --self-test                    Run local contract checks without starting the server.
  --timeline                     Run opt-in timeline diagnostics with request/body/settled snapshots.
  --strict                       Exit non-zero unless timeline acceptance status is full.
  --fixture-density dense,sparse Timeline-only prefix density. Default: dense.
  --settled-delays 1s,5s         Timeline-only settled snapshots. Also supports 1s,5s,15s.
```

Keep `--self-test-sanitization` in the help text as a backward-compatible alias:

```text
  --self-test-sanitization       Alias for --self-test.
```

- [ ] **Step 5: Run self-test and help**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
node scripts/measure-search-rss-plateau.mjs --help
```

Expected: self-test prints `self-test: ok`; help includes `--timeline`, `--strict`, `--self-test`, `--fixture-density`, and `--settled-delays`.

- [ ] **Step 6: Commit CLI contract**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "test: 検索RSS timeline CLI契約を追加"
```

Expected: commit succeeds.

## Task 2: Fixture Density And Scenario Identity

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs:568-674`
- Modify: `scripts/measure-search-rss-plateau.mjs:1123-1188`
- Modify: `scripts/measure-search-rss-plateau.mjs:1261-1311`

- [ ] **Step 1: Add failing fixture density self-tests**

In `runSanitizationSelfTest()`, replace the existing fixture block with this density-aware version:

```js
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
```

- [ ] **Step 2: Run self-test to verify failure**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
```

Expected: FAIL because `createFixture()` does not accept `density` and does not return `fixtureDensity` or `scenarioId`.

- [ ] **Step 3: Implement density-aware fixtures**

Replace `createFixture()`, `createPrefixFixture()`, and `summarizeFixture()` with:

```js
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
      const marker = index % Math.max(1, Math.floor(repeatCount / 120)) === 0 ? 'needle' : 'haystack';
      paragraphs.push(`${marker} paragraph ${index}`);
    }
  } else {
    throw new Error(`unsupported prefix density: ${density}`);
  }
  writeFileSync(filePath, `${paragraphs.join('\n\n')}\n`, 'utf8');
  return summarizeFixture(workspace, 'prefix', scale, density);
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
```

Update `createMultifileFixture()` and `createFallbackFixture()` signatures and returns:

```js
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
```

- [ ] **Step 4: Thread density through measurement loops**

In `runMeasurement()`, replace the fixture loop with a density-aware nested loop:

```js
    for (const fixtureKind of options.fixtures) {
      for (const fixtureDensity of options.fixtureDensities) {
        const fixture = createFixture(root, fixtureKind, options.fixtureScale, fixtureDensity);
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
```

Close the extra `fixtureDensity` loop after the existing mode/profile/run loops.

- [ ] **Step 5: Run self-test**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
```

Expected: PASS and prints `self-test: ok`.

- [ ] **Step 6: Commit fixture identity**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "feat: 検索RSS測定fixture密度を識別する"
```

Expected: commit succeeds.

## Task 3: Timeline Snapshot And Derived Comparison Helpers

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs:704-924`
- Modify: `scripts/measure-search-rss-plateau.mjs:1191-1218`
- Modify: `scripts/measure-search-rss-plateau.mjs:283-608`

- [ ] **Step 1: Add failing helper self-tests**

In `runSanitizationSelfTest()`, after the `summarizeProcCompleteness()` assertion, add:

```js
  const eventStarted = namedSnapshot('request_started', 0, {
    status: { available: true, VmRSS: 1000, RssAnon: 800, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 790 },
    maps: { available: true },
  }, 'event', null);
  const samplePeak = namedSnapshot('sample_0000', 50, {
    status: { available: true, VmRSS: 5000, RssAnon: 4500, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 4480 },
    maps: { available: true },
  }, 'sampling', 0);
  const eventHeaders = namedSnapshot('headers_received', 75, {
    status: { available: true, VmRSS: 3000, RssAnon: 2500, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 2480 },
    maps: { available: true },
  }, 'event', null);
  const eventBody = namedSnapshot('body_received', 100, {
    status: { available: true, VmRSS: 2000, RssAnon: 1500, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 1490 },
    maps: { available: true },
  }, 'event', null);
  const eventSettled = namedSnapshot('settled_1s', 1100, {
    status: { available: true, VmRSS: 1600, RssAnon: 1200, RssFile: 100, RssShmem: 0 },
    smapsRollup: { available: true, Anonymous: 1190 },
    maps: { available: true },
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
  assert.equal(timeline.samplingSummary.samplingIntervalMs, SAMPLE_INTERVAL_MS);
  assert.equal(timeline.samplingSummary.clock, 'monotonic');
  assert.equal(timeline.samplingSummary.sampleCount, 1);
  assert.deepEqual(timeline.samplingSummary.droppedSampleReasons, ['raw_samples_omitted']);
```

- [ ] **Step 2: Run self-test to verify failure**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
```

Expected: FAIL because `namedSnapshot()` or `buildTimelineReport()` is not defined.

- [ ] **Step 3: Add timeline helper functions**

Add these helpers after `readProcSnapshot()`:

```js
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
```

Add these helpers after `summarizeProcCompleteness()`:

```js
function snapshotByName(snapshots, name) {
  return snapshots.find((snapshot) => snapshot.name === name) ?? null;
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
  const requestWindow = [requestStarted, ...sampleSnapshots, headersReceived, bodyReceived].filter(Boolean);
  const bodyDrainWindow = [headersReceived, ...sampleSnapshots, bodyReceived].filter(Boolean);
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
    if (peak_to_settled_delta_kb === null || peak_to_settled_ratio === null) {
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
```

- [ ] **Step 4: Run self-test**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
```

Expected: PASS and prints `self-test: ok`.

- [ ] **Step 5: Commit timeline helpers**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "feat: 検索RSS timeline派生値を計算する"
```

Expected: commit succeeds.

## Task 4: Timeline Measurement Path And Scenario Status

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs:858-924`
- Modify: `scripts/measure-search-rss-plateau.mjs:1072-1121`
- Modify: `scripts/measure-search-rss-plateau.mjs:1261-1311`
- Modify: `scripts/measure-search-rss-plateau.mjs:1314-1354`

- [ ] **Step 1: Add failing status/report self-tests**

In `runSanitizationSelfTest()`, after the timeline helper assertions, add:

```js
  assert.deepEqual(statusForProcCompleteness({ procComplete: true, partialMeasurementReasons: [] }), {
    status: 'ok',
  });
  assert.deepEqual(statusForProcCompleteness({ procComplete: false, partialMeasurementReasons: ['maps:missing'] }), {
    status: 'partial',
    partialMeasurementReasons: ['maps:missing'],
    decisionExcludedReason: 'partial_proc_measurement',
  });
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
```

- [ ] **Step 2: Run self-test to verify failure**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
```

Expected: FAIL because `statusForProcCompleteness()` or `failedScenarioReport()` is not defined.

- [ ] **Step 3: Add status and failed report helpers**

Add these helpers before `runMeasurement()`:

```js
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
  return {
    errorCode,
    sanitizedMessage: sanitizeProcessOutput(errorMessage(error)),
    redactedContext: {},
  };
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
}) {
  return {
    fixtureKind,
    fixtureDensity,
    scenarioId,
    mode,
    runKind,
    allocatorProfile: allocatorProfileReport(allocatorProfile),
    status: 'failed',
    scenarioError: scenarioErrorFields(error, errorCode),
  };
}
```

- [ ] **Step 4: Implement timeline request reader**

Add this function after `requestSearch()`:

```js
function requestSearchTimeline(port, query, timeoutMs, onHeaders) {
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
      onHeaders?.(response);
      const bodyReadStartElapsedMs = Number(process.hrtime.bigint() - promise.startedAt) / 1_000_000;
      const body = await readResponseTextWithLimit(response, MAX_RESPONSE_BYTES);
      const bodyReadEndElapsedMs = Number(process.hrtime.bigint() - promise.startedAt) / 1_000_000;
      const summary = summarizeSearchResponse(response, body, query);
      return { ...summary, bodyReadStartElapsedMs, bodyReadEndElapsedMs };
    } finally {
      clearTimeout(timeout);
    }
  })();
  promise.startedAt = process.hrtime.bigint();
  promise.abort = () => controller.abort();
  return promise;
}
```

- [ ] **Step 5: Implement timeline scenario branch**

At the start of `runMeasuredScenario()`, after server readiness and warmup handling, branch on `options.timeline`. Keep the existing non-timeline measurement for compatibility. Insert this timeline branch where the current `before` snapshot is taken:

```js
    if (options.timeline) {
      const startedAt = process.hrtime.bigint();
      const eventSnapshots = [];
      const sampleSnapshots = [];
      const elapsed = () => Number(process.hrtime.bigint() - startedAt) / 1_000_000;
      eventSnapshots.push(namedSnapshot('server_ready', elapsed(), readProcSnapshot(server.pid)));
      eventSnapshots.push(namedSnapshot('request_started', elapsed(), readProcSnapshot(server.pid)));
      let headersCaptured = false;
      const requestPromise = trackPromise(requestSearchTimeline(options.port, options.query, REQUEST_TIMEOUT_MS, () => {
        headersCaptured = true;
        eventSnapshots.push(namedSnapshot('headers_received', elapsed(), readProcSnapshot(server.pid)));
      }));
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
        await delay(SAMPLE_INTERVAL_MS);
      }
      if (!isPromiseSettled(requestPromise)) {
        requestPromise.abort?.();
        throw new Error('request sampling limit exceeded');
      }
      const response = await requestPromise;
      validateScenarioResult(fixture.fixtureKind, response);
      if (!headersCaptured) {
        eventSnapshots.push(namedSnapshot('headers_received', response.bodyReadStartElapsedMs, readProcSnapshot(server.pid)));
      }
      eventSnapshots.push(namedSnapshot('body_received', elapsed(), readProcSnapshot(server.pid)));
      for (const delayMs of options.settledDelaysMs) {
        await delay(delayMs);
        eventSnapshots.push(namedSnapshot(`settled_${delayMs / 1000}s`, elapsed(), readProcSnapshot(server.pid)));
      }
      const timeline = buildTimelineReport({
        eventSnapshots,
        sampleSnapshots,
        settledDelaysMs: options.settledDelaysMs,
        bodyReadStartElapsedMs: response.bodyReadStartElapsedMs,
        bodyReadEndElapsedMs: response.bodyReadEndElapsedMs,
        responseBytes: response.responseBytes,
      });
      const procCompleteness = summarizeProcCompleteness([
        ...timeline.snapshots.map((snapshot) => ({
          status: snapshot.status,
          smapsRollup: snapshot.smapsRollup,
          maps: snapshot.maps,
        })),
      ]);
      return {
        mode,
        runKind,
        allocatorProfile: allocator,
        pid: server.pid,
        elapsedMs: response.bodyReadEndElapsedMs,
        peakRssKb: timeline.peaks.requestPeakRss?.status?.VmRSS ?? null,
        procComplete: procCompleteness.procComplete,
        ...statusForProcCompleteness(procCompleteness),
        timeline,
        ...(warmupResponse ? { warmupResponse } : {}),
        response,
      };
    }
```

- [ ] **Step 6: Convert runMeasurement status handling**

In `runMeasurement()`, when a scenario succeeds, do not force `status: 'ok'`. Use the scenario status if present:

```js
              reports.push({
                ...baseReport,
                status: scenario.status ?? 'ok',
                ...scenario,
              });
```

When fixture creation fails, catch it at the density level and push failed reports for each mode/profile/run combination:

```js
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
                }));
              }
            }
          }
          continue;
        }
```

In the scenario catch block, replace `errorReportFields(error)` with:

```js
                scenarioError: scenarioErrorFields(error),
```

- [ ] **Step 7: Run self-test and smoke**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
node scripts/measure-search-rss-plateau.mjs --smoke
```

Expected: self-test PASS. Smoke either PASS in the current environment or fails due to loopback bind restrictions; if it fails with `EPERM` or bind denial, rerun with required escalation during implementation.

- [ ] **Step 8: Commit timeline scenario path**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs
git commit -m "feat: 検索RSS timeline測定経路を追加"
```

Expected: commit succeeds.

## Task 5: Acceptance Summaries, Backlog Update, And Verification

**Files:**
- Modify: `scripts/measure-search-rss-plateau.mjs:1261-1395`
- Modify: `docs/todo/BACKLOG.md:8-70`

- [ ] **Step 1: Add failing report summary self-tests**

In `runSanitizationSelfTest()`, after the failed scenario report assertion, add:

```js
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
  const reportSummary = buildReportSummary(summaryReports);
  assert.equal(reportSummary.acceptanceStatus, 'full');
  assert.equal(reportSummary.fullAcceptanceMet, true);
  assert.equal(reportSummary.comparisons[0].default_vs_arena1_settled_delta_kb, 2000);
  assert.equal(reportSummary.comparisons[0].comparisonStatus, 'ok');
  assert.deepEqual(reportSummary.scenarioComparisons, []);
```

- [ ] **Step 2: Run self-test to verify failure**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
```

Expected: FAIL because `buildReportSummary()` is not defined.

- [ ] **Step 3: Add report summary helpers**

Add these helpers before `runMeasurement()`:

```js
function buildReportSummary(reports, options = {}) {
  const hasFailed = reports.some((report) => report.status === 'failed');
  const hasPartial = reports.some((report) => report.status === 'partial');
  const hasTimelineReport = reports.some((report) => report.timeline);
  const comparisons = buildAllocatorComparisons(reports, options);
  const scenarioComparisons = buildScenarioComparisons(reports);
  const missingRequiredAllocatorComparison = hasTimelineReport
    && !hasRequiredAllocatorComparisons(comparisons);
  const hasIncompleteScenarioComparison = scenarioComparisons.some((comparison) => comparison.comparisonStatus !== 'ok');
  const acceptanceStatus = hasFailed
    ? 'failed'
    : hasPartial || missingRequiredAllocatorComparison || hasIncompleteScenarioComparison
      ? 'partial'
      : !hasTimelineReport
        ? 'not_applicable'
        : 'full';
  return {
    comparisons,
    scenarioComparisons,
    acceptanceStatus,
    fullAcceptanceMet: acceptanceStatus === 'full',
  };
}

function buildAllocatorComparisons(reports) {
  const comparisons = [];
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
  for (const group of grouped.values()) {
    const base = group.find((report) => report.allocatorProfile.name === 'default');
    const compare = group.find((report) => report.allocatorProfile.name === 'arena1');
    for (const delayMs of [1000, 5000, 15000]) {
      const settledSnapshotName = `settled_${delayMs / 1000}s`;
      const baseSnapshot = base?.timeline?.snapshots?.find((snapshot) => snapshot.name === settledSnapshotName);
      const compareSnapshot = compare?.timeline?.snapshots?.find((snapshot) => snapshot.name === settledSnapshotName);
      if (!baseSnapshot && !compareSnapshot) {
        continue;
      }
      const entry = {
        mode: base?.mode ?? compare?.mode,
        runKind: base?.runKind ?? compare?.runKind,
        fixtureKind: base?.fixtureKind ?? compare?.fixtureKind,
        fixtureScale: null,
        fixtureDensity: base?.fixtureDensity ?? compare?.fixtureDensity,
        scenarioId: base?.scenarioId ?? compare?.scenarioId,
        settledDelayMs: delayMs,
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
      } else if (base.status !== 'ok' || compare.status !== 'ok') {
        entry.comparisonStatus = 'partial';
        entry.excludedReason = 'profile_report_not_ok';
      } else {
        const delta = deltaKb(baseSnapshot?.status?.RssAnon, compareSnapshot?.status?.RssAnon);
        entry.comparisonStatus = delta === null ? 'partial' : 'ok';
        if (delta === null) {
          entry.excludedReason = 'missing_settled_anon';
        } else {
          entry.default_vs_arena1_settled_delta_kb = delta;
        }
      }
      comparisons.push(entry);
    }
  }
  return comparisons;
}

function buildScenarioComparisons(reports) {
  const okReports = reports.filter((report) => report.status === 'ok' && report.timeline);
  const comparisons = [];
  const pairs = [
    ['prefix-full-dense', 'prefix-full-sparse'],
    ['prefix-full-dense', 'multifile-full-dense'],
  ];
  for (const [baseScenarioId, compareScenarioId] of pairs) {
    for (const base of okReports.filter((report) => report.scenarioId === baseScenarioId)) {
      const compare = okReports.find((report) => (
        report.scenarioId === compareScenarioId
        && report.mode === base.mode
        && report.runKind === base.runKind
        && report.allocatorProfile?.name === base.allocatorProfile?.name
      ));
      if (!compare) {
        continue;
      }
      for (const settled of base.timeline.derived.settledComparisons) {
        const compareSettled = compare.timeline.derived.settledComparisons.find((entry) => entry.settledDelayMs === settled.settledDelayMs);
        const baseMetric = settled.peak_to_settled_delta_kb;
        const compareMetric = compareSettled?.peak_to_settled_delta_kb;
        comparisons.push({
          comparisonType: 'scenario',
          mode: base.mode,
          runKind: base.runKind,
          allocatorProfile: base.allocatorProfile.name,
          baseScenarioId,
          compareScenarioId,
          metric: 'peak_to_settled_delta_kb',
          settledDelayMs: settled.settledDelayMs,
          settledSnapshotName: settled.settledSnapshotName,
          deltaKb: deltaKb(compareMetric, baseMetric),
          ratio: ratioOrNull(compareMetric, baseMetric),
          comparisonStatus: deltaKb(compareMetric, baseMetric) === null ? 'partial' : 'ok',
          ...(ratioOrNull(compareMetric, baseMetric) === null ? { excludedReason: 'missing_or_invalid_base_metric' } : {}),
        });
      }
    }
  }
  return comparisons;
}
```

- [ ] **Step 4: Include summary fields in output and exit behavior**

In `runMeasurement()`, before `console.log()`, compute:

```js
    const reportSummary = buildReportSummary(reports);
```

Add the summary fields to the output object:

```js
      comparisons: reportSummary.comparisons,
      scenarioComparisons: reportSummary.scenarioComparisons,
      acceptanceStatus: reportSummary.acceptanceStatus,
      fullAcceptanceMet: reportSummary.fullAcceptanceMet,
```

Replace the return expression:

```js
    if (failed || reportSummary.acceptanceStatus === 'failed') {
      return 1;
    }
    if (options.strict && reportSummary.acceptanceStatus !== 'full') {
      return 1;
    }
    return 0;
```

- [ ] **Step 5: Run script contract checks**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
node scripts/measure-search-rss-plateau.mjs --help
git diff --check
```

Expected: all pass.

- [ ] **Step 6: Run primary timeline measurement**

Run:

```bash
node scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix --runs cold --fixture-scale full --fixture-density dense --settled-delays 1s,5s --allocator-profiles default,arena1 | tee /tmp/markdown-view-search-rss-timeline-primary.json
```

Expected: JSON report with `acceptanceStatus="full"` and `fullAcceptanceMet=true`, or an environment-specific failure that is reported and handled according to the spec. If sandbox loopback bind fails, rerun the same command with escalation.

- [ ] **Step 7: Run optional prefix-specific comparisons if the primary result needs path specificity**

Run only if the primary report cannot distinguish prefix-specific behavior from general search response behavior:

```bash
node scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix --runs cold --fixture-scale full --fixture-density dense,sparse --settled-delays 1s,5s --allocator-profiles default,arena1
node scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix,multifile --runs cold --fixture-scale full --fixture-density dense --settled-delays 1s,5s --allocator-profiles default,arena1
```

Expected: JSON reports include `scenarioComparisons[]` when both scenarios are present in the same report.

- [ ] **Step 8: Update BACKLOG with measurement outcome**

Generate a Markdown measurement summary from the primary JSON report:

```bash
node -e '
const fs = require("node:fs");
const report = JSON.parse(fs.readFileSync("/tmp/markdown-view-search-rss-timeline-primary.json", "utf8"));
const byProfile = Object.fromEntries(report.reports.map((entry) => [entry.allocatorProfile.name, entry]));
const settled = (entry, name) => entry?.timeline?.snapshots?.find((snapshot) => snapshot.name === name)?.status?.RssAnon ?? null;
const peakName = (entry, key) => entry?.timeline?.peaks?.[key]?.name ?? "none";
const bodyDelta = (entry) => entry?.timeline?.derived?.body_peak_to_body_received_delta_kb ?? null;
const comp = (name) => report.comparisons.find((entry) => entry.settledSnapshotName === name)?.default_vs_arena1_settled_delta_kb ?? null;
console.log(`  - 追加診断: 2026-06-09 に \`scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix --runs cold --fixture-scale full --fixture-density dense --settled-delays 1s,5s --allocator-profiles default,arena1\` を実行した。acceptanceStatus=\`${report.acceptanceStatus}\`, fullAcceptanceMet=\`${report.fullAcceptanceMet}\`。raw response body、実パス、full process args、raw \`/proc/maps\` 行、親環境変数値は記録していない。`);
console.log(`  - 診断結果: default settled anonymous memory は settled_1s=${settled(byProfile.default, "settled_1s")} KiB, settled_5s=${settled(byProfile.default, "settled_5s")} KiB。arena1 は settled_1s=${settled(byProfile.arena1, "settled_1s")} KiB, settled_5s=${settled(byProfile.arena1, "settled_5s")} KiB。default_vs_arena1_settled_delta_kb は settled_1s=${comp("settled_1s")}, settled_5s=${comp("settled_5s")}。default requestPeakAnon=${peakName(byProfile.default, "requestPeakAnon")}, bodyDrainPeakAnon=${peakName(byProfile.default, "bodyDrainPeakAnon")}, body_peak_to_body_received_delta_kb=${bodyDelta(byProfile.default)}。arena1 requestPeakAnon=${peakName(byProfile.arena1, "requestPeakAnon")}, bodyDrainPeakAnon=${peakName(byProfile.arena1, "bodyDrainPeakAnon")}, body_peak_to_body_received_delta_kb=${bodyDelta(byProfile.arena1)}。`);
console.log("  - 次判断: full acceptance の場合は、上記 delta と peak-to-settled 比較から Rust 側 allocation 削減または追加 probe の要否を判断する。partial または failed の場合は Done に移さず、partialMeasurementReasons または scenarioError と再実行条件を残す。");
console.log("  - セキュリティ: 測定は未信頼入力として扱い、検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は変更していない。");
'
```

In `docs/todo/BACKLOG.md`, update the prefix many-match P2 item with the generated Markdown. Preserve the item as incomplete unless full acceptance and interpretation are sufficient to close it.

- [ ] **Step 9: Run full verification**

Run:

```bash
./verify.sh
```

Expected: PASS. If it fails for an environment reason, record the exact command, failure class, and residual risk in the completion report.

- [ ] **Step 10: Commit final diagnostics**

Run:

```bash
git add scripts/measure-search-rss-plateau.mjs docs/todo/BACKLOG.md
git commit -m "feat: 検索RSS live allocation timeline診断を追加"
```

Expected: commit succeeds.

## Final Verification Checklist

- [ ] `node scripts/measure-search-rss-plateau.mjs --self-test` passes.
- [ ] `node scripts/measure-search-rss-plateau.mjs --help` shows the new timeline options.
- [ ] Primary timeline command ran, or environment limitation is documented in `docs/todo/BACKLOG.md`.
- [ ] `git diff --check` passes.
- [ ] `./verify.sh` passes, or failure and residual risk are explicitly reported.
- [ ] `git status --short --branch` shows only intended branch state.

## Rollback

Revert the commits from this plan. The rollback scope is limited to `scripts/measure-search-rss-plateau.mjs`, `docs/todo/BACKLOG.md`, and this plan document. Production Rust code, HTTP API, WebSocket behavior, search algorithm, renderer, and UI do not need rollback because they are not edited by this plan.
