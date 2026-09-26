import type { JsonValue } from './json';

export const DEFAULT_RUNTIME_HOST = '127.0.0.1';
export const DEFAULT_RUNTIME_PORT = 3477;
export const ENGINE_FORMAT_VERSION = 1;

export type ProgramId = string;
export type ProgramVersionId = string;
export type ProgramLanguage = 'typescript' | 'python' | 'rust';

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

export type WebhookTriggerConfig = {
  name: string;
  type: 'webhook';
  program: string;
};

export type CronTriggerConfig = {
  name: string;
  type: 'cron';
  program: string;
  schedule: string;
  timezone?: string;
  input?: JsonValue;
};

export type TriggerConfig = WebhookTriggerConfig | CronTriggerConfig;

export type TriggerSummary = {
  id: string;
  publicId: string;
  name: string;
  type: 'webhook' | 'cron';
  programId: string;
  schedule: string | null;
  timezone: string | null;
  status: string;
  url: string | null;
};

export type ReplaceTriggersRequest = {
  triggers: TriggerConfig[];
};

export type ReplaceTriggersResponse = {
  triggers: TriggerSummary[];
};

export type ListProgramTriggersResponse = {
  triggers: TriggerSummary[];
};

export type ResolvedTrigoraConfig = {
  configPath: string;
  rootDir: string;
  programGlobs: string[];
  projectName: string;
  triggers: TriggerConfig[];
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
  encoding?: 'base64';
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
  frontendId: string;
  frontendVersion: string;
  languageSemanticsVersion: string;
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
