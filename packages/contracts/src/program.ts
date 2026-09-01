/**
 * Durable program identity and project configuration.
 *
 * A program is an ordinary exported async function discovered from
 * `trigora.config.ts` globs. There is no wrapper and no flow id.
 */

export const DEFAULT_RUNTIME_HOST = '127.0.0.1';
export const DEFAULT_RUNTIME_PORT = 3477;

export type ProgramId = string;

export type ProgramIdentity = {
  /**
   * Unique program id. Local preview uses the export name and requires
   * export names to be unique across discovered files.
   */
  id: ProgramId;
  exportName: string;
  /** Project-relative POSIX path to the source module. */
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
  /**
   * Optional HTTP endpoint for the TCC compiler in the engine repo.
   * When omitted, the CLI uses the local passthrough compiler.
   */
  endpoint?: string;
};

export type TrigoraConfig = {
  /** One or more globs of modules that export durable programs. */
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
