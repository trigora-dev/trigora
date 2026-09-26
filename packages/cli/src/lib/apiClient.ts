import {
  DEFAULT_RUNTIME_HOST,
  DEFAULT_RUNTIME_PORT,
  resolveEventName,
  resolveProgramId,
  type ApiErrorResponse,
  type CancelExecutionResponse,
  type CreateProjectRequest,
  type CreateProjectResponse,
  type DeployProgramRequest,
  type DeployProgramResponse,
  type ReplaceTriggersRequest,
  type ReplaceTriggersResponse,
  type DurableProgram,
  type EventDefinition,
  type Execution,
  type ExecutionResult,
  type GetExecutionResponse,
  type GetExecutionResultResponse,
  type GetProgramResponse,
  type JsonValue,
  type ListExecutionsResponse,
  type ListProgramsResponse,
  type ListProgramVersionsResponse,
  type ListProjectsResponse,
  type Pagination,
  type SendEventResponse,
  type StartExecutionRequest,
  type StartExecutionResponse,
  type WhoAmIResponse,
} from '@trigora/contracts';

export const DEFAULT_RUNTIME_URL = `http://${DEFAULT_RUNTIME_HOST}:${DEFAULT_RUNTIME_PORT}`;
export const DEFAULT_CLOUD_API_URL = 'https://api.trigora.dev';

export type CreateApiClientOptions = {
  url?: string;
  /** `null` ignores `TRIGORA_TOKEN` and sends no Authorization header. */
  token?: string | null;
  projectId?: string;
  fetch?: typeof fetch;
};

export class ApiClientError extends Error {
  readonly code?: string;
  readonly status: number;

  constructor(message: string, options: { status: number; code?: string }) {
    super(message);
    this.name = 'ApiClientError';
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

export type ApiClient = {
  whoAmI(): Promise<WhoAmIResponse>;
  listProjects(): Promise<ListProjectsResponse>;
  createProject(body: CreateProjectRequest): Promise<CreateProjectResponse>;
  deployProgram(body: DeployProgramRequest): Promise<DeployProgramResponse>;
  replaceTriggers(
    projectId: string,
    body: ReplaceTriggersRequest,
  ): Promise<ReplaceTriggersResponse>;
  listPrograms(pagination?: Pagination): Promise<ListProgramsResponse>;
  getProgram(programId: string): Promise<GetProgramResponse>;
  listProgramVersions(
    programId: string,
    pagination?: Pagination,
  ): Promise<ListProgramVersionsResponse>;
  start<TInput, TResult>(
    program: DurableProgram<TInput, TResult> | string,
    input: TInput,
  ): Promise<ExecutionHandle<TResult>>;
  startExecution(body: StartExecutionRequest): Promise<StartExecutionResponse>;
  get<TResult = unknown>(executionId: string): ExecutionHandle<TResult>;
  getExecution(executionId: string): Promise<Execution>;
  listExecutions(pagination?: Pagination): Promise<ListExecutionsResponse>;
  sendEvent(executionId: string, name: string, payload?: JsonValue): Promise<SendEventResponse>;
  cancelExecution(executionId: string): Promise<CancelExecutionResponse>;
  getResult(executionId: string): Promise<ExecutionResult>;
  programs(): Promise<ListProgramsResponse>;
  executions(): Promise<ListExecutionsResponse>;
};

const RESULT_POLL_INTERVAL_MS = 50;
const PROJECT_HEADER = 'X-Trigora-Project-Id';

function resolveToken(options: CreateApiClientOptions): string | undefined {
  if (options.token === null) {
    return undefined;
  }
  const token = (options.token ?? process.env.TRIGORA_TOKEN)?.trim();
  return token || undefined;
}

function resolveBaseUrl(options: CreateApiClientOptions): string {
  if (options.url) {
    return options.url.replace(/\/$/, '');
  }

  if (resolveToken(options)) {
    return (process.env.TRIGORA_API_BASE_URL ?? DEFAULT_CLOUD_API_URL).replace(/\/$/, '');
  }

  return (process.env.TRIGORA_RUNTIME_URL ?? DEFAULT_RUNTIME_URL).replace(/\/$/, '');
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function reviveError(error: Execution['error']): Error {
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

function isApiErrorResponse(value: unknown): value is ApiErrorResponse {
  return (
    typeof value === 'object' &&
    value !== null &&
    'error' in value &&
    typeof (value as ApiErrorResponse).error?.message === 'string'
  );
}

function query(pagination?: Pagination): string {
  if (!pagination) {
    return '';
  }
  const params = new URLSearchParams();
  if (pagination.limit !== undefined) {
    params.set('limit', String(pagination.limit));
  }
  if (pagination.cursor) {
    params.set('cursor', pagination.cursor);
  }
  const encoded = params.toString();
  return encoded ? `?${encoded}` : '';
}

export function createApiClient(options: CreateApiClientOptions = {}): ApiClient {
  const url = resolveBaseUrl(options);
  const token = resolveToken(options);
  const projectId = options.projectId;
  const fetchImpl = options.fetch ?? fetch;

  async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
    const headers: Record<string, string> = {
      Accept: 'application/json',
      ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
      ...(init?.headers as Record<string, string> | undefined),
    };
    if (token) {
      headers.Authorization = `Bearer ${token}`;
    }
    if (projectId) {
      headers[PROJECT_HEADER] = projectId;
    }

    const requestUrl = `${url}${path}`;
    let response: Response;

    try {
      response = await fetchImpl(requestUrl, {
        ...init,
        headers,
      });
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      throw new ApiClientError(
        token
          ? `Could not reach Trigora Cloud at ${requestUrl}. ${reason}`
          : `Could not reach the local Trigora runtime at ${requestUrl}. Is \`trigora dev\` running? ${reason}`,
        { status: 0 },
      );
    }

    const body = await readJson(response);
    if (!response.ok) {
      const apiError = isApiErrorResponse(body) ? body.error : undefined;
      throw new ApiClientError(apiError?.message ?? `Request failed (${response.status})`, {
        status: response.status,
        code: apiError?.code,
      });
    }

    return body as T;
  }

  class ExecutionHandleImpl<TResult> implements ExecutionHandle<TResult> {
    constructor(readonly id: string) {}

    async result(): Promise<TResult> {
      for (;;) {
        const { result } = await apiFetch<GetExecutionResultResponse>(
          `/v1/executions/${encodeURIComponent(this.id)}/result`,
        );

        if (result.status === 'completed') {
          return result.result as TResult;
        }

        if (result.status === 'failed') {
          throw reviveError(result.error);
        }

        if (result.status === 'cancelled') {
          throw new ApiClientError(`Execution "${this.id}" was cancelled.`, {
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
      await apiFetch<SendEventResponse>(`/v1/executions/${encodeURIComponent(this.id)}/events`, {
        method: 'POST',
        body: JSON.stringify({
          name: resolveEventName(event),
          payload: payload as JsonValue,
        }),
      });
    }

    async cancel(): Promise<void> {
      await apiFetch<CancelExecutionResponse>(
        `/v1/executions/${encodeURIComponent(this.id)}/cancel`,
        { method: 'POST' },
      );
    }
  }

  return {
    whoAmI() {
      return apiFetch<WhoAmIResponse>('/v1/whoami');
    },
    listProjects() {
      return apiFetch<ListProjectsResponse>('/v1/projects');
    },
    createProject(body) {
      return apiFetch<CreateProjectResponse>('/v1/projects', {
        method: 'POST',
        body: JSON.stringify(body),
      });
    },
    deployProgram(body) {
      return apiFetch<DeployProgramResponse>('/v1/programs/deploy', {
        method: 'POST',
        body: JSON.stringify(body),
      });
    },
    replaceTriggers(projectId, body) {
      return apiFetch<ReplaceTriggersResponse>(
        `/v1/projects/${encodeURIComponent(projectId)}/triggers`,
        {
          method: 'PUT',
          body: JSON.stringify(body),
        },
      );
    },
    listPrograms(pagination) {
      return apiFetch<ListProgramsResponse>(`/v1/programs${query(pagination)}`);
    },
    getProgram(programId) {
      return apiFetch<GetProgramResponse>(`/v1/programs/${encodeURIComponent(programId)}`);
    },
    listProgramVersions(programId, pagination) {
      return apiFetch<ListProgramVersionsResponse>(
        `/v1/programs/${encodeURIComponent(programId)}/versions${query(pagination)}`,
      );
    },
    async start(program, input) {
      const response = await apiFetch<StartExecutionResponse>('/v1/executions', {
        method: 'POST',
        body: JSON.stringify({
          programId: resolveProgramId(program),
          input,
        }),
      });
      return new ExecutionHandleImpl(response.execution.id);
    },
    startExecution(body) {
      return apiFetch<StartExecutionResponse>('/v1/executions', {
        method: 'POST',
        body: JSON.stringify(body),
      });
    },
    get(executionId) {
      return new ExecutionHandleImpl(executionId);
    },
    async getExecution(executionId) {
      const { execution } = await apiFetch<GetExecutionResponse>(
        `/v1/executions/${encodeURIComponent(executionId)}`,
      );
      return execution;
    },
    listExecutions(pagination) {
      return apiFetch<ListExecutionsResponse>(`/v1/executions${query(pagination)}`);
    },
    sendEvent(executionId, name, payload) {
      return apiFetch<SendEventResponse>(
        `/v1/executions/${encodeURIComponent(executionId)}/events`,
        {
          method: 'POST',
          body: JSON.stringify({ name, payload }),
        },
      );
    },
    cancelExecution(executionId) {
      return apiFetch<CancelExecutionResponse>(
        `/v1/executions/${encodeURIComponent(executionId)}/cancel`,
        { method: 'POST' },
      );
    },
    async getResult(executionId) {
      const { result } = await apiFetch<GetExecutionResultResponse>(
        `/v1/executions/${encodeURIComponent(executionId)}/result`,
      );
      return result;
    },
    programs() {
      return apiFetch<ListProgramsResponse>('/v1/programs');
    },
    executions() {
      return apiFetch<ListExecutionsResponse>('/v1/executions');
    },
  };
}
