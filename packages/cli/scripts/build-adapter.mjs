import { build } from 'esbuild';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const cliRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

await build({
  absWorkingDir: cliRoot,
  entryPoints: ['src/tsAdapter.ts'],
  outfile: 'helper/ts-adapter.js',
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node22',
  external: ['@tcc-engine/frontend-typescript', 'typescript'],
});
