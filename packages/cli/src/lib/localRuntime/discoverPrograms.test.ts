import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { discoverPrograms } from './discoverPrograms';

const tempDirs: string[] = [];

afterEach(async () => {
  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
});

describe('discoverPrograms', () => {
  it('loads exported async functions from configured globs', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-discover-'));
    tempDirs.push(root);
    await fs.mkdir(path.join(root, 'src', 'programs'), { recursive: true });
    await fs.writeFile(
      path.join(root, 'src', 'programs', 'hello.ts'),
      `import { effect, event, waitForEvent } from '@trigora/sdk';

export const greeted = event<{ name: string }>('greeted');

export async function hello(input: { query: string }) {
  const greeting = await effect('greet', () => input.query);
  const who = await waitForEvent(greeted);
  return { greeting, from: who.name };
}
`,
    );

    const programs = await discoverPrograms({
      rootDir: root,
      globs: ['./src/programs/**/*.ts'],
    });

    expect(programs.map((program) => program.id)).toEqual(['hello']);
    expect(programs[0]?.file).toBe('src/programs/hello.ts');
  });
});
