import fs from 'node:fs/promises';
import path from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';

import { build, type Plugin } from 'esbuild';
import type { ProgramIdentity } from '@trigora/contracts';
import { CliDisplayError } from '../cliOutput';
import { globFiles } from './globFiles';

const require = createRequire(import.meta.url);

export type DiscoveredProgram = ProgramIdentity & {
  fn: (input: never) => Promise<unknown>;
};

export function toProgramIdentity(program: DiscoveredProgram): ProgramIdentity {
  return {
    id: program.id,
    exportName: program.exportName,
    file: program.file,
  };
}

function toPosix(filePath: string): string {
  return filePath.split(path.sep).join('/');
}

function sdkSingletonPlugin(): Plugin {
  return {
    name: 'trigora-sdk-singleton',
    setup(buildApi) {
      buildApi.onResolve({ filter: /^@trigora\/(?:sdk|contracts)(?:\/.*)?$/ }, (args) => {
        try {
          return {
            path: require.resolve(args.path),
            external: true,
          };
        } catch {
          return {
            path: args.path,
            external: true,
          };
        }
      });
    },
  };
}

async function bundleProgramModule(filePath: string, outFile: string): Promise<void> {
  await build({
    absWorkingDir: path.dirname(filePath),
    bundle: true,
    entryPoints: [filePath],
    format: 'esm',
    keepNames: true,
    logLevel: 'silent',
    outfile: outFile,
    packages: 'external',
    platform: 'node',
    plugins: [sdkSingletonPlugin()],
    sourcemap: 'inline',
    target: 'node20',
  });
}

function isProgramFunction(value: unknown): value is (input: never) => Promise<unknown> {
  return typeof value === 'function' && value.constructor.name === 'AsyncFunction';
}

export async function discoverPrograms(options: {
  rootDir: string;
  globs: string[];
}): Promise<DiscoveredProgram[]> {
  const files = await globFiles(options.rootDir, options.globs);

  if (files.length === 0) {
    throw new CliDisplayError({
      title: 'No programs found',
      details: [
        { label: 'Globs', value: options.globs.join(', ') },
        { label: 'Root', value: options.rootDir },
      ],
      hint: 'Export named async functions from files matching `programs` in trigora.config.ts.',
    });
  }

  const cacheDir = path.join(options.rootDir, '.trigora', 'dev');
  await fs.mkdir(cacheDir, { recursive: true });

  const discovered: DiscoveredProgram[] = [];
  const seen = new Map<string, string>();

  for (const filePath of files) {
    const relative = toPosix(path.relative(options.rootDir, filePath));
    const outFile = path.join(
      cacheDir,
      `${relative.replaceAll('/', '__')}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`,
    );

    try {
      await bundleProgramModule(filePath, outFile);
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      throw new CliDisplayError({
        title: 'Failed to load program module',
        details: [
          { label: 'File', value: relative },
          { label: 'Reason', value: reason },
        ],
      });
    }

    const imported = (await import(pathToFileURL(outFile).href)) as Record<string, unknown>;

    for (const [exportName, value] of Object.entries(imported)) {
      if (!isProgramFunction(value)) {
        continue;
      }

      const id = exportName === 'default' ? value.name : exportName;

      if (!id) {
        continue;
      }

      const previous = seen.get(id);
      if (previous) {
        throw new CliDisplayError({
          title: 'Duplicate program id',
          details: [
            { label: 'Program', value: id },
            { label: 'First', value: previous },
            { label: 'Second', value: relative },
          ],
          hint: 'Export names must be unique across discovered program files.',
        });
      }

      seen.set(id, relative);
      discovered.push({
        id,
        exportName,
        file: relative,
        fn: value,
      });
    }
  }

  if (discovered.length === 0) {
    throw new CliDisplayError({
      title: 'No program exports found',
      details: [
        {
          label: 'Files',
          value: files.map((file) => toPosix(path.relative(options.rootDir, file))).join(', '),
        },
      ],
      hint: 'Export a named async function such as `export async function researchAgent(input) { ... }`.',
    });
  }

  return discovered.sort((left, right) => left.id.localeCompare(right.id));
}
