import { describe, expect, it } from 'vitest';

import { passthroughCompile } from './compiler';

describe('passthroughCompile', () => {
  it('hashes sources into an artifact identity', () => {
    const result = passthroughCompile({
      sourceRoot: '/tmp/project',
      programs: [{ id: 'hello', exportName: 'hello', file: 'hello.ts' }],
      files: [{ path: 'hello.ts', contents: 'export async function hello() {}' }],
    });

    expect(result.ok).toBe(true);
    expect(result.artifact.compilerVersion).toBe('passthrough-local@0.9.0');
    expect(result.artifact.artifactHash).toMatch(/^[a-f0-9]{64}$/);
    expect(result.diagnostics).toEqual([]);
  });
});
