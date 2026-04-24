import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { buildDeploymentArtifact } from './buildDeploymentArtifact';

const originalCwd = process.cwd();
const tempDirs: string[] = [];

async function makeTempDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-build-deployment-artifact-'));
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

describe('buildDeploymentArtifact', () => {
  it('bundles flow entrypoints for deployment', async () => {
    const tempDir = await makeTempDir();
    const helloPath = path.join(tempDir, 'flows', 'hello.ts');
    const ordersPath = path.join(tempDir, 'flows', 'nested', 'orders.ts');
    const sharedPath = path.join(tempDir, 'flows', 'shared.ts');

    await fs.mkdir(path.dirname(ordersPath), { recursive: true });
    await fs.writeFile(sharedPath, `export const message = 'from helper';`, 'utf-8');
    await fs.writeFile(
      helloPath,
      `
        import { message } from './shared';

        export default {
          id: 'hello',
          message,
        };
      `,
      'utf-8',
    );
    await fs.writeFile(ordersPath, `export default { id: 'orders' };`, 'utf-8');

    process.chdir(tempDir);

    const artifact = await buildDeploymentArtifact({
      version: 1,
      flows: [
        {
          id: 'hello',
          entrypoint: 'flows/hello.ts',
          routePath: '/hello',
          trigger: { type: 'webhook' },
        },
        {
          id: 'orders',
          entrypoint: 'flows/nested/orders.ts',
          routePath: '/orders',
          trigger: { type: 'webhook', event: 'orders.created' },
        },
      ],
    });

    expect(artifact.version).toBe(1);
    expect(artifact.format).toBe('esm');
    expect(artifact.target).toBe('node20');
    expect(artifact.files).toHaveLength(2);

    expect(artifact.files[0]).toEqual({
      entrypoint: 'flows/hello.ts',
      path: 'flows/hello.mjs',
      contents: expect.any(String),
    });
    expect(artifact.files[1]).toEqual({
      entrypoint: 'flows/nested/orders.ts',
      path: 'flows/nested/orders.mjs',
      contents: expect.any(String),
    });

    expect(artifact.files[0]?.contents).toContain('from helper');
    expect(artifact.files[0]?.contents).not.toContain("from './shared'");
  });

  it('wraps bundling failures with the entrypoint path', async () => {
    const tempDir = await makeTempDir();
    const helloPath = path.join(tempDir, 'flows', 'hello.ts');

    await fs.mkdir(path.dirname(helloPath), { recursive: true });
    await fs.writeFile(
      helloPath,
      `
        import { missing } from './missing';

        export default {
          id: 'hello',
          missing,
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(
      buildDeploymentArtifact({
        version: 1,
        flows: [
          {
            id: 'hello',
            entrypoint: 'flows/hello.ts',
            routePath: '/hello',
            trigger: { type: 'webhook' },
          },
        ],
      }),
    ).rejects.toThrow(/Failed to bundle flow "flows\/hello\.ts" for deployment:/);
  });
});
