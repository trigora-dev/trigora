import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { initCommand } from './init';

const originalCwd = process.cwd();
const originalConsoleLog = console.log;

const tempDirs: string[] = [];

async function makeTempDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-init-command-'));
  tempDirs.push(dir);
  return dir;
}

beforeEach(() => {
  console.log = vi.fn();
});

afterEach(async () => {
  process.chdir(originalCwd);
  console.log = originalConsoleLog;

  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
});

describe('initCommand', () => {
  it('creates starter files in the current directory', async () => {
    const tempDir = await makeTempDir();
    process.chdir(tempDir);

    await initCommand({ force: false });

    const config = await fs.readFile(path.join(tempDir, 'trigora.config.ts'), 'utf-8');
    const hello = await fs.readFile(path.join(tempDir, 'src', 'programs', 'hello.ts'), 'utf-8');
    const envExample = await fs.readFile(path.join(tempDir, '.env.example'), 'utf-8');

    expect(config).toContain("programs: './src/programs/**/*.ts'");
    expect(hello).toContain('export async function hello');
    expect(hello).toContain('waitForEvent(greeted)');
    expect(envExample).toContain('TRIGORA_RUNTIME_URL=http://127.0.0.1:3477');

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Project initialized/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Created/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/trigora\.config\.ts/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/src\/programs\/hello\.ts/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/\.env\.example/));
  });

  it('does not overwrite existing files by default', async () => {
    const tempDir = await makeTempDir();
    process.chdir(tempDir);

    await fs.mkdir(path.join(tempDir, 'src', 'programs'), { recursive: true });
    await fs.writeFile(path.join(tempDir, 'trigora.config.ts'), 'custom config', 'utf-8');
    await fs.writeFile(
      path.join(tempDir, 'src', 'programs', 'hello.ts'),
      'custom program',
      'utf-8',
    );
    await fs.writeFile(path.join(tempDir, '.env.example'), 'custom env', 'utf-8');

    await initCommand({ force: false });

    const config = await fs.readFile(path.join(tempDir, 'trigora.config.ts'), 'utf-8');
    const hello = await fs.readFile(path.join(tempDir, 'src', 'programs', 'hello.ts'), 'utf-8');
    const envExample = await fs.readFile(path.join(tempDir, '.env.example'), 'utf-8');

    expect(config).toBe('custom config');
    expect(hello).toBe('custom program');
    expect(envExample).toBe('custom env');

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Skipped/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/trigora\.config\.ts/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/src\/programs\/hello\.ts/));
  });

  it('overwrites existing files when force is true', async () => {
    const tempDir = await makeTempDir();
    process.chdir(tempDir);

    await fs.mkdir(path.join(tempDir, 'src', 'programs'), { recursive: true });
    await fs.writeFile(path.join(tempDir, 'trigora.config.ts'), 'custom config', 'utf-8');
    await fs.writeFile(
      path.join(tempDir, 'src', 'programs', 'hello.ts'),
      'custom program',
      'utf-8',
    );
    await fs.writeFile(path.join(tempDir, '.env.example'), 'custom env', 'utf-8');

    await initCommand({ force: true });

    const config = await fs.readFile(path.join(tempDir, 'trigora.config.ts'), 'utf-8');
    const hello = await fs.readFile(path.join(tempDir, 'src', 'programs', 'hello.ts'), 'utf-8');
    const envExample = await fs.readFile(path.join(tempDir, '.env.example'), 'utf-8');

    expect(config).toContain('defineConfig');
    expect(hello).toContain('export async function hello');
    expect(envExample).toContain('TRIGORA_RUNTIME_URL');

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Updated/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/trigora\.config\.ts/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/src\/programs\/hello\.ts/));
  });

  it('prints next steps at the end', async () => {
    const tempDir = await makeTempDir();
    process.chdir(tempDir);

    await initCommand({ force: false });

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Next steps/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/1\..*trigora dev/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/2\..*@trigora\/client/));
  });
});
