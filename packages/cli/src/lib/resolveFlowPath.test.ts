import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { resolveFlowPath } from './resolveFlowPath';

const originalCwd = process.cwd;
const tempDirs: string[] = [];

async function makeTempDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-resolve-flow-'));
  tempDirs.push(dir);
  return dir;
}

beforeEach(() => {
  process.cwd = () => tempDirs[tempDirs.length - 1] ?? originalCwd();
});

afterEach(async () => {
  process.cwd = originalCwd;

  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
});

describe('resolveFlowPath', () => {
  it('resolves a direct file path', async () => {
    const tempDir = await makeTempDir();
    const filePath = path.join(tempDir, 'custom-flow.ts');

    await fs.writeFile(filePath, 'export default {};', 'utf-8');

    const resolved = resolveFlowPath(filePath);

    expect(await fs.realpath(resolved)).toBe(await fs.realpath(filePath));
  });

  it('resolves a flow from the flows directory', async () => {
    const tempDir = await makeTempDir();
    const flowsDir = path.join(tempDir, 'flows');
    const filePath = path.join(flowsDir, 'payment.ts');

    await fs.mkdir(flowsDir, { recursive: true });
    await fs.writeFile(filePath, 'export default {};', 'utf-8');

    const resolved = resolveFlowPath('payment');

    expect(await fs.realpath(resolved)).toBe(await fs.realpath(filePath));
  });

  it('resolves a .js flow from the flows directory', async () => {
    const tempDir = await makeTempDir();
    const flowsDir = path.join(tempDir, 'flows');
    const filePath = path.join(flowsDir, 'payment.js');

    await fs.mkdir(flowsDir, { recursive: true });
    await fs.writeFile(filePath, 'export default {};', 'utf-8');

    const resolved = resolveFlowPath('payment');

    expect(await fs.realpath(resolved)).toBe(await fs.realpath(filePath));
  });

  it('throws a helpful error when the flow cannot be found', () => {
    expect(() => resolveFlowPath('missing-flow')).toThrow(/Could not find flow "missing-flow"\./);
    expect(() => resolveFlowPath('missing-flow')).toThrow(/flows\/missing-flow\.ts/);
    expect(() => resolveFlowPath('missing-flow')).toThrow(/flows\/missing-flow\.js/);
    expect(() => resolveFlowPath('missing-flow')).toThrow(/missing-flow\.ts/);
    expect(() => resolveFlowPath('missing-flow')).toThrow(/missing-flow\.js/);
  });
});
