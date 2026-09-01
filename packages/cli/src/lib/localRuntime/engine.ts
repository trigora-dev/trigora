import { randomUUID } from 'node:crypto';

import type {
  ExecutionRecord,
  JsonValue,
  RuntimeErrorCode,
  SerializedError,
  WaitForEventOptions,
} from '@trigora/contracts';
import { runWithDurableRuntime, type DurableRuntimeHost } from '@trigora/sdk';

import type { DiscoveredProgram } from './discoverPrograms';
import { parseDuration } from './parseDuration';

export class LocalRuntimeError extends Error {
  readonly code: RuntimeErrorCode;

  constructor(code: RuntimeErrorCode, message: string) {
    super(message);
    this.name = 'LocalRuntimeError';
    this.code = code;
  }
}

export class ExecutionCancelledError extends Error {
  constructor(executionId: string) {
    super(`Execution "${executionId}" was cancelled.`);
    this.name = 'ExecutionCancelledError';
  }
}

export type EngineListener = (event: {
  type: 'started' | 'waiting' | 'resumed' | 'completed' | 'failed' | 'cancelled' | 'effect';
  execution: ExecutionRecord;
  name?: string;
}) => void;

type Deferred = {
  promise: Promise<void>;
  resolve: () => void;
};

function createDeferred(): Deferred {
  let settled = false;
  let resolve = () => undefined;
  const promise = new Promise<void>((next) => {
    resolve = () => {
      if (settled) {
        return;
      }
      settled = true;
      next();
    };
  });
  return { promise, resolve };
}

type EventWaiter = {
  name: string;
  match?: Record<string, JsonValue>;
  resolve: (payload: JsonValue) => void;
  reject: (error: unknown) => void;
};

type LiveExecution = {
  record: ExecutionRecord;
  controller: AbortController;
  waiter?: EventWaiter;
  parked: Deferred;
  runPromise: Promise<void>;
};

function now(): string {
  return new Date().toISOString();
}

function toJsonValue(value: unknown): JsonValue {
  if (value === undefined) {
    return null;
  }

  return JSON.parse(JSON.stringify(value)) as JsonValue;
}

function serializeError(error: unknown): SerializedError {
  if (error instanceof Error) {
    return {
      name: error.name,
      message: error.message,
      stack: error.stack,
    };
  }

  return {
    name: 'Error',
    message: String(error),
  };
}

function snapshot(record: ExecutionRecord): ExecutionRecord {
  return {
    ...record,
    wait: record.wait ? { ...record.wait } : undefined,
    error: record.error ? { ...record.error } : undefined,
  };
}

function payloadMatches(payload: JsonValue, match?: Record<string, JsonValue>): boolean {
  if (!match) {
    return true;
  }

  if (typeof payload !== 'object' || payload === null || Array.isArray(payload)) {
    return false;
  }

  return Object.entries(match).every(([key, expected]) => payload[key] === expected);
}

function abortableDelay(ms: number, signal: AbortSignal, executionId: string): Promise<void> {
  if (signal.aborted) {
    return Promise.reject(new ExecutionCancelledError(executionId));
  }

  return new Promise((resolve, reject) => {
    const timer = setTimeout(finish, ms);

    function finish() {
      signal.removeEventListener('abort', onAbort);
      resolve();
    }

    function onAbort() {
      clearTimeout(timer);
      signal.removeEventListener('abort', onAbort);
      reject(new DOMException('This operation was aborted.', 'AbortError'));
    }

    signal.addEventListener('abort', onAbort, { once: true });
  });
}

export class LocalExecutionEngine {
  private readonly programs = new Map<string, DiscoveredProgram>();
  private readonly executions = new Map<string, LiveExecution>();

  constructor(private readonly onEvent?: EngineListener) {}

  listPrograms(): DiscoveredProgram[] {
    return [...this.programs.values()];
  }

  getProgram(programId: string): DiscoveredProgram | undefined {
    return this.programs.get(programId);
  }

  replacePrograms(programs: DiscoveredProgram[]): void {
    this.programs.clear();
    for (const program of programs) {
      this.programs.set(program.id, program);
    }
  }

  getExecution(executionId: string): ExecutionRecord {
    return snapshot(this.requireLive(executionId).record);
  }

  async start(
    programId: string,
    input: unknown,
    parentExecutionId?: string,
  ): Promise<ExecutionRecord> {
    const program = this.programs.get(programId);

    if (!program) {
      throw new LocalRuntimeError('program_not_found', `Program "${programId}" was not found.`);
    }

    const id = `exec_local_${randomUUID()}`;
    const createdAt = now();
    const live: LiveExecution = {
      record: {
        id,
        programId,
        status: 'running',
        input: toJsonValue(input),
        parentExecutionId,
        attempt: 1,
        createdAt,
        updatedAt: createdAt,
      },
      controller: new AbortController(),
      parked: createDeferred(),
      runPromise: Promise.resolve(),
    };

    this.executions.set(id, live);
    this.emit('started', live);

    live.runPromise = runWithDurableRuntime(this.createHost(live), async () =>
      program.fn(input as never),
    )
      .then((result) => {
        this.complete(live, result);
      })
      .catch((error: unknown) => {
        this.fail(live, error);
      });

    await live.parked.promise;
    return snapshot(live.record);
  }

  async send(executionId: string, name: string, payload: unknown): Promise<ExecutionRecord> {
    const live = this.requireLive(executionId);

    if (live.record.status !== 'waiting' || live.record.wait?.type !== 'event' || !live.waiter) {
      throw new LocalRuntimeError(
        'execution_not_waiting',
        `Execution "${executionId}" is not waiting for an event.`,
      );
    }

    const jsonPayload = toJsonValue(payload);

    if (live.waiter.name !== name || !payloadMatches(jsonPayload, live.waiter.match)) {
      throw new LocalRuntimeError(
        'event_mismatch',
        `Execution "${executionId}" is waiting for event "${live.waiter.name}".`,
      );
    }

    live.parked = createDeferred();
    live.waiter.resolve(jsonPayload);
    await live.parked.promise;
    return snapshot(live.record);
  }

  async cancel(executionId: string): Promise<ExecutionRecord> {
    const live = this.requireLive(executionId);

    if (live.record.status === 'completed' || live.record.status === 'failed') {
      throw new LocalRuntimeError(
        'execution_not_cancellable',
        `Execution "${executionId}" is already ${live.record.status}.`,
      );
    }

    if (live.record.status !== 'cancelled') {
      live.controller.abort();
      live.waiter?.reject(new ExecutionCancelledError(executionId));
      this.mark(live, 'cancelled');
      this.emit('cancelled', live);
      live.parked.resolve();
    }

    return snapshot(live.record);
  }

  private requireLive(executionId: string): LiveExecution {
    const live = this.executions.get(executionId);

    if (!live) {
      throw new LocalRuntimeError(
        'execution_not_found',
        `Execution "${executionId}" was not found.`,
      );
    }

    return live;
  }

  private createHost(live: LiveExecution): DurableRuntimeHost {
    return {
      effect: async (name, run) => {
        this.emit('effect', live, name);
        this.throwIfAborted(live);
        return run();
      },
      sleep: async (duration) => {
        const ms = parseDuration(duration, 'sleep duration');
        this.enterWait(live, {
          type: 'timer',
          resumeAt: new Date(Date.now() + ms).toISOString(),
        });
        try {
          await abortableDelay(ms, live.controller.signal, live.record.id);
          this.throwIfAborted(live);
        } finally {
          this.leaveWait(live);
        }
      },
      waitForEvent: async (name, options?: WaitForEventOptions) => {
        const timeoutMs =
          options?.timeout === undefined
            ? undefined
            : parseDuration(options.timeout, 'event timeout');
        this.enterWait(live, {
          type: 'event',
          eventName: name,
          timeoutAt:
            timeoutMs === undefined ? undefined : new Date(Date.now() + timeoutMs).toISOString(),
        });

        try {
          const payload = await new Promise<JsonValue>((resolve, reject) => {
            live.waiter = {
              name,
              match: options?.match,
              resolve,
              reject,
            };
            live.parked.resolve();

            if (timeoutMs !== undefined) {
              const timer = setTimeout(() => {
                reject(new Error(`Timed out waiting for event "${name}".`));
              }, timeoutMs);
              const originalResolve = resolve;
              const originalReject = reject;
              live.waiter.resolve = (value: JsonValue) => {
                clearTimeout(timer);
                originalResolve(value);
              };
              live.waiter.reject = (error: unknown) => {
                clearTimeout(timer);
                originalReject(error);
              };
            }
          });

          this.throwIfAborted(live);
          return payload as never;
        } finally {
          this.leaveWait(live);
        }
      },
      invoke: async (programId, input) => {
        this.throwIfAborted(live);
        const child = await this.start(programId, input, live.record.id);
        this.enterWait(live, { type: 'child', childExecutionId: child.id });
        const childLive = this.requireLive(child.id);
        await childLive.runPromise;
        this.leaveWait(live);

        if (childLive.record.status === 'failed') {
          throw new Error(
            childLive.record.error?.message ?? `Child program "${programId}" failed.`,
          );
        }

        if (childLive.record.status === 'cancelled') {
          throw new ExecutionCancelledError(childLive.record.id);
        }

        return childLive.record.result as never;
      },
      getExecution: () => ({
        id: live.record.id,
        attempt: live.record.attempt,
        programId: live.record.programId,
        signal: live.controller.signal,
      }),
    };
  }

  private enterWait(live: LiveExecution, wait: NonNullable<ExecutionRecord['wait']>): void {
    this.throwIfAborted(live);
    live.record.status = 'waiting';
    live.record.wait = wait;
    live.record.updatedAt = now();
    this.emit('waiting', live);
    if (wait.type === 'timer') {
      live.parked.resolve();
    }
  }

  private leaveWait(live: LiveExecution): void {
    live.waiter = undefined;
    if (live.record.status === 'waiting') {
      live.record.status = 'running';
      live.record.wait = undefined;
      live.record.updatedAt = now();
      this.emit('resumed', live);
    }
  }

  private complete(live: LiveExecution, result: unknown): void {
    if (live.record.status === 'cancelled') {
      return;
    }

    live.record.status = 'completed';
    live.record.result = toJsonValue(result);
    live.record.wait = undefined;
    live.record.updatedAt = now();
    this.emit('completed', live);
    live.parked.resolve();
  }

  private fail(live: LiveExecution, error: unknown): void {
    if (live.record.status === 'cancelled') {
      return;
    }

    if (
      error instanceof ExecutionCancelledError ||
      (error instanceof Error && error.name === 'AbortError')
    ) {
      live.record.status = 'cancelled';
      live.record.wait = undefined;
      live.record.updatedAt = now();
      this.emit('cancelled', live);
      live.parked.resolve();
      return;
    }

    live.record.status = 'failed';
    live.record.error = serializeError(error);
    live.record.wait = undefined;
    live.record.updatedAt = now();
    this.emit('failed', live);
    live.parked.resolve();
  }

  private mark(live: LiveExecution, status: ExecutionRecord['status']): void {
    live.record.status = status;
    live.record.wait = undefined;
    live.record.updatedAt = now();
  }

  private throwIfAborted(live: LiveExecution): void {
    if (live.controller.signal.aborted || live.record.status === 'cancelled') {
      throw new ExecutionCancelledError(live.record.id);
    }
  }

  private emit(
    type: Parameters<EngineListener>[0]['type'],
    live: LiveExecution,
    name?: string,
  ): void {
    this.onEvent?.({ type, execution: snapshot(live.record), name });
  }
}
