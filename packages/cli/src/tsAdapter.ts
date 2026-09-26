import { compile, PACKAGE_VERSION } from '@tcc-engine/frontend-typescript';

import {
  extractTypeScriptEffects,
  extractTypeScriptEffectSources,
} from './lib/localRuntime/extractEffects';

export const compilerVersion = PACKAGE_VERSION;

export type CompiledEffect = {
  id: string;
  key: string;
  source: string;
};

export type CompiledProgram = {
  artifactJson: string;
  artifactHash: string;
  compilerVersion: string;
  language: 'typescript';
  frontendId: string;
  frontendVersion: string;
  languageSemanticsVersion: string;
  engineFormatVersion: number;
  effects: CompiledEffect[];
  handlers: Record<string, (input?: unknown) => unknown>;
};

export function compileProgram(
  source: string,
  filename: string,
  programId: string,
): CompiledProgram {
  const artifact = compile(source, { filename });
  const sources = extractTypeScriptEffectSources(source, filename);
  const handlers = extractTypeScriptEffects(source, filename);
  return {
    artifactJson: JSON.stringify(artifact),
    artifactHash: artifact.envelope.artifact_hash,
    compilerVersion: PACKAGE_VERSION,
    language: 'typescript',
    frontendId: artifact.envelope.frontend_id ?? 'typescript',
    frontendVersion: artifact.envelope.frontend_version ?? PACKAGE_VERSION,
    languageSemanticsVersion: artifact.envelope.language_semantics_version ?? 'ts.subset.v1',
    engineFormatVersion: artifact.envelope.engine_format_version ?? 1,
    effects: Object.keys(sources).map((key) => ({
      id: `${programId}:${key}`,
      key,
      source: sources[key] ?? '',
    })),
    handlers,
  };
}

export function runHandler(handler: (input?: unknown) => unknown, input: unknown): unknown {
  const value = handler(input ?? {});
  return value === undefined ? null : JSON.parse(JSON.stringify(value));
}
