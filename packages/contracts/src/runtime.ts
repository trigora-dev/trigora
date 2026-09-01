import type { CompilerDiagnostic } from './compiler';
import type { ExecutionRecord } from './execution';
import type { JsonValue } from './flow';
import type { ArtifactIdentity, ProgramId, ProgramIdentity } from './program';

export type RuntimeErrorCode =
  | 'compiler_failed'
  | 'event_mismatch'
  | 'execution_not_cancellable'
  | 'execution_not_found'
  | 'execution_not_waiting'
  | 'invalid_input'
  | 'program_not_found';

export type RuntimeErrorResponse = {
  error: {
    code: RuntimeErrorCode;
    message: string;
    diagnostics?: CompilerDiagnostic[];
  };
};

export type ListProgramsResponse = {
  artifact: ArtifactIdentity;
  programs: ProgramIdentity[];
  runtime: {
    url: string;
  };
};

export type StartExecutionRequest = {
  programId: ProgramId;
  input?: JsonValue;
};

export type StartExecutionResponse = {
  execution: ExecutionRecord;
};

export type GetExecutionResponse = {
  execution: ExecutionRecord;
};

export type SendExecutionEventRequest = {
  name: string;
  payload?: JsonValue;
};

export type SendExecutionEventResponse = {
  execution: ExecutionRecord;
};

export type CancelExecutionResponse = {
  execution: ExecutionRecord;
};

export type HealthResponse = {
  ok: true;
};
