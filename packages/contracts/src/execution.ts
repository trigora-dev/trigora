import type { JsonValue } from './flow';
import type { ProgramId } from './program';

export type ExecutionId = string;

export type ExecutionStatus = 'running' | 'waiting' | 'completed' | 'failed' | 'cancelled';

export type SerializedError = {
  name: string;
  message: string;
  stack?: string;
};

export type TimerWaitCondition = {
  type: 'timer';
  resumeAt: string;
};

export type EventWaitCondition = {
  type: 'event';
  eventName: string;
  timeoutAt?: string;
};

export type ChildWaitCondition = {
  type: 'child';
  childExecutionId: ExecutionId;
};

export type WaitCondition = TimerWaitCondition | EventWaitCondition | ChildWaitCondition;

export type ExecutionRecord = {
  id: ExecutionId;
  programId: ProgramId;
  status: ExecutionStatus;
  input: JsonValue;
  result?: JsonValue;
  error?: SerializedError;
  wait?: WaitCondition;
  parentExecutionId?: ExecutionId;
  attempt: number;
  createdAt: string;
  updatedAt: string;
};

/**
 * Typed event channel used by `waitForEvent()` and `run.send()`.
 * `__payload` is a phantom field for inference only.
 */
export type EventDefinition<TPayload = unknown> = {
  readonly name: string;
  readonly __payload?: TPayload;
};

export type WaitForEventOptions = {
  timeout?: string | number;
  match?: Record<string, JsonValue>;
};
