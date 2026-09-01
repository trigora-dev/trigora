import { createHash } from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';

import type {
  CompileRequest,
  CompileResult,
  CompilerDiagnostic,
  ProgramIdentity,
} from '@trigora/contracts';
import { CliDisplayError } from '../cliOutput';

export const PASSTHROUGH_COMPILER_VERSION = 'passthrough-local@0.9.0';

function hashSources(files: CompileRequest['files']): string {
  const hash = createHash('sha256');

  for (const file of [...files].sort((left, right) => left.path.localeCompare(right.path))) {
    hash.update(file.path);
    hash.update('\0');
    hash.update(file.contents);
    hash.update('\0');
  }

  return hash.digest('hex');
}

export function passthroughCompile(request: CompileRequest): CompileResult {
  return {
    ok: true,
    artifact: {
      artifactHash: hashSources(request.files),
      compilerVersion: PASSTHROUGH_COMPILER_VERSION,
    },
    diagnostics: [],
  };
}

export function formatCompilerDiagnostics(diagnostics: CompilerDiagnostic[]): string[] {
  return diagnostics.map((diagnostic) => {
    const location = diagnostic.file
      ? `${diagnostic.file}${diagnostic.start ? `:${diagnostic.start.line}:${diagnostic.start.column}` : ''}`
      : undefined;
    const lines = [
      `${diagnostic.severity === 'error' ? 'error' : 'warning'} ${diagnostic.code}: ${diagnostic.message}`,
    ];

    if (location) {
      lines.push(`  ${location}`);
    }

    if (diagnostic.hint) {
      lines.push(`  ${diagnostic.hint}`);
    }

    return lines.join('\n');
  });
}

export async function compilePrograms(
  request: CompileRequest,
  endpoint?: string,
): Promise<CompileResult> {
  if (!endpoint) {
    return passthroughCompile(request);
  }

  let response: Response;

  try {
    response = await fetch(new URL('/v1/compile', endpoint), {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(request),
    });
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new CliDisplayError({
      title: 'Compiler unreachable',
      details: [
        { label: 'Endpoint', value: endpoint },
        { label: 'Reason', value: reason },
      ],
    });
  }

  const result = (await response.json()) as CompileResult;

  if (!response.ok || !result.ok) {
    throw new CliDisplayError({
      title: 'Compilation failed',
      details: [
        { label: 'Endpoint', value: endpoint },
        {
          label: 'Diagnostics',
          value:
            formatCompilerDiagnostics(result.diagnostics ?? []).join('\n') ||
            'Unknown compiler error.',
        },
      ],
    });
  }

  return result;
}

export async function readCompileFiles(
  rootDir: string,
  programs: ProgramIdentity[],
): Promise<CompileRequest['files']> {
  const uniqueFiles = [...new Set(programs.map((program) => program.file))];

  return Promise.all(
    uniqueFiles.map(async (relativePath) => ({
      path: relativePath,
      contents: await fs.readFile(path.join(rootDir, relativePath), 'utf-8'),
    })),
  );
}
