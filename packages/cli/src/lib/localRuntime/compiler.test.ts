import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { CliDisplayError } from '../cliOutput';
import { compileTypeScriptProgram } from './compiler';

describe('compileTypeScriptProgram', () => {
  it('compiles a supported default-export program', () => {
    const compiled = compileTypeScriptProgram(
      `import { effect, waitForEvent } from "@trigora/sdk";

export default async function approval() {
  const result = await effect("generate", () => 42);
  const approval = await waitForEvent("approved");
  return { result, approval };
}
`,
      'approval.ts',
    );

    expect(compiled.language).toBe('typescript');
    expect(compiled.artifactHash).toMatch(/^[a-f0-9]{64}$/);
    expect(compiled.effects.generate?.()).toBe(42);
  });

  it('compiles JavaScript-style source as typescript', () => {
    const compiled = compileTypeScriptProgram(
      `import { effect } from "@trigora/sdk";

export default async function run(a, b) {
  const total = a + b;
  const doubled = await effect("double", () => 1);
  return doubled;
}
`,
      'run.js',
    );

    expect(compiled.language).toBe('typescript');
    expect(compiled.effects.double?.()).toBe(1);
  });

  it('names an anonymous default export "default"', () => {
    const compiled = compileTypeScriptProgram(
      `export default async function () {
  return 1;
}
`,
      'program.ts',
    );
    const artifact = JSON.parse(compiled.artifactJson) as {
      program: { entry: number; functions: Array<{ id: number; name: string }> };
    };
    const entry = artifact.program.functions.find((fn) => fn.id === artifact.program.entry);
    expect(entry?.name).toBe('default');
  });

  it('extracts effect callback source for Cloud workers', async () => {
    const { extractTypeScriptEffectSources } = await import('./extractEffects');
    const sources = extractTypeScriptEffectSources(
      `import { effect, waitForEvent } from "@trigora/sdk";

export default async function approval() {
  const result = await effect("generate", () => 42);
  const approval = await waitForEvent("approved");
  return { result, approval };
}
`,
      'approval.ts',
    );

    expect(sources.generate).toContain('42');
  });

  it('rejects named exports that are not the default entry', () => {
    expect(() =>
      compileTypeScriptProgram(
        `import { effect } from "@trigora/sdk";
export async function approval() {
  return effect("generate", () => 42);
}
`,
        'approval.ts',
      ),
    ).toThrow(/unsupported top-level statement|default export/);
  });
});

describe('discoverPrograms', () => {
  const tempDirs: string[] = [];

  afterEach(async () => {
    await Promise.all(
      tempDirs.splice(0).map(async (dir) => {
        await fs.rm(dir, { recursive: true, force: true });
      }),
    );
  });

  it('loads a default-export program from configured globs', async () => {
    const { discoverPrograms } = await import('./discoverPrograms');
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-discover-'));
    tempDirs.push(root);
    await fs.mkdir(path.join(root, 'src', 'programs'), { recursive: true });
    await fs.writeFile(
      path.join(root, 'src', 'programs', 'hello.ts'),
      `import { effect, waitForEvent } from '@trigora/sdk';

export default async function hello() {
  const greeting = await effect('greet', () => 'hello');
  const who = await waitForEvent('greeted');
  return { greeting, from: who };
}
`,
    );

    const programs = await discoverPrograms({
      rootDir: root,
      globs: ['./src/programs/**/*.ts'],
    });

    expect(programs.map((program) => program.id)).toEqual(['hello']);
    expect(programs[0]?.file).toBe('src/programs/hello.ts');
    expect(programs[0]?.exportName).toBe('hello');
  });

  it('compiles a Python approval program when tcc_engine is installed', async () => {
    const { compilePythonProgram } = await import('./compiler');
    try {
      const compiled = await compilePythonProgram(
        `from trigora import effect, program, wait_for_event

@program
async def approval():
    result = await effect("generate", lambda: 42)
    review = await wait_for_event("approved")
    return {"result": result, "review": review}
`,
        'approval.py',
      );
      expect(compiled.language).toBe('python');
      expect(compiled.artifactHash).toMatch(/^[a-f0-9]{64}$/);
      expect(compiled.effects.generate?.()).toBe(42);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const title = error instanceof CliDisplayError ? error.title : '';
      if (/tcc_engine|Python compiler unavailable/.test(`${title} ${message}`)) {
        return;
      }
      throw error;
    }
  });
});
