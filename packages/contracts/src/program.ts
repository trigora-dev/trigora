import type { JsonValue } from './json';

export const DEFAULT_RUNTIME_HOST = '127.0.0.1';
export const DEFAULT_RUNTIME_PORT = 3477;
export const ENGINE_FORMAT_VERSION = 1;

export type ProgramId = string;
export type ProgramVersionId = string;
export type ProgramLanguage = 'javascript' | 'python';

export type ProgramIdentity = {
  id: ProgramId;
  exportName: string;
  file: string;
};

export type ArtifactIdentity = {
  artifactHash: string;
  compilerVersion: string;
};

export type TrigoraRuntimeConfig = {
  host?: string;
  port?: number;
};

export type TrigoraCompilerConfig = {
  endpoint?: string;
};

export type TrigoraConfig = {
  programs: string | string[];
  runtime?: TrigoraRuntimeConfig;
  compiler?: TrigoraCompilerConfig;
};

export type ResolvedTrigoraConfig = {
  configPath: string;
  rootDir: string;
  programGlobs: string[];
  runtime: {
    host: string;
    port: number;
  };
  compiler: {
    endpoint?: string;
  };
};

export type DeployArtifact = {
  hash: string;
  blob: string;
  engineFormatVersion: number;
  languageSemanticsVersion: string;
  frontendId: string;
  frontendVersion: string;
};

export type ArtifactFile = {
  path: string;
  contents: string;
  entrypoint?: boolean;
};

export type EffectBundle = {
  language: ProgramLanguage;
  files: ArtifactFile[];
};

export type DeployProgramRequest = {
  name: string;
  artifact: DeployArtifact;
  effectBundle: EffectBundle;
};

export type ProgramVersion = {
  id: ProgramVersionId;
  programId: ProgramId;
  artifactHash: string;
  engineFormatVersion: number;
  languageSemanticsVersion: string;
  frontendId: string;
  frontendVersion: string;
  language: ProgramLanguage;
  createdAt: string;
};

export type ProgramVersionSummary = {
  id: ProgramVersionId;
  artifactHash: string;
  language: ProgramLanguage;
  createdAt: string;
};

export type Program = {
  id: ProgramId;
  projectId: string;
  name: string;
  currentVersionId: string | null;
  currentVersion: ProgramVersion | null;
  createdAt: string;
  updatedAt: string;
};

export type ProgramSummary = {
  id: ProgramId;
  name: string;
  language: ProgramLanguage | null;
  currentVersionId: string | null;
  updatedAt: string;
};

export type DeployProgramResponse = {
  program: Program;
  version: ProgramVersion;
};

export type ListProgramsResponse = {
  programs: ProgramSummary[];
  nextCursor?: string;
};

export type GetProgramResponse = {
  program: Program;
};

export type ListProgramVersionsResponse = {
  versions: ProgramVersionSummary[];
  nextCursor?: string;
};

export type HealthResponse = {
  ok: true;
};

export type { JsonValue };
