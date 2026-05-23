import { spawnSync } from 'node:child_process';
import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

const expectedDir = path.join(process.cwd(), 'src', 'template', 'assets', 'generated-js');
const actualDir = mkdtempSync(path.join(tmpdir(), 'markdown-view-inline-js-'));

try {
  const result = spawnSync(
    process.execPath,
    [path.join('scripts', 'build-inline-js.mjs')],
    {
      env: {
        ...process.env,
        MV_INLINE_JS_OUT_DIR: actualDir,
      },
      stdio: 'inherit',
    }
  );

  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }

  const expectedFiles = jsFileNames(expectedDir);
  const actualFiles = jsFileNames(actualDir);
  const missing = expectedFiles.filter((fileName) => !actualFiles.includes(fileName));
  const extra = actualFiles.filter((fileName) => !expectedFiles.includes(fileName));
  const changed = expectedFiles.filter((fileName) => {
    if (!actualFiles.includes(fileName)) {
      return false;
    }
    return readFileSync(path.join(expectedDir, fileName), 'utf8')
      !== readFileSync(path.join(actualDir, fileName), 'utf8');
  });

  if (missing.length > 0 || extra.length > 0 || changed.length > 0) {
    if (missing.length > 0) {
      console.error(`Missing generated inline JS fallback files: ${missing.join(', ')}`);
    }
    if (extra.length > 0) {
      console.error(`Unexpected generated inline JS fallback files: ${extra.join(', ')}`);
    }
    if (changed.length > 0) {
      console.error(`Stale generated inline JS fallback files: ${changed.join(', ')}`);
    }
    console.error('Run `MV_INLINE_JS_OUT_DIR=src/template/assets/generated-js npm run build:inline-js` and commit the generated files.');
    process.exit(1);
  }
} finally {
  rmSync(actualDir, { recursive: true, force: true });
}

function jsFileNames(directory) {
  return readdirSync(directory)
    .filter((fileName) => fileName.endsWith('.js'))
    .sort();
}
