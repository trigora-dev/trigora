import type { JsonValue } from './json';
import type { ProgramId } from './program';

export type ExecutionId = string;

export type ExecutionStatus = 'running' | 'waiting' | 'completed' | 'failed' | 'cancelled';

export type SerializedError = {
  name: string;
  message: string;
  stack?: string;
};

export type ExecutionWait =
  | {
      type: 'event';
      event: string;
    }
  | {
      type: 'timer';
      wakeAt: string;
    }
  | {
      type: 'child';
      executionId: ExecutionId;
    };

export type EventDefinition<TPayload = unknown> = {
  readonly name: string;
  readonly __payload?: TPayload;
};

export type WaitForEventOptions = {
  timeout?: string | number;
  match?: Record<string, JsonValue>;
};

export type ExecutionSummary = {
  id: ExecutionId;
  projectId: string;
  programId: ProgramId;
  programName: string;
  status: ExecutionStatus;
  wait?: ExecutionWait;
  createdAt: string;
  updatedAt: string;
};

export type Execution = ExecutionSummary & {
  programVersionId: string;
  artifactHash: string;
  engineFormatVersion: number;
  input: JsonValue;
  result?: JsonValue;
  error?: SerializedError;
  parentExecutionId?: ExecutionId;
  attempt: number;
};

export type ExecutionDetail = Execution;

export type ExecutionResult = {
  status: ExecutionStatus;
  result?: JsonValue;
  error?: SerializedError;
};

export type StartExecutionRequest = {
  programId: ProgramId;
  input?: JsonValue;
};

export type StartExecutionResponse = {
  execution: Execution;
};

export type GetExecutionResponse = {
  execution: Execution;
};

export type ListExecutionsResponse = {
  executions: ExecutionSummary[];
  nextCursor?: string;
};

export type SendEventRequest = {
  name: string;
  payload?: JsonValue;
};

export type SendEventResponse = {
  execution: Execution;
};

export type CancelExecutionRequest = Record<string, never>;

export type CancelExecutionResponse = {
  execution: Execution;
};

export type GetExecutionResultResponse = {
  result: ExecutionResult;
};
