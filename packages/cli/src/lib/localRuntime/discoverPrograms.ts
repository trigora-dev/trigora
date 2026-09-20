import fs from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';

import type { ArtifactIdentity, ProgramIdentity } from '@trigora/contracts';
import { CliDisplayError } from '../cliOutput';
import {
  compilePythonProgram,
  compileTypeScriptProgram,
  languageFromFile,
  type CompiledProgramArtifact,
  type ProgramLanguage,
} from './compiler';
import { globFiles } from './globFiles';

export type DiscoveredProgram = ProgramIdentity &
  CompiledProgramArtifact & {
    source: string;
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

function defaultExportName(source: string, fallback: string): string {
  const match = source.match(/export\s+default\s+async\s+function\s+([A-Za-z_$][\w$]*)/);
  return match?.[1] ?? fallback;
}

function fileStem(filePath: string): string {
  return path.basename(filePath, path.extname(filePath));
}

export function workspaceArtifact(programs: DiscoveredProgram[]): ArtifactIdentity {
  const hash = createHash('sha256');
  for (const program of [...programs].sort((left, right) => left.id.localeCompare(right.id))) {
    hash.update(program.id);
    hash.update('\0');
    hash.update(program.artifactHash);
    hash.update('\0');
  }

  return {
    artifactHash: hash.digest('hex'),
    compilerVersion: programs[0]?.compilerVersion ?? 'tcc-engine',
  };
}

export async function discoverPrograms(options: {
  rootDir: string;
  globs: string[];
}): Promise<DiscoveredProgram[]> {
  const files = (await globFiles(options.rootDir, options.globs)).filter((filePath) =>
    Boolean(languageFromFile(filePath)),
  );

  if (files.length === 0) {
    throw new CliDisplayError({
      title: 'No programs found',
      details: [
        { label: 'Globs', value: options.globs.join(', ') },
        { label: 'Root', value: options.rootDir },
      ],
      hint: 'Export a default async function from TypeScript files, or `async def run()` from Python files, matching `programs` in trigora.config.ts.',
    });
  }

  const discovered: DiscoveredProgram[] = [];
  const seen = new Map<string, string>();

  for (const filePath of files) {
    const relative = toPosix(path.relative(options.rootDir, filePath));
    const language = languageFromFile(filePath) as ProgramLanguage;
    const source = await fs.readFile(filePath, 'utf-8');
    const compiled =
      language === 'python'
        ? await compilePythonProgram(source, relative)
        : compileTypeScriptProgram(source, relative);
    const exportName = language === 'python' ? 'run' : defaultExportName(source, 'default');
    const id = language === 'python' || exportName === 'default' ? fileStem(filePath) : exportName;
    const previous = seen.get(id);

    if (previous) {
      throw new CliDisplayError({
        title: 'Duplicate program id',
        details: [
          { label: 'Program', value: id },
          { label: 'First', value: previous },
          { label: 'Second', value: relative },
        ],
        hint: 'Use a unique default-export function name or file name for each program.',
      });
    }

    seen.set(id, relative);
    discovered.push({
      id,
      exportName,
      file: relative,
      source,
      ...compiled,
    });
  }

  return discovered.sort((left, right) => left.id.localeCompare(right.id));
}
