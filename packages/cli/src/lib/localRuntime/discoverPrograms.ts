import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import fs from 'node:fs/promises';
import path from 'node:path';

import type { ArtifactIdentity, ProgramIdentity } from '@trigora/contracts';
import { CliDisplayError } from '../cliOutput';
import {
  compilePythonProgram,
  compileTypeScriptProgram,
  compileRustProgram,
  languageFromFile,
  type CompiledProgramArtifact,
  type ProgramLanguage,
} from './compiler';
import { globFiles } from './globFiles';
import { findCargoToml } from './rustEffects';

export type DiscoveredProgram = ProgramIdentity &
  CompiledProgramArtifact & {
    source: string;
    deployed?: boolean;
    blocksExecution?: boolean;
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

function exportNameFromArtifact(artifactJson: string, file: string): string {
  let artifact: {
    program?: { entry?: unknown; functions?: Array<{ id?: unknown; name?: unknown }> };
  };
  try {
    artifact = JSON.parse(artifactJson) as typeof artifact;
  } catch {
    throw new CliDisplayError({
      title: 'Invalid program artifact',
      details: [
        { label: 'File', value: file },
        { label: 'Reason', value: 'The compiler did not return JSON.' },
      ],
    });
  }

  const entry = artifact.program?.entry;
  const functions = artifact.program?.functions;
  const func = Array.isArray(functions)
    ? (functions.find((item) => item.id === entry) ??
      (typeof entry === 'number' ? functions[entry] : undefined))
    : undefined;
  if (entry === undefined || entry === null || typeof func?.name !== 'string' || func.name === '') {
    throw new CliDisplayError({
      title: 'Invalid program artifact',
      details: [
        { label: 'File', value: file },
        { label: 'Reason', value: 'program.entry does not name a function.' },
      ],
    });
  }
  return func.name;
}

function fileStem(filePath: string): string {
  return path.basename(filePath, path.extname(filePath));
}

function rustProgramId(stem: string, filePath: string, rootDir: string): string {
  if (stem !== 'lib' && stem !== 'main') {
    return stem;
  }
  const cargoPath = findCargoToml(path.dirname(filePath), rootDir);
  if (!cargoPath) {
    return stem;
  }
  const match = readFileSync(cargoPath, 'utf8').match(/^name\s*=\s*"([^"]+)"/m);
  return match?.[1] || stem;
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
      hint: 'TypeScript default-exports an async program entry, Python marks one with `@program`, and Rust uses `pub async fn main`. Point `[project].programs` at those files in trigora.toml.',
    });
  }

  const discovered: DiscoveredProgram[] = [];
  const seen = new Map<string, string>();

  for (const filePath of files) {
    const relative = toPosix(path.relative(options.rootDir, filePath));
    const language = languageFromFile(filePath) as ProgramLanguage;
    const source = await fs.readFile(filePath, 'utf-8');
    const stem = fileStem(filePath);
    const id =
      language === 'rust'
        ? rustProgramId(stem, filePath, options.rootDir)
        : language === 'python' || defaultExportName(source, 'default') === 'default'
          ? stem
          : defaultExportName(source, 'default');
    const compiled =
      language === 'python'
        ? await compilePythonProgram(source, relative)
        : language === 'rust'
          ? await compileRustProgram(source, relative, { rootDir: options.rootDir, programId: id })
          : compileTypeScriptProgram(source, relative);
    const exportName = exportNameFromArtifact(compiled.artifactJson, relative);
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
