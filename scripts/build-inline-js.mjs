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
