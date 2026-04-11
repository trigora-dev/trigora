import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { loadProjectEnv } from './loadProjectEnv';

const originalCwd = process.cwd();
const originalEnv = { ...process.env };
const tempDirs: string[] = [];

async function makeTempDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-load-project-env-'));
  tempDirs.push(dir);
  return dir;
}

afterEach(async () => {
  process.chdir(originalCwd);
  process.env = { ...originalEnv };

  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
});

describe('loadProjectEnv', () => {
  it('loads variables from .env', async () => {
    const tempDir = await makeTempDir();

    await fs.writeFile(
      path.join(tempDir, '.env'),
      `
        TRIGORA_DEPLOY_TOKEN=token-from-env
      `,
      'utf-8',
    );

    process.chdir(tempDir);
    delete process.env.TRIGORA_DEPLOY_TOKEN;

    loadProjectEnv();

    expect(process.env.TRIGORA_DEPLOY_TOKEN).toBe('token-from-env');
  });

  it('allows .env.local to override .env values', async () => {
    const tempDir = await makeTempDir();

    await fs.writeFile(path.join(tempDir, '.env'), `TRIGORA_DEPLOY_TOKEN=base-token`, 'utf-8');
    await fs.writeFile(
      path.join(tempDir, '.env.local'),
      `TRIGORA_DEPLOY_TOKEN=local-token`,
      'utf-8',
    );

    process.chdir(tempDir);
    delete process.env.TRIGORA_DEPLOY_TOKEN;

    loadProjectEnv();

    expect(process.env.TRIGORA_DEPLOY_TOKEN).toBe('local-token');
  });

  it('preserves existing shell environment variables', async () => {
    const tempDir = await makeTempDir();

    await fs.writeFile(path.join(tempDir, '.env'), `TRIGORA_DEPLOY_TOKEN=file-token`, 'utf-8');

    process.chdir(tempDir);
    process.env.TRIGORA_DEPLOY_TOKEN = 'shell-token';

    loadProjectEnv();

    expect(process.env.TRIGORA_DEPLOY_TOKEN).toBe('shell-token');
  });

  it('parses quoted values and ignores comments', async () => {
    const tempDir = await makeTempDir();

    await fs.writeFile(
      path.join(tempDir, '.env'),
      `
        # comment
        TRIGORA_DEPLOY_TOKEN="quoted-token"
        INVALID LINE
      `,
      'utf-8',
    );

    process.chdir(tempDir);
    delete process.env.TRIGORA_DEPLOY_TOKEN;

    loadProjectEnv();

    expect(process.env.TRIGORA_DEPLOY_TOKEN).toBe('quoted-token');
  });

  it('does nothing when env files are missing', async () => {
    const tempDir = await makeTempDir();

    process.chdir(tempDir);
    delete process.env.TRIGORA_DEPLOY_TOKEN;

    loadProjectEnv();

    expect(process.env.TRIGORA_DEPLOY_TOKEN).toBeUndefined();
  });
});
