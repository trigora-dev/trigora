import { defineConfig } from 'tsup';

export default defineConfig({
  entry: ['src/index.ts', 'src/github.ts', 'src/stripe.ts', 'src/runtime.ts'],
  format: ['esm'],
  target: 'node20',
  outDir: 'dist',
  clean: true,
  sourcemap: false,
  splitting: true,
  dts: false,
  shims: false,
  external: ['@trigora/contracts'],
});
