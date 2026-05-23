import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync } from 'node:fs';
import path from 'node:path';

const outDir = process.env.MV_INLINE_JS_OUT_DIR;
const tscBin = path.join(process.cwd(), 'node_modules', 'typescript', 'bin', 'tsc');

if (!outDir) {
  console.error('MV_INLINE_JS_OUT_DIR is required.');
  process.exit(1);
}

if (!existsSync(tscBin)) {
  console.error('TypeScript compiler was not found. Run `npm ci` before building inline JS.');
  process.exit(1);
}

mkdirSync(outDir, { recursive: true });

const result = spawnSync(
  process.execPath,
  [tscBin, '-p', 'tsconfig.inline-js.json', '--outDir', outDir],
  { stdio: 'inherit' }
);

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
