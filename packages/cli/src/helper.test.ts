import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { beforeAll, describe, expect, it } from 'vitest';

const cliRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const helper = path.join(cliRoot, 'helper', 'node-helper.js');
const adapter = path.join(cliRoot, 'helper', 'ts-adapter.js');

beforeAll(() => {
  const { status } = spawnSyncBuild();
  expect(status).toBe(0);
});

function spawnSyncBuild(): { status: number | null } {
  return spawnSync(process.execPath, ['scripts/build-adapter.mjs'], {
    cwd: cliRoot,
    encoding: 'utf8',
  });
}

describe('node helper protocol', () => {
  it('compiles a program, runs one effect, and shuts down', async () => {
    const child = spawn(process.execPath, [helper], { stdio: ['pipe', 'pipe', 'pipe'] });
    const lines = readLines(child);
    await send(child, { id: 1, op: 'version' });
    const version = await lines.next();
    expect(version.ok).toBe(true);
    expect(version.compilerVersion).toBe('26.10.1');

    await send(child, {
      id: 2,
      op: 'compile',
      programId: 'program',
      filename: 'src/program.ts',
      source: `import { effect } from '@trigora/sdk';

export default async function program() {
  return await effect('generate', () => 42);
}
`,
    });
    const compiled = await lines.next();
    expect(compiled.ok).toBe(true);
    expect(compiled.effects).toEqual([
      expect.objectContaining({ id: 'program:generate', key: 'generate' }),
    ]);

    await send(child, {
      id: 3,
      op: 'effect',
      handlerId: 'program:generate',
      input: {},
    });
    const effect = await lines.next();
    expect(effect).toMatchObject({ ok: true, value: 42 });

    await send(child, { id: 4, op: 'shutdown' });
    const shutdown = await lines.next();
    expect(shutdown).toMatchObject({ ok: true });
    await new Promise<void>((resolve) => child.once('exit', () => resolve()));
  });
});

describe('published helper packaging', () => {
  it('imports the built adapter and does not load TypeScript sources', () => {
    const helperSource = fs.readFileSync(helper, 'utf8');
    const adapterSource = fs.readFileSync(adapter, 'utf8');
    expect(helperSource).toContain('./ts-adapter.js');
    expect(helperSource).not.toMatch(/from ['"][^'"]+\.ts['"]/);
    expect(adapterSource).not.toMatch(/from ['"][^'"]+\.ts['"]/);
    expect(helperSource).not.toContain('/src/');
    expect(adapterSource).not.toContain('/src/lib/localRuntime/');
  });
});

function send(child: ChildProcessWithoutNullStreams, message: unknown): Promise<void> {
  return new Promise((resolve, reject) => {
    child.stdin.write(`${JSON.stringify(message)}\n`, (error) => {
      if (error) {
        reject(error);
      } else {
        resolve();
      }
    });
  });
}

function readLines(child: ChildProcessWithoutNullStreams): {
  next: () => Promise<Record<string, unknown>>;
} {
  let buffer = '';
  const pending: Array<(line: string) => void> = [];
  child.stdout.on('data', (chunk: Buffer) => {
    buffer += chunk.toString();
    let newline = buffer.indexOf('\n');
    while (newline !== -1) {
      const line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      pending.shift()?.(line);
      newline = buffer.indexOf('\n');
    }
  });
  return {
    next: () =>
      new Promise((resolve) => {
        const finish = (line: string) => resolve(JSON.parse(line) as Record<string, unknown>);
        if (buffer.includes('\n')) {
          const newline = buffer.indexOf('\n');
          const line = buffer.slice(0, newline);
          buffer = buffer.slice(newline + 1);
          finish(line);
          return;
        }
        pending.push(finish);
      }),
  };
}
