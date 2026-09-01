import type { ArtifactIdentity, ProgramIdentity } from './program';

export type CompilerDiagnosticSeverity = 'error' | 'warning';

export type SourceLocation = {
  line: number;
  column: number;
};

export type CompilerDiagnostic = {
  severity: CompilerDiagnosticSeverity;
  code: string;
  message: string;
  file?: string;
  start?: SourceLocation;
  end?: SourceLocation;
  hint?: string;
};

export type CompileSourceFile = {
  path: string;
  contents: string;
};

export type CompileRequest = {
  sourceRoot: string;
  programs: ProgramIdentity[];
  files: CompileSourceFile[];
};

export type CompiledModule = {
  path: string;
  contents: string;
};

export type CompileResult = {
  ok: boolean;
  artifact: ArtifactIdentity;
  diagnostics: CompilerDiagnostic[];
  /**
   * Optional compiled modules. When omitted, the local runtime loads the
   * original source (passthrough compiler). A real TCC compiler should
   * return lowered modules here.
   */
  modules?: CompiledModule[];
};
