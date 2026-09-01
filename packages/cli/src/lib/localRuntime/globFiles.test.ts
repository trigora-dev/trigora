import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { globFiles } from './globFiles';

const tempDirs: string[] = [];

afterEach(async () => {
  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
});

describe('globFiles', () => {
  it('expands ** globs and exact paths', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-glob-'));
    tempDirs.push(root);
    await fs.mkdir(path.join(root, 'src', 'programs'), { recursive: true });
    await fs.writeFile(
      path.join(root, 'src', 'programs', 'hello.ts'),
      'export async function hello() {}',
    );
    await fs.writeFile(
      path.join(root, 'src', 'programs', 'agent.ts'),
      'export async function agent() {}',
    );
    await fs.writeFile(path.join(root, 'README.md'), '# skip');

    const matched = await globFiles(root, ['./src/programs/**/*.ts']);
    expect(matched.map((file) => path.basename(file)).sort()).toEqual(['agent.ts', 'hello.ts']);

    const exact = await globFiles(root, ['src/programs/hello.ts']);
    expect(exact).toHaveLength(1);
    expect(path.basename(exact[0] ?? '')).toBe('hello.ts');
  });
});
