import type {
  CancelExecutionResponse,
  EventDefinition,
  ExecutionRecord,
  GetExecutionResponse,
  JsonValue,
  ListProgramsResponse,
  RuntimeErrorResponse,
  SendExecutionEventResponse,
  StartExecutionResponse,
} from '@trigora/contracts';
import { DEFAULT_RUNTIME_HOST, DEFAULT_RUNTIME_PORT } from '@trigora/contracts';
import { resolveEventName, resolveProgramId, type DurableProgram } from '@trigora/sdk';

export const DEFAULT_RUNTIME_URL = `http://${DEFAULT_RUNTIME_HOST}:${DEFAULT_RUNTIME_PORT}`;

export type CreateClientOptions = {
  url?: string;
};

export class TrigoraRuntimeError extends Error {
  readonly code?: string;
  readonly status: number;

  constructor(message: string, options: { status: number; code?: string }) {
    super(message);
    this.name = 'TrigoraRuntimeError';
    this.status = options.status;
    this.code = options.code;
  }
}

export type ExecutionHandle<TResult = unknown> = {
  readonly id: string;
  result(): Promise<TResult>;
  send<TPayload>(event: EventDefinition<TPayload> | string, payload: TPayload): Promise<void>;
  cancel(): Promise<void>;
};

export type TrigoraClient = {
  start<TInput, TResult>(
    program: DurableProgram<TInput, TResult> | string,
    input: TInput,
  ): Promise<ExecutionHandle<TResult>>;
  get<TResult = unknown>(executionId: string): ExecutionHandle<TResult>;
  programs(): Promise<ListProgramsResponse>;
};

const RESULT_POLL_INTERVAL_MS = 50;

function resolveRuntimeUrl(url?: string): string {
  return (url ?? process.env.TRIGORA_RUNTIME_URL ?? DEFAULT_RUNTIME_URL).replace(/\/$/, '');
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function reviveError(error: ExecutionRecord['error']): Error {
  const revived = new Error(error?.message ?? 'Execution failed');
  revived.name = error?.name ?? 'Error';
  if (error?.stack) {
    revived.stack = error.stack;
  }
  return revived;
}

async function readJson(response: Response): Promise<unknown> {
  const text = await response.text();

  if (!text) {
    return undefined;
  }

  try {
    return JSON.parse(text) as unknown;
  } catch {
    return text;
  }
}

function isRuntimeErrorResponse(value: unknown): value is RuntimeErrorResponse {
  return (
    typeof value === 'object' &&
    value !== null &&
    'error' in value &&
    typeof (value as RuntimeErrorResponse).error?.message === 'string'
  );
}

async function runtimeFetch<T>(url: string, init?: RequestInit): Promise<T> {
  let response: Response;

  try {
    response = await fetch(url, {
      ...init,
      headers: {
        Accept: 'application/json',
        ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
        ...init?.headers,
      },
    });
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new TrigoraRuntimeError(
      `Could not reach the local Trigora runtime at ${url}. Is \`trigora dev\` running? ${reason}`,
      { status: 0 },
    );
  }

  const body = await readJson(response);

  if (!response.ok) {
    const runtimeError = isRuntimeErrorResponse(body) ? body.error : undefined;
    throw new TrigoraRuntimeError(
      runtimeError?.message ?? `Runtime request failed (${response.status})`,
      {
        status: response.status,
        code: runtimeError?.code,
      },
    );
  }

  return body as T;
}

class ExecutionHandleImpl<TResult> implements ExecutionHandle<TResult> {
  constructor(
    private readonly url: string,
    readonly id: string,
  ) {}

  async result(): Promise<TResult> {
    for (;;) {
      const { execution } = await runtimeFetch<GetExecutionResponse>(
        `${this.url}/v1/executions/${encodeURIComponent(this.id)}`,
      );

      if (execution.status === 'completed') {
        return execution.result as TResult;
      }

      if (execution.status === 'failed') {
        throw reviveError(execution.error);
      }

      if (execution.status === 'cancelled') {
        throw new TrigoraRuntimeError(`Execution "${this.id}" was cancelled.`, {
          status: 409,
          code: 'execution_not_cancellable',
        });
      }

      await delay(RESULT_POLL_INTERVAL_MS);
    }
  }

  async send<TPayload>(
    event: EventDefinition<TPayload> | string,
    payload: TPayload,
  ): Promise<void> {
    await runtimeFetch<SendExecutionEventResponse>(
      `${this.url}/v1/executions/${encodeURIComponent(this.id)}/events`,
      {
        method: 'POST',
        body: JSON.stringify({
          name: resolveEventName(event),
          payload: payload as JsonValue,
        }),
      },
    );
  }

  async cancel(): Promise<void> {
    await runtimeFetch<CancelExecutionResponse>(
      `${this.url}/v1/executions/${encodeURIComponent(this.id)}/cancel`,
      { method: 'POST' },
    );
  }
}

export function createClient(options: CreateClientOptions = {}): TrigoraClient {
  const url = resolveRuntimeUrl(options.url);

  return {
    async start(program, input) {
      const response = await runtimeFetch<StartExecutionResponse>(`${url}/v1/executions`, {
        method: 'POST',
        body: JSON.stringify({
          programId: resolveProgramId(program),
          input,
        }),
      });

      return new ExecutionHandleImpl(url, response.execution.id);
    },
    get(executionId) {
      return new ExecutionHandleImpl(url, executionId);
    },
    programs() {
      return runtimeFetch<ListProgramsResponse>(`${url}/v1/programs`);
    },
  };
}

export const trigora: TrigoraClient = createClient();
