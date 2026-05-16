import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { buildDeploymentManifest } from './buildDeploymentManifest';

const originalCwd = process.cwd();
const tempDirs: string[] = [];

async function makeTempDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-build-deployment-manifest-'));
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

describe('buildDeploymentManifest', () => {
  it('builds a manifest for a specific webhook flow', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(
      buildDeploymentManifest({
        filePath: flowPath,
      }),
    ).resolves.toEqual({
      version: 1,
      flow: {
        id: 'hello',
        entrypoint: 'flows/hello.ts',
        trigger: { type: 'webhook' },
      },
    });
  });

  it('auto-picks the only flow in the flows directory', async () => {
    const tempDir = await makeTempDir();
    const helloPath = path.join(tempDir, 'flows', 'hello.ts');

    await fs.mkdir(path.dirname(helloPath), { recursive: true });
    await fs.writeFile(
      helloPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );
    process.chdir(tempDir);

    await expect(buildDeploymentManifest({})).resolves.toEqual({
      version: 1,
      flow: {
        id: 'hello',
        entrypoint: 'flows/hello.ts',
        trigger: { type: 'webhook' },
      },
    });
  });

  it('throws a helpful error when multiple flows exist and none is selected', async () => {
    const tempDir = await makeTempDir();
    const helloPath = path.join(tempDir, 'flows', 'hello.ts');
    const nightlyPath = path.join(tempDir, 'flows', 'nightly.ts');

    await fs.mkdir(path.dirname(helloPath), { recursive: true });
    await fs.writeFile(
      helloPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );
    await fs.writeFile(
      nightlyPath,
      `
        export default {
          id: 'nightly',
          trigger: { type: 'cron', cron: '0 2 * * *' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(buildDeploymentManifest({})).rejects.toThrow(
      'Multiple flows found. Pass a flow name or file path to deploy one flow.',
    );
  });

  it('throws a helpful error when no flows exist', async () => {
    const tempDir = await makeTempDir();
    process.chdir(tempDir);

    await expect(buildDeploymentManifest({})).rejects.toThrow(
      'No flow files found in "flows". Create one with "trigora init" or pass a specific flow to "trigora deploy <flow>".',
    );
  });

  it('throws a helpful error when a flow uses an unsupported trigger', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
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

    await expect(
      buildDeploymentManifest({
        filePath: flowPath,
      }),
    ).rejects.toThrow(
      'Flow "hello" in "flows/hello.ts" uses unsupported trigger "manual". trigora deploy currently supports only webhook- and cron-triggered flows.',
    );
  });

  it('requires an explicit selection when multiple flow files are present', async () => {
    const tempDir = await makeTempDir();
    const firstPath = path.join(tempDir, 'flows', 'hello.ts');
    const secondPath = path.join(tempDir, 'flows', 'nested', 'hello-copy.ts');

    await fs.mkdir(path.dirname(secondPath), { recursive: true });
    await fs.writeFile(
      firstPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );
    await fs.writeFile(
      secondPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(buildDeploymentManifest({})).rejects.toThrow(
      'Multiple flows found. Pass a flow name or file path to deploy one flow.',
    );
  });
});
