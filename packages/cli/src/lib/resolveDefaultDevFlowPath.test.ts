import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { resolveDefaultDevFlowPath } from './resolveDefaultDevFlowPath';

const tempDirs: string[] = [];

async function makeTempDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-resolve-default-dev-flow-'));
  tempDirs.push(dir);
  return dir;
}

afterEach(async () => {
  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
});

describe('resolveDefaultDevFlowPath', () => {
  it('resolves the only flow in the flows directory', async () => {
    const tempDir = await makeTempDir();
    const flowsDir = path.join(tempDir, 'flows');
    const filePath = path.join(flowsDir, 'stripe-checkout.ts');

    await fs.mkdir(flowsDir, { recursive: true });
    await fs.writeFile(filePath, 'export default {};', 'utf-8');

    const resolved = await resolveDefaultDevFlowPath(tempDir);

    expect(await fs.realpath(resolved)).toBe(await fs.realpath(filePath));
  });

  it('throws a helpful error when no flows exist', async () => {
    const tempDir = await makeTempDir();

    await expect(resolveDefaultDevFlowPath(tempDir)).rejects.toThrow(
      'No flow files found in "flows". Create one with "trigora init" or pass a specific flow to "trigora dev <flow>".',
    );
  });

  it('throws a helpful error when multiple flows exist', async () => {
    const tempDir = await makeTempDir();
    const flowsDir = path.join(tempDir, 'flows');

    await fs.mkdir(flowsDir, { recursive: true });
    await fs.writeFile(path.join(flowsDir, 'a.ts'), 'export default {};', 'utf-8');
    await fs.writeFile(path.join(flowsDir, 'b.ts'), 'export default {};', 'utf-8');

    await expect(resolveDefaultDevFlowPath(tempDir)).rejects.toThrow(
      'Multiple flow files found in "flows". Pass a specific flow to "trigora dev <flow>".',
    );
  });
});
