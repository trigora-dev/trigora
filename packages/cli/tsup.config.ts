import { defineConfig } from 'tsup';

export default defineConfig({
  entry: ['src/index.ts'],
  format: ['esm'],
  target: 'node22',
  outDir: 'dist',
  clean: true,
  sourcemap: false,
  splitting: false,
  dts: false,
  shims: false,
  banner: {
    js: '#!/usr/bin/env -S node --experimental-sqlite',
  },
  external: [
    '@tcc-engine/bindings-javascript',
    '@tcc-engine/frontend-typescript',
    '@tcc-engine/host-node',
  ],
});
