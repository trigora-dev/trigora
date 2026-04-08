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

    const helloFlow = await fs.readFile(path.join(tempDir, 'flows', 'hello.ts'), 'utf-8');
    const payload = await fs.readFile(path.join(tempDir, 'payload.json'), 'utf-8');
    const envExample = await fs.readFile(path.join(tempDir, '.env.example'), 'utf-8');

    expect(helloFlow).toContain("id: 'hello'");
    expect(helloFlow).toContain("trigger: { type: 'manual' }");
    expect(payload).toContain('"message": "Hello, world!"');
    expect(envExample).toContain('# Add environment variables here');

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Created flows\/hello\.ts/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Created payload\.json/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Created \.env\.example/));
  });

  it('does not overwrite existing files by default', async () => {
    const tempDir = await makeTempDir();
    process.chdir(tempDir);

    await fs.mkdir(path.join(tempDir, 'flows'), { recursive: true });
    await fs.writeFile(path.join(tempDir, 'flows', 'hello.ts'), 'custom flow', 'utf-8');
    await fs.writeFile(path.join(tempDir, 'payload.json'), 'custom payload', 'utf-8');
    await fs.writeFile(path.join(tempDir, '.env.example'), 'custom env', 'utf-8');

    await initCommand({ force: false });

    const helloFlow = await fs.readFile(path.join(tempDir, 'flows', 'hello.ts'), 'utf-8');
    const payload = await fs.readFile(path.join(tempDir, 'payload.json'), 'utf-8');
    const envExample = await fs.readFile(path.join(tempDir, '.env.example'), 'utf-8');

    expect(helloFlow).toBe('custom flow');
    expect(payload).toBe('custom payload');
    expect(envExample).toBe('custom env');

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Skipped flows\/hello\.ts \(already exists\)/),
    );
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Skipped payload\.json \(already exists\)/),
    );
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Skipped \.env\.example \(already exists\)/),
    );
  });

  it('overwrites existing files when force is true', async () => {
    const tempDir = await makeTempDir();
    process.chdir(tempDir);

    await fs.mkdir(path.join(tempDir, 'flows'), { recursive: true });
    await fs.writeFile(path.join(tempDir, 'flows', 'hello.ts'), 'custom flow', 'utf-8');
    await fs.writeFile(path.join(tempDir, 'payload.json'), 'custom payload', 'utf-8');
    await fs.writeFile(path.join(tempDir, '.env.example'), 'custom env', 'utf-8');

    await initCommand({ force: true });

    const helloFlow = await fs.readFile(path.join(tempDir, 'flows', 'hello.ts'), 'utf-8');
    const payload = await fs.readFile(path.join(tempDir, 'payload.json'), 'utf-8');
    const envExample = await fs.readFile(path.join(tempDir, '.env.example'), 'utf-8');

    expect(helloFlow).toContain("id: 'hello'");
    expect(payload).toContain('"message": "Hello, world!"');
    expect(envExample).toContain('# Add environment variables here');

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Updated flows\/hello\.ts/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Updated payload\.json/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Updated \.env\.example/));
  });

  it('prints next steps at the end', async () => {
    const tempDir = await makeTempDir();
    process.chdir(tempDir);

    await initCommand({ force: false });

    expect(console.log).toHaveBeenCalledWith('');
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Next steps:/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/trigora trigger hello --payload payload\.json/),
    );
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/trigora dev hello --payload payload\.json/),
    );
  });
});
