import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['**/*.{test,spec}.ts'],
    environment: 'node',
    passWithNoTests: true,
    hideSkippedTests: true,
    pool: 'forks',
    execArgv: ['--experimental-sqlite'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html'],
    },
  },
});
