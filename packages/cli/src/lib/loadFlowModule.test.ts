import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';
import { loadFlowModule } from './loadFlowModule';

const originalCwd = process.cwd();
const tempDirs: string[] = [];

async function makeTempDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-load-flow-'));
  tempDirs.push(dir);
  return dir;
}

afterEach(async () => {
  process.chdir(originalCwd);

  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
});

describe('loadFlowModule', () => {
  it('loads a valid flow module', async () => {
    const tempDir = await makeTempDir();

    const filePath = path.join(tempDir, 'hello.ts');

    await fs.writeFile(
      filePath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'manual' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    const flow = await loadFlowModule('./hello.ts');

    expect(flow.id).toBe('hello');
    expect(flow.trigger).toEqual({ type: 'manual' });
    expect(typeof flow.run).toBe('function');
  });

  it('throws a helpful error when default export is missing', async () => {
    const tempDir = await makeTempDir();

    const filePath = path.join(tempDir, 'missing-default.ts');

    await fs.writeFile(
      filePath,
      `
        export const value = 123;
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(loadFlowModule('./missing-default.ts')).rejects.toThrow(
      'No default export found in "./missing-default.ts". Expected a default exported flow.',
    );
  });

  it('throws a helpful error when the module is invalid', async () => {
    const tempDir = await makeTempDir();

    const filePath = path.join(tempDir, 'invalid.ts');

    await fs.writeFile(
      filePath,
      `
        export default {
          id: '',
          trigger: { type: 'manual' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(loadFlowModule('./invalid.ts')).rejects.toThrow(
      'Invalid flow in "./invalid.ts": "id" must be a non-empty string.',
    );
  });

  it('wraps import errors with the file path', async () => {
    const tempDir = await makeTempDir();

    process.chdir(tempDir);

    await expect(loadFlowModule('./does-not-exist.ts')).rejects.toThrow(
      /Failed to import flow file "\.\/does-not-exist\.ts":/,
    );
  });
});
