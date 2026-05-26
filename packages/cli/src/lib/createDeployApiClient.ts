import type {
  ApiErrorCode,
  ApiErrorResponse,
  ApiErrorStep,
  CreateDeploymentRequest,
  CreateDeploymentResponse,
  DeleteFlowResponse,
  DeleteFlowSecretResponse,
  FlowInvocationLogLevel,
  FlowInvocationLogRecord,
  FlowInvocationRecord,
  FlowInvocationStatus,
  FlowSecretRecord,
  FlowStatusResponse,
  FlowRecord,
  FlowStatus,
  FlowTriggerType,
  GetFlowResponse,
  GetInvocationResponse,
  InvocationExecutionContext,
  ListFlowInvocationsQuery,
  ListInvocationsResponse,
  ListSecretsQuery,
  ListSecretsResponse,
  ListFlowsResponse,
  SetFlowSecretRequest,
  SetFlowSecretResponse,
  WhoAmIResponse,
} from '@trigora/contracts';

export type DeployApiClient = {
  createDeployment(request: CreateDeploymentRequest): Promise<CreateDeploymentResponse>;
  deleteFlow(flowSlug: string): Promise<DeleteFlowResponse>;
  deleteFlowSecret(flowSlug: string, name: string): Promise<DeleteFlowSecretResponse>;
  disableFlow(flowSlug: string): Promise<FlowStatusResponse['flow']>;
  enableFlow(flowSlug: string): Promise<FlowStatusResponse['flow']>;
  getFlow(flowSlug: string): Promise<GetFlowResponse['flow']>;
  getInvocation(invocationId: string): Promise<GetInvocationResponse['invocation']>;
  listInvocations(
    query?: ListFlowInvocationsQuery,
  ): Promise<ListInvocationsResponse['invocations']>;
  listSecrets(query?: ListSecretsQuery): Promise<ListSecretsResponse['secrets']>;
  listFlows(): Promise<ListFlowsResponse['flows']>;
  setFlowSecret(
    flowSlug: string,
    request: Omit<SetFlowSecretRequest, 'flow'>,
  ): Promise<SetFlowSecretResponse['secret']>;
  whoAmI(): Promise<WhoAmIResponse>;
};

export const TRIGORA_API_BASE_URL = 'https://api.trigora.dev';

function getConfiguredApiBaseUrl(): string {
  const configuredBaseUrl = process.env.TRIGORA_API_BASE_URL?.trim();
  return configuredBaseUrl && configuredBaseUrl.length > 0
    ? configuredBaseUrl
    : TRIGORA_API_BASE_URL;
}

type FetchHeaders = Record<string, string>;

type FetchRequest = {
  body?: string;
  headers?: FetchHeaders;
  method?: string;
};

type FetchResponse = {
  json(): Promise<unknown>;
  ok: boolean;
  status: number;
  text(): Promise<string>;
};

type FetchLike = (input: string, init?: FetchRequest) => Promise<FetchResponse>;

type DeployApiClientConfig = {
  baseUrl?: string;
  fetch?: FetchLike;
  token: string;
};

type ApiErrorPayload = {
  code?: ApiErrorCode;
  details?: unknown;
  message: string;
  step?: ApiErrorStep;
};

function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, '');
}

function isApiErrorCode(value: unknown): value is ApiErrorCode {
  return (
    value === 'bad_request' ||
    value === 'conflict' ||
    value === 'deployment_not_found' ||
    value === 'forbidden' ||
    value === 'internal_error' ||
    value === 'invalid_cron_expression' ||
    value === 'not_found' ||
    value === 'rate_limited' ||
    value === 'unauthorized'
  );
}

function isApiErrorStep(value: unknown): value is ApiErrorStep {
  return (
    value === 'uploading_package' ||
    value === 'worker_creation' ||
    value === 'dispatch_setup' ||
    value === 'activating'
  );
}

function isErrorPayload(
  value: unknown,
): value is { code?: ApiErrorCode; message: string; step?: ApiErrorStep } {
  return (
    typeof value === 'object' &&
    value !== null &&
    'message' in value &&
    typeof value.message === 'string' &&
    (!('code' in value) || value.code === undefined || isApiErrorCode(value.code)) &&
    (!('step' in value) || value.step === undefined || isApiErrorStep(value.step))
  );
}

function isNestedErrorPayload(value: unknown): value is ApiErrorResponse {
  return (
    typeof value === 'object' &&
    value !== null &&
    'error' in value &&
    typeof value.error === 'object' &&
    value.error !== null &&
    'message' in value.error &&
    typeof value.error.message === 'string' &&
    (!('code' in value.error) ||
      value.error.code === undefined ||
      isApiErrorCode(value.error.code)) &&
    (!('step' in value.error) || value.error.step === undefined || isApiErrorStep(value.error.step))
  );
}

function getErrorCodeFromStatus(status: number): ApiErrorCode | undefined {
  switch (status) {
    case 400:
      return 'bad_request';
    case 401:
      return 'unauthorized';
    case 403:
      return 'forbidden';
    case 404:
      return 'not_found';
    case 409:
      return 'conflict';
    case 429:
      return 'rate_limited';
    case 500:
      return 'internal_error';
    default:
      return undefined;
  }
}

function withFallbackErrorCode(response: ApiErrorPayload, status: number): ApiErrorPayload {
  return {
    code: response.code ?? getErrorCodeFromStatus(status),
    details: response.details,
    message: response.message,
    step: response.step,
  };
}

function getFallbackErrorResponse(status: number): ApiErrorPayload {
  const code = getErrorCodeFromStatus(status);

  if (code === 'unauthorized' || code === 'forbidden') {
    return {
      code,
      message: 'Deploy token is invalid or no longer active.',
    };
  }

  return withFallbackErrorCode(
    {
      message: `Request failed with status ${status}.`,
    },
    status,
  );
}

async function readErrorResponse(response: FetchResponse): Promise<ApiErrorPayload> {
  const payload = await response.json().catch(async () => {
    const text = await response.text().catch(() => '');
    return text;
  });

  if (isErrorPayload(payload)) {
    return withFallbackErrorCode(
      {
        code: payload.code,
        details: 'details' in payload ? payload.details : undefined,
        message: payload.message,
        step: payload.step,
      },
      response.status,
    );
  }

  if (isNestedErrorPayload(payload)) {
    return withFallbackErrorCode(
      {
        code: payload.error.code,
        details: 'details' in payload.error ? payload.error.details : undefined,
        message: payload.error.message,
        step: payload.error.step,
      },
      response.status,
    );
  }

  if (typeof payload === 'string' && payload.trim()) {
    return { message: payload };
  }

  return getFallbackErrorResponse(response.status);
}

export class DeployApiRequestError extends Error {
  readonly code?: ApiErrorCode;
  readonly details?: unknown;
  readonly status: number;
  readonly step?: ApiErrorStep;

  constructor(response: ApiErrorPayload, status: number) {
    super(response.message);
    this.name = 'DeployApiRequestError';
    this.code = response.code;
    this.details = response.details;
    this.status = status;
    this.step = response.step;
  }
}

export class DeployApiNetworkError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'DeployApiNetworkError';
  }
}

export class DeployApiResponseError extends Error {
  constructor() {
    super('Trigora Cloud returned an unexpected response.');
    this.name = 'DeployApiResponseError';
  }
}

function getNetworkErrorMessage(error: Error): string {
  if (error.message === 'Failed to fetch') {
    return 'Could not reach the Trigora API. Check your network connection.';
  }

  return error.message;
}

function isWebhookTrigger(
  value: unknown,
): value is { type: 'webhook'; event?: string; route?: string } {
  return (
    typeof value === 'object' &&
    value !== null &&
    'type' in value &&
    value.type === 'webhook' &&
    (!('event' in value) || typeof value.event === 'string') &&
    (!('route' in value) || typeof value.route === 'string')
  );
}

function isCronTrigger(value: unknown): value is { type: 'cron'; cron: string } {
  return (
    typeof value === 'object' &&
    value !== null &&
    'type' in value &&
    value.type === 'cron' &&
    'cron' in value &&
    typeof value.cron === 'string'
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function getOptionalString(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined;
}

function isFlowStatus(value: unknown): value is FlowStatus {
  return value === 'ready' || value === 'disabled' || value === 'failed';
}

function isPlan(value: unknown): value is CreateDeploymentResponse['plan'] {
  return value === 'free' || value === 'pro' || value === 'scale' || value === 'internal';
}

function isFlowInvocationStatus(value: unknown): value is FlowInvocationStatus {
  return value === 'running' || value === 'succeeded' || value === 'failed';
}

function isFlowInvocationLogLevel(value: unknown): value is FlowInvocationLogLevel {
  return value === 'info' || value === 'warn' || value === 'error';
}

function getNullableString(value: unknown): string | null | undefined {
  return value === null || typeof value === 'string' ? value : undefined;
}

function getNullableNumber(value: unknown): number | null | undefined {
  return value === null || typeof value === 'number' ? value : undefined;
}

function normalizeFlowTriggerType(value: unknown): FlowTriggerType | undefined {
  if (value === 'webhook' || value === 'cron' || value === 'queue') {
    return value;
  }

  if (isRecord(value) && typeof value.type === 'string') {
    return normalizeFlowTriggerType(value.type);
  }

  return undefined;
}

function normalizeFlowRecord(value: unknown): FlowRecord | undefined {
  if (!isRecord(value)) {
    return undefined;
  }

  const trigger = normalizeFlowTriggerType(value.trigger);
  const id = getOptionalString(value.id);
  const slug = getOptionalString(value.slug);
  const createdAt = getOptionalString(value.createdAt);
  const status = isFlowStatus(value.status) ? value.status : undefined;

  if (!trigger || !id || !slug || !createdAt || !status) {
    return undefined;
  }

  switch (trigger) {
    case 'webhook': {
      const endpoint = getOptionalString(value.endpoint) ?? getOptionalString(value.url);
      const routePath = getOptionalString(value.routePath);

      if (!endpoint || !routePath) {
        return undefined;
      }

      return {
        id,
        slug,
        trigger,
        status,
        createdAt,
        routePath,
        endpoint,
      };
    }
    case 'cron': {
      const schedule = getOptionalString(value.schedule) ?? getOptionalString(value.cron);
      const timezone = getOptionalString(value.timezone);

      if (!schedule || timezone !== 'UTC') {
        return undefined;
      }

      return {
        id,
        slug,
        trigger,
        status,
        createdAt,
        schedule,
        timezone,
      };
    }
    case 'queue':
      return {
        id,
        slug,
        trigger,
        status,
        createdAt,
        queue: getOptionalString(value.queue) ?? getOptionalString(value.topic),
      };
  }
}

function readFlowListResponse(payload: unknown): ListFlowsResponse | undefined {
  if (!isRecord(payload) || !Array.isArray(payload.flows)) {
    return undefined;
  }

  const flows = payload.flows.map((value) => normalizeFlowRecord(value));

  if (!flows.every((flow): flow is FlowRecord => Boolean(flow))) {
    return undefined;
  }

  return { flows };
}

function readFlowResponse(payload: unknown): GetFlowResponse | undefined {
  if (!isRecord(payload) || !('flow' in payload)) {
    return undefined;
  }

  const flow = normalizeFlowRecord(payload.flow);

  return flow ? { flow } : undefined;
}

function readWhoAmIResponse(payload: unknown): WhoAmIResponse | undefined {
  if (
    !isRecord(payload) ||
    !isRecord(payload.workspace) ||
    typeof payload.workspace.id !== 'string' ||
    typeof payload.workspace.slug !== 'string' ||
    typeof payload.workspace.name !== 'string' ||
    !isPlan(payload.workspace.plan) ||
    typeof payload.actorType !== 'string'
  ) {
    return undefined;
  }

  const workspace = {
    id: payload.workspace.id,
    name: payload.workspace.name,
    plan: payload.workspace.plan,
    slug: payload.workspace.slug,
  };

  if (
    payload.actorType === 'deploy_token' &&
    isRecord(payload.token) &&
    typeof payload.token.id === 'string' &&
    typeof payload.token.label === 'string' &&
    typeof payload.token.status === 'string' &&
    typeof payload.token.createdAt === 'string'
  ) {
    return {
      actorType: 'deploy_token',
      workspace,
      token: {
        id: payload.token.id,
        label: payload.token.label,
        status: payload.token.status,
        createdAt: payload.token.createdAt,
      },
    };
  }

  if (
    payload.actorType === 'user' &&
    isRecord(payload.user) &&
    typeof payload.user.id === 'string' &&
    typeof payload.user.email === 'string' &&
    typeof payload.user.emailVerified === 'boolean' &&
    (payload.user.image === null || typeof payload.user.image === 'string') &&
    typeof payload.user.name === 'string' &&
    typeof payload.workspace.role === 'string'
  ) {
    return {
      actorType: 'user',
      user: {
        id: payload.user.id,
        email: payload.user.email,
        emailVerified: payload.user.emailVerified,
        image: payload.user.image,
        name: payload.user.name,
      },
      workspace: {
        ...workspace,
        role: payload.workspace.role,
      },
    };
  }

  return undefined;
}

function readFlowStatusResponse(payload: unknown): FlowStatusResponse | undefined {
  if (!isRecord(payload) || payload.ok !== true || !('flow' in payload)) {
    return undefined;
  }

  const trigger = normalizeFlowTriggerType(
    isRecord(payload.flow) ? payload.flow.trigger : undefined,
  );

  if (
    !isRecord(payload.flow) ||
    typeof payload.flow.id !== 'string' ||
    !isFlowStatus(payload.flow.status) ||
    typeof payload.flow.slug !== 'string' ||
    !trigger
  ) {
    return undefined;
  }

  if (trigger === 'webhook') {
    const routePath = getOptionalString(payload.flow.routePath);
    const endpoint =
      getOptionalString(payload.flow.endpoint) ?? getOptionalString(payload.flow.url);

    if (!routePath || !endpoint) {
      return undefined;
    }

    return {
      ok: true,
      flow: {
        id: payload.flow.id,
        slug: payload.flow.slug,
        status: payload.flow.status,
        trigger,
        routePath,
        endpoint,
      },
    };
  }

  if (trigger === 'cron') {
    const schedule =
      getOptionalString(payload.flow.schedule) ?? getOptionalString(payload.flow.cron);
    const timezone = getOptionalString(payload.flow.timezone);

    if (!schedule || timezone !== 'UTC') {
      return undefined;
    }

    return {
      ok: true,
      flow: {
        id: payload.flow.id,
        slug: payload.flow.slug,
        status: payload.flow.status,
        trigger,
        schedule,
        timezone,
      },
    };
  }

  return {
    ok: true,
    flow: {
      id: payload.flow.id,
      slug: payload.flow.slug,
      status: payload.flow.status,
      trigger,
      queue: getOptionalString(payload.flow.queue) ?? getOptionalString(payload.flow.topic),
    },
  };
}

function normalizeFlowSecretRecord(
  value: unknown,
): ListSecretsResponse['secrets'][number] | undefined {
  if (
    !isRecord(value) ||
    typeof value.flowSlug !== 'string' ||
    typeof value.name !== 'string' ||
    typeof value.createdAt !== 'string' ||
    typeof value.updatedAt !== 'string'
  ) {
    return undefined;
  }

  return {
    flowSlug: value.flowSlug,
    name: value.name,
    createdAt: value.createdAt,
    updatedAt: value.updatedAt,
  };
}

function isFlowSecretRecord(value: unknown): value is FlowSecretRecord {
  return (
    isRecord(value) &&
    typeof value.name === 'string' &&
    typeof value.createdAt === 'string' &&
    typeof value.updatedAt === 'string'
  );
}

function readFlowSecretsResponse(payload: unknown): ListSecretsResponse | undefined {
  if (!isRecord(payload) || !Array.isArray(payload.secrets)) {
    return undefined;
  }

  const secrets = payload.secrets.map((value) => normalizeFlowSecretRecord(value));

  if (
    !secrets.every((secret): secret is ListSecretsResponse['secrets'][number] => Boolean(secret))
  ) {
    return undefined;
  }

  return { secrets };
}

function readSetFlowSecretResponse(payload: unknown): SetFlowSecretResponse | undefined {
  if (!isRecord(payload) || payload.ok !== true || !('secret' in payload)) {
    return undefined;
  }

  return isFlowSecretRecord(payload.secret)
    ? {
        ok: true,
        secret: payload.secret,
      }
    : undefined;
}

function readDeleteFlowSecretResponse(payload: unknown): DeleteFlowSecretResponse | undefined {
  if (
    !isRecord(payload) ||
    payload.ok !== true ||
    payload.deleted !== true ||
    typeof payload.name !== 'string'
  ) {
    return undefined;
  }

  return {
    ok: true,
    deleted: true,
    name: payload.name,
  };
}

function readDeleteFlowResponse(payload: unknown): DeleteFlowResponse | undefined {
  if (!isRecord(payload) || payload.deleted !== true) {
    return undefined;
  }

  return {
    deleted: true,
  };
}

function normalizeFlowInvocationRecord(value: unknown): FlowInvocationRecord | undefined {
  if (!isRecord(value)) {
    return undefined;
  }

  const id = getOptionalString(value.id);
  const status = isFlowInvocationStatus(value.status) ? value.status : undefined;
  const startedAt = getOptionalString(value.startedAt);
  const completedAt = getNullableString(value.completedAt);
  const durationMs = getNullableNumber(value.durationMs);
  const httpStatus = getNullableNumber(value.httpStatus);
  const errorCode = getNullableString(value.errorCode);
  const errorMessage = getNullableString(value.errorMessage);

  if (
    !id ||
    !status ||
    !startedAt ||
    completedAt === undefined ||
    durationMs === undefined ||
    httpStatus === undefined ||
    errorCode === undefined ||
    errorMessage === undefined
  ) {
    return undefined;
  }

  return {
    id,
    status,
    startedAt,
    completedAt,
    durationMs,
    httpStatus,
    errorCode,
    errorMessage,
  };
}

function normalizeInvocationSummaryRecord(
  value: unknown,
): ListInvocationsResponse['invocations'][number] | undefined {
  if (!isRecord(value) || typeof value.flowSlug !== 'string') {
    return undefined;
  }

  const invocation = normalizeFlowInvocationRecord(value);

  return invocation
    ? {
        ...invocation,
        flowSlug: value.flowSlug,
      }
    : undefined;
}

function normalizeFlowInvocationLogRecord(value: unknown): FlowInvocationLogRecord | undefined {
  if (!isRecord(value)) {
    return undefined;
  }

  const sequence = typeof value.sequence === 'number' ? value.sequence : undefined;
  const level = isFlowInvocationLogLevel(value.level) ? value.level : undefined;
  const message = getOptionalString(value.message);
  const timestamp = getOptionalString(value.timestamp);
  const metadata =
    value.metadata === undefined
      ? undefined
      : value.metadata === null
        ? null
        : isRecord(value.metadata) && !Array.isArray(value.metadata)
          ? (value.metadata as Record<string, unknown>)
          : undefined;

  if (sequence === undefined || !level || !message || !timestamp || metadata === undefined) {
    return undefined;
  }

  return {
    sequence,
    level,
    message,
    timestamp,
    metadata,
  };
}

function normalizeInvocationExecutionTrigger(
  value: unknown,
): InvocationExecutionContext['trigger'] | undefined {
  if (!isRecord(value) || typeof value.type !== 'string') {
    return undefined;
  }

  if (value.type === 'webhook') {
    const endpoint = getOptionalString(value.endpoint);
    const routePath = getOptionalString(value.routePath);

    return endpoint && routePath
      ? {
          type: 'webhook',
          endpoint,
          routePath,
        }
      : undefined;
  }

  if (value.type === 'cron') {
    const cron = getNullableString(value.cron);
    const scheduledAt = getNullableString(value.scheduledAt);
    const lastRunAt = getNullableString(value.lastRunAt);
    const nextRunAt = getNullableString(value.nextRunAt);
    const timezone = getOptionalString(value.timezone);

    return cron !== undefined &&
      scheduledAt !== undefined &&
      lastRunAt !== undefined &&
      nextRunAt !== undefined &&
      timezone
      ? {
          type: 'cron',
          cron,
          scheduledAt,
          lastRunAt,
          nextRunAt,
          timezone,
        }
      : undefined;
  }

  return undefined;
}

function normalizeInvocationExecutionContext(
  value: unknown,
): InvocationExecutionContext | undefined {
  if (!isRecord(value)) {
    return undefined;
  }

  const attempt = getNullableNumber(value.attempt);
  const deploymentId = getOptionalString(value.deploymentId);
  const flowSlug = getOptionalString(value.flowSlug);
  const invocationId = getOptionalString(value.invocationId);
  const trigger = normalizeInvocationExecutionTrigger(value.trigger);
  const triggerType = getOptionalString(value.triggerType);
  const workspaceSlug = getOptionalString(value.workspaceSlug);

  if (
    attempt === undefined ||
    !deploymentId ||
    !flowSlug ||
    !invocationId ||
    !trigger ||
    !triggerType ||
    !workspaceSlug
  ) {
    return undefined;
  }

  return {
    attempt,
    deploymentId,
    flowSlug,
    invocationId,
    trigger,
    triggerType,
    workspaceSlug,
  };
}

function readFlowInvocationsResponse(payload: unknown): ListInvocationsResponse | undefined {
  if (!isRecord(payload) || !Array.isArray(payload.invocations)) {
    return undefined;
  }

  const invocations = payload.invocations.map((value) => normalizeInvocationSummaryRecord(value));

  if (
    !invocations.every((invocation): invocation is ListInvocationsResponse['invocations'][number] =>
      Boolean(invocation),
    )
  ) {
    return undefined;
  }

  return { invocations };
}

function readFlowInvocationResponse(payload: unknown): GetInvocationResponse | undefined {
  if (!isRecord(payload) || !('invocation' in payload) || !isRecord(payload.invocation)) {
    return undefined;
  }

  const invocation = normalizeFlowInvocationRecord(payload.invocation);
  const flowSlug = getOptionalString(payload.invocation.flowSlug);
  const triggerType = getOptionalString(payload.invocation.triggerType);
  const executionContext = normalizeInvocationExecutionContext(payload.invocation.executionContext);
  const logs = Array.isArray(payload.invocation.logs)
    ? payload.invocation.logs.map((value) => normalizeFlowInvocationLogRecord(value))
    : undefined;

  if (
    !invocation ||
    !flowSlug ||
    !triggerType ||
    !executionContext ||
    !logs ||
    !logs.every((log): log is FlowInvocationLogRecord => Boolean(log))
  ) {
    return undefined;
  }

  return {
    invocation: {
      ...invocation,
      flowSlug,
      triggerType,
      executionContext,
      logs,
    },
  };
}

function isDeploymentFlow(
  value: unknown,
): value is CreateDeploymentResponse['manifestJson']['flow'] {
  if (
    typeof value !== 'object' ||
    value === null ||
    !('id' in value) ||
    typeof value.id !== 'string' ||
    !('entrypoint' in value) ||
    typeof value.entrypoint !== 'string' ||
    !('trigger' in value)
  ) {
    return false;
  }

  return isWebhookTrigger(value.trigger) || isCronTrigger(value.trigger);
}

function isDeploymentFlowResponse(value: unknown): value is CreateDeploymentResponse['flow'] {
  if (
    typeof value !== 'object' ||
    value === null ||
    !('id' in value) ||
    typeof value.id !== 'string' ||
    !('slug' in value) ||
    typeof value.slug !== 'string' ||
    !('status' in value) ||
    !isFlowStatus(value.status) ||
    !('trigger' in value)
  ) {
    return false;
  }

  if (value.trigger === 'webhook') {
    return (
      'url' in value &&
      typeof value.url === 'string' &&
      'routePath' in value &&
      typeof value.routePath === 'string'
    );
  }

  if (value.trigger === 'cron') {
    return (
      'schedule' in value &&
      typeof value.schedule === 'string' &&
      'timezone' in value &&
      value.timezone === 'UTC' &&
      'url' in value &&
      value.url === null
    );
  }

  return false;
}

function isDeploymentResponse(payload: unknown): payload is CreateDeploymentResponse {
  return (
    typeof payload === 'object' &&
    payload !== null &&
    'id' in payload &&
    typeof payload.id === 'string' &&
    'plan' in payload &&
    isPlan(payload.plan) &&
    'status' in payload &&
    (payload.status === 'active' || payload.status === 'failed') &&
    'manifestVersion' in payload &&
    typeof payload.manifestVersion === 'number' &&
    'manifestJson' in payload &&
    typeof payload.manifestJson === 'object' &&
    payload.manifestJson !== null &&
    'version' in payload.manifestJson &&
    typeof payload.manifestJson.version === 'number' &&
    'flow' in payload.manifestJson &&
    isDeploymentFlow(payload.manifestJson.flow) &&
    'flow' in payload &&
    isDeploymentFlowResponse(payload.flow) &&
    'createdAt' in payload &&
    typeof payload.createdAt === 'string' &&
    'updatedAt' in payload &&
    typeof payload.updatedAt === 'string'
  );
}

export function createDeployApiClient(config: DeployApiClientConfig): DeployApiClient {
  const fetchImpl = config.fetch ?? globalThis.fetch;

  if (!fetchImpl) {
    throw new Error('Fetch is not available in this environment.');
  }

  const baseUrl = normalizeBaseUrl(config.baseUrl ?? getConfiguredApiBaseUrl());

  function createAuthorizedHeaders(includeJsonContentType = false): FetchHeaders {
    return {
      Authorization: `Bearer ${config.token}`,
      ...(includeJsonContentType ? { 'Content-Type': 'application/json' } : {}),
    };
  }

  return {
    async deleteFlow(flowSlug) {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/flows/${encodeURIComponent(flowSlug)}`, {
          method: 'DELETE',
          headers: createAuthorizedHeaders(),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const deleteFlowResponse = readDeleteFlowResponse(payload);

      if (!deleteFlowResponse) {
        throw new DeployApiResponseError();
      }

      return deleteFlowResponse;
    },
    async createDeployment(request) {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/deployments`, {
          method: 'POST',
          headers: createAuthorizedHeaders(true),
          body: JSON.stringify(request),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();

      if (!isDeploymentResponse(payload)) {
        throw new DeployApiResponseError();
      }

      return payload;
    },
    async setFlowSecret(flowSlug, request) {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/secrets`, {
          method: 'POST',
          headers: createAuthorizedHeaders(true),
          body: JSON.stringify({
            ...request,
            flow: flowSlug,
          }),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const setSecretResponse = readSetFlowSecretResponse(payload);

      if (!setSecretResponse) {
        throw new DeployApiResponseError();
      }

      return setSecretResponse.secret;
    },
    async listSecrets(query = {}) {
      let response: FetchResponse;
      const url =
        query.flow !== undefined
          ? `${baseUrl}/v1/secrets?flow=${encodeURIComponent(query.flow)}`
          : `${baseUrl}/v1/secrets`;

      try {
        response = await fetchImpl(url, {
          method: 'GET',
          headers: createAuthorizedHeaders(),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const secretListResponse = readFlowSecretsResponse(payload);

      if (!secretListResponse) {
        throw new DeployApiResponseError();
      }

      return secretListResponse.secrets;
    },
    async listFlows() {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/flows`, {
          method: 'GET',
          headers: createAuthorizedHeaders(),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const flowListResponse = readFlowListResponse(payload);

      if (!flowListResponse) {
        throw new DeployApiResponseError();
      }

      return flowListResponse.flows;
    },
    async getFlow(flowSlug) {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/flows/${encodeURIComponent(flowSlug)}`, {
          method: 'GET',
          headers: createAuthorizedHeaders(),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const flowResponse = readFlowResponse(payload);

      if (!flowResponse) {
        throw new DeployApiResponseError();
      }

      return flowResponse.flow;
    },
    async whoAmI() {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/whoami`, {
          method: 'GET',
          headers: createAuthorizedHeaders(),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const whoAmIResponse = readWhoAmIResponse(payload);

      if (!whoAmIResponse) {
        throw new DeployApiResponseError();
      }

      return whoAmIResponse;
    },
    async listInvocations(query) {
      let response: FetchResponse;
      const search = new URLSearchParams();

      if (query?.flow) {
        search.set('flow', query.flow);
      }

      if (query?.limit !== undefined) {
        search.set('limit', String(query.limit));
      }

      if (query?.range) {
        search.set('range', query.range);
      }

      if (query?.status) {
        search.set('status', query.status);
      }

      const url = `${baseUrl}/v1/invocations${search.size > 0 ? `?${search.toString()}` : ''}`;

      try {
        response = await fetchImpl(url, {
          method: 'GET',
          headers: createAuthorizedHeaders(),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const invocationsResponse = readFlowInvocationsResponse(payload);

      if (!invocationsResponse) {
        throw new DeployApiResponseError();
      }

      return invocationsResponse.invocations;
    },
    async getInvocation(invocationId) {
      let response: FetchResponse;

      try {
        response = await fetchImpl(
          `${baseUrl}/v1/invocations/${encodeURIComponent(invocationId)}`,
          {
            method: 'GET',
            headers: createAuthorizedHeaders(),
          },
        );
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const invocationResponse = readFlowInvocationResponse(payload);

      if (!invocationResponse) {
        throw new DeployApiResponseError();
      }

      return invocationResponse.invocation;
    },
    async disableFlow(flowSlug) {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/flows/${encodeURIComponent(flowSlug)}/disable`, {
          method: 'POST',
          headers: createAuthorizedHeaders(),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const disableFlowResponse = readFlowStatusResponse(payload);

      if (!disableFlowResponse) {
        throw new DeployApiResponseError();
      }

      return disableFlowResponse.flow;
    },
    async enableFlow(flowSlug) {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/flows/${encodeURIComponent(flowSlug)}/enable`, {
          method: 'POST',
          headers: createAuthorizedHeaders(),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const enableFlowResponse = readFlowStatusResponse(payload);

      if (!enableFlowResponse) {
        throw new DeployApiResponseError();
      }

      return enableFlowResponse.flow;
    },
    async deleteFlowSecret(flowSlug, name) {
      let response: FetchResponse;
      const url = `${baseUrl}/v1/secrets/${encodeURIComponent(name)}?flow=${encodeURIComponent(flowSlug)}`;

      try {
        response = await fetchImpl(url, {
          method: 'DELETE',
          headers: createAuthorizedHeaders(),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(getNetworkErrorMessage(error));
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const deleteSecretResponse = readDeleteFlowSecretResponse(payload);

      if (!deleteSecretResponse) {
        throw new DeployApiResponseError();
      }

      return deleteSecretResponse;
    },
  };
}
