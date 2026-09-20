import { spawn } from 'node:child_process';
import path from 'node:path';

import {
  compile as compileTypeScript,
  CompileError,
  PACKAGE_VERSION,
} from '@tcc-engine/frontend-typescript';
import { CliDisplayError } from '../cliOutput';
import {
  extractTypeScriptEffects,
  PYTHON_EFFECT_SCRIPT,
  PYTHON_EFFECT_SOURCE_SCRIPT,
} from './extractEffects';

export type ProgramLanguage = 'typescript' | 'python';

export type CompiledProgramArtifact = {
  language: ProgramLanguage;
  artifactJson: string;
  artifactHash: string;
  compilerVersion: string;
  effects: Record<string, () => unknown>;
};

const PYTHON_COMPILE_SCRIPT = `
import json, sys
from tcc_engine import CompileError, artifact_json, compile

filename = sys.argv[1]
source = sys.stdin.read()
try:
    artifact = compile(source, filename=filename)
    sys.stdout.write(artifact_json(artifact))
except CompileError as err:
    payload = {"ok": False, "message": str(err), "file": getattr(err, "filename", filename)}
    span = getattr(err, "span", None)
    if span:
        payload["span"] = span
    json.dump(payload, sys.stderr)
    sys.exit(2)
`;

function runPython(script: string, filename: string, source: string, extraArgs: string[] = []): Promise<{
  stdout: string;
  stderr: string;
  status: number | null;
}> {
  return new Promise((resolve, reject) => {
    const child = spawn('python3', ['-c', script, filename, ...extraArgs], {
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => {
      stdout += String(chunk);
    });
    child.stderr.on('data', (chunk) => {
      stderr += String(chunk);
    });
    child.on('error', reject);
    child.on('close', (status) => {
      resolve({ stdout, stderr, status });
    });
    child.stdin.end(source);
  });
}

function isCompileError(error: unknown): error is CompileError {
  return error instanceof Error && error.name === 'CompileError';
}

function throwCompileFailure(title: string, file: string, message: string, span?: {
  start_line?: number;
  start_column?: number;
}): never {
  const location =
    span?.start_line === undefined
      ? file
      : `${file}:${span.start_line}:${span.start_column ?? 1}`;
  throw new CliDisplayError({
    title,
    message,
    details: [
      { label: 'File', value: location },
      { label: 'Reason', value: message },
    ],
  });
}

export function compileTypeScriptProgram(source: string, filename: string): CompiledProgramArtifact {
  try {
    const artifact = compileTypeScript(source, { filename });
    return {
      language: 'typescript',
      artifactJson: JSON.stringify(artifact),
      artifactHash: artifact.envelope.artifact_hash,
      compilerVersion: PACKAGE_VERSION,
      effects: extractTypeScriptEffects(source, filename),
    };
  } catch (error) {
    if (isCompileError(error)) {
      throwCompileFailure('Compilation failed', error.file || filename, error.message, error.span ?? undefined);
    }
    throw error;
  }
}

export async function compilePythonProgram(source: string, filename: string): Promise<CompiledProgramArtifact> {
  let compiled;
  try {
    compiled = await runPython(PYTHON_COMPILE_SCRIPT, filename, source);
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new CliDisplayError({
      title: 'Python compiler unavailable',
      details: [
        { label: 'File', value: filename },
        { label: 'Reason', value: reason },
      ],
      hint: 'Install the local tcc_engine wheel from dist-packages/, then retry.',
    });
  }

  if (compiled.status !== 0) {
    let payload: { message?: string; file?: string; span?: { start_line?: number; start_column?: number } } = {};
    try {
      payload = JSON.parse(compiled.stderr || compiled.stdout) as typeof payload;
    } catch {
      payload = { message: compiled.stderr.trim() || compiled.stdout.trim() || 'Python compilation failed.' };
    }

    if (/No module named ['"]tcc_engine['"]/.test(payload.message ?? compiled.stderr)) {
      throw new CliDisplayError({
        title: 'Python compiler unavailable',
        details: [
          { label: 'File', value: filename },
          { label: 'Reason', value: 'The tcc_engine package is not installed.' },
        ],
        hint: 'pip install ./dist-packages/tcc_engine-0.1.0rc1-cp39-cp39-macosx_11_0_arm64.whl',
      });
    }

    throwCompileFailure(
      'Compilation failed',
      payload.file || filename,
      payload.message || 'Python compilation failed.',
      payload.span,
    );
  }

  const artifact = JSON.parse(compiled.stdout) as { envelope: { artifact_hash: string; frontend_version: string } };
  const extracted = await runPython(PYTHON_EFFECT_SCRIPT, filename, source);
  if (extracted.status !== 0) {
    throwCompileFailure(
      'Failed to load effect handlers',
      filename,
      extracted.stderr.trim() || extracted.stdout.trim() || 'Could not evaluate effect callbacks.',
    );
  }

  const values = JSON.parse(extracted.stdout || '{}') as Record<string, unknown>;
  const effects: Record<string, () => unknown> = {};
  for (const [key, value] of Object.entries(values)) {
    effects[key] = () => value;
  }

  return {
    language: 'python',
    artifactJson: compiled.stdout,
    artifactHash: artifact.envelope.artifact_hash,
    compilerVersion: artifact.envelope.frontend_version,
    effects,
  };
}

export async function extractPythonEffectSources(
  source: string,
  filename: string,
): Promise<Record<string, string>> {
  const extracted = await runPython(PYTHON_EFFECT_SOURCE_SCRIPT, filename, source);
  if (extracted.status !== 0) {
    throwCompileFailure(
      'Failed to load effect handlers',
      filename,
      extracted.stderr.trim() || extracted.stdout.trim() || 'Could not read effect callbacks.',
    );
  }
  return JSON.parse(extracted.stdout || '{}') as Record<string, string>;
}

export function languageFromFile(filePath: string): ProgramLanguage | undefined {
  const extension = path.extname(filePath).toLowerCase();
  if (extension === '.ts' || extension === '.mts' || extension === '.js' || extension === '.mjs') {
    return 'typescript';
  }
  if (extension === '.py') {
    return 'python';
  }
  return undefined;
}
