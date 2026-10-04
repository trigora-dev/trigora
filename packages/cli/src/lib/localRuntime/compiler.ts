import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

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
import { extractRustEffects, runRustEffect, rustEffectBinary } from './rustEffects';

export type ProgramLanguage = 'typescript' | 'python' | 'rust';

export type EffectHandler = (input?: unknown) => unknown;

export type CompiledProgramArtifact = {
  language: ProgramLanguage;
  artifactJson: string;
  artifactHash: string;
  compilerVersion: string;
  effects: Record<string, EffectHandler>;
};

const PYTHON_VERSION_SCRIPT = `
import json, sys
try:
    from tcc_engine import PACKAGE_VERSION
except ModuleNotFoundError:
    json.dump({"status": "missing"}, sys.stdout)
    raise SystemExit(0)
json.dump({"status": "ok", "version": PACKAGE_VERSION}, sys.stdout)
`;

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

function runPython(
  script: string,
  filename: string,
  source: string,
  extraArgs: string[] = [],
): Promise<{
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

function throwCompileFailure(
  title: string,
  file: string,
  message: string,
  span?: {
    start_line?: number;
    start_column?: number;
  },
): never {
  const location =
    span?.start_line === undefined ? file : `${file}:${span.start_line}:${span.start_column ?? 1}`;
  throw new CliDisplayError({
    title,
    message,
    details: [
      { label: 'File', value: location },
      { label: 'Reason', value: message },
    ],
  });
}

export function compileTypeScriptProgram(
  source: string,
  filename: string,
): CompiledProgramArtifact {
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
      throwCompileFailure(
        'Compilation failed',
        error.file || filename,
        error.message,
        error.span ?? undefined,
      );
    }
    throw error;
  }
}

function pythonInstallCommand(): string {
  return 'python3 -m pip install trigora';
}

function pythonCompilerMissing(filename: string): CliDisplayError {
  return new CliDisplayError({
    title: 'Python compiler unavailable',
    message: 'Python compiler support is not installed.',
    details: [
      { label: 'File', value: filename },
      { label: 'Install', value: pythonInstallCommand() },
    ],
  });
}

function pythonCompilerMismatch(filename: string, found: string): CliDisplayError {
  return new CliDisplayError({
    title: 'Python compiler version mismatch',
    message: `This CLI requires Python compiler ${PACKAGE_VERSION}, installed with the trigora authoring package.`,
    details: [
      { label: 'File', value: filename },
      { label: 'Found', value: found },
      { label: 'Install', value: pythonInstallCommand() },
    ],
  });
}

async function requirePythonCompiler(filename: string): Promise<void> {
  let probed;
  try {
    probed = await runPython(PYTHON_VERSION_SCRIPT, filename, '');
  } catch {
    throw pythonCompilerMissing(filename);
  }

  let payload: { status?: string; version?: string } = {};
  try {
    payload = JSON.parse(probed.stdout) as typeof payload;
  } catch {
    throw pythonCompilerMissing(filename);
  }

  if (payload.status === 'missing') {
    throw pythonCompilerMissing(filename);
  }
  if (payload.version !== PACKAGE_VERSION) {
    throw pythonCompilerMismatch(filename, payload.version || 'unknown');
  }
}

export async function compilePythonProgram(
  source: string,
  filename: string,
): Promise<CompiledProgramArtifact> {
  await requirePythonCompiler(filename);

  let compiled;
  try {
    compiled = await runPython(PYTHON_COMPILE_SCRIPT, filename, source);
  } catch {
    throw pythonCompilerMissing(filename);
  }

  if (compiled.status !== 0) {
    let payload: {
      message?: string;
      file?: string;
      span?: { start_line?: number; start_column?: number };
    } = {};
    try {
      payload = JSON.parse(compiled.stderr || compiled.stdout) as typeof payload;
    } catch {
      payload = {
        message: compiled.stderr.trim() || compiled.stdout.trim() || 'Python compilation failed.',
      };
    }

    if (/No module named ['"]tcc_engine['"]/.test(payload.message ?? compiled.stderr)) {
      throw pythonCompilerMissing(filename);
    }

    throwCompileFailure(
      'Compilation failed',
      payload.file || filename,
      payload.message || 'Python compilation failed.',
      payload.span,
    );
  }

  const artifact = JSON.parse(compiled.stdout) as {
    envelope: { artifact_hash: string; frontend_version: string };
  };
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

function rustCompilerBin(): string | undefined {
  const override = process.env.TRIGORA_RUST_COMPILER_BIN?.trim();
  const here = path.dirname(fileURLToPath(import.meta.url));
  const extension = process.platform === 'win32' ? '.exe' : '';
  const candidates = [
    override,
    path.resolve(
      here,
      `../../../node_modules/@tcc-engine/frontend-rust/vendor/tcc-rust-compile${extension}`,
    ),
    path.resolve(here, `../../../../tcc-engine/target/release/tcc-rust-compile${extension}`),
  ].filter((candidate): candidate is string => Boolean(candidate));
  return candidates.find((candidate) => fs.existsSync(candidate));
}

export async function compileRustProgram(
  source: string,
  filename: string,
  options: { rootDir: string; programId: string },
): Promise<CompiledProgramArtifact> {
  const compiler = rustCompilerBin();
  if (!compiler) {
    throw new CliDisplayError({
      title: 'Rust compiler unavailable',
      details: [
        { label: 'File', value: filename },
        { label: 'Reason', value: 'The Rust compiler binary was not found.' },
      ],
      hint: 'Reinstall the CLI with `npm install -g trigora`. The Rust compiler is included.',
    });
  }
  const file = path.join(options.rootDir, `.trigora-compile-${options.programId}.rs`);
  fs.mkdirSync(options.rootDir, { recursive: true });
  fs.writeFileSync(file, source);
  const compiled = spawnSync(compiler, [file], { encoding: 'utf8' });
  fs.rmSync(file, { force: true });
  if (compiled.error || compiled.status !== 0) {
    throwCompileFailure(
      'Compilation failed',
      filename,
      (
        compiled.stderr ||
        compiled.stdout ||
        compiled.error?.message ||
        'Rust compilation failed.'
      ).trim(),
    );
  }
  let artifact: { envelope: { artifact_hash: string; frontend_version?: string } };
  try {
    artifact = JSON.parse(compiled.stdout) as typeof artifact;
  } catch {
    throwCompileFailure('Compilation failed', filename, 'The compiler did not return JSON.');
  }

  let extracted;
  try {
    extracted = extractRustEffects(source);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throwCompileFailure('Failed to load effect handlers', filename, message);
  }

  const effects: Record<string, EffectHandler> = {};
  if (extracted.length > 0) {
    const binary = rustEffectBinary({
      rootDir: options.rootDir,
      programId: options.programId,
      file: filename,
      source,
    });
    for (const effect of extracted) {
      effects[effect.key] = (input?: unknown) => runRustEffect(binary, effect.key, input ?? {});
    }
  }

  return {
    language: 'rust',
    artifactJson: JSON.stringify(artifact),
    artifactHash: artifact.envelope.artifact_hash,
    compilerVersion: artifact.envelope.frontend_version || '',
    effects,
  };
}

export function languageFromFile(filePath: string): ProgramLanguage | undefined {
  const extension = path.extname(filePath).toLowerCase();
  if (extension === '.ts' || extension === '.mts' || extension === '.js' || extension === '.mjs') {
    return 'typescript';
  }
  if (extension === '.py') {
    return 'python';
  }
  if (extension === '.rs') {
    return 'rust';
  }
  return undefined;
}
