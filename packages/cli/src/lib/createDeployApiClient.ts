import type {
  ApiErrorCode,
  ApiErrorResponse,
  ApiErrorStep,
  CreateDeploymentRequest,
  CreateDeploymentResponse,
  DisableFlowResponse,
  FlowRecord,
  FlowStatus,
  FlowTriggerType,
  GetFlowResponse,
  ListFlowsResponse,
} from '@trigora/contracts';

export type DeployApiClient = {
  createDeployment(request: CreateDeploymentRequest): Promise<CreateDeploymentResponse>;
  disableFlow(flowId: string): Promise<DisableFlowResponse['flow']>;
  getFlow(flowId: string): Promise<GetFlowResponse['flow']>;
  listFlows(): Promise<ListFlowsResponse['flows']>;
};

// export const TRIGORA_API_BASE_URL = 'https://api.trigora.dev';
export const TRIGORA_API_BASE_URL = 'http://localhost:8787';

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
  readonly status: number;
  readonly step?: ApiErrorStep;

  constructor(response: ApiErrorPayload, status: number) {
    super(response.message);
    this.name = 'DeployApiRequestError';
    this.code = response.code;
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

function isWebhookTrigger(value: unknown): value is { type: 'webhook'; event?: string } {
  return (
    typeof value === 'object' &&
    value !== null &&
    'type' in value &&
    value.type === 'webhook' &&
    (!('event' in value) || typeof value.event === 'string')
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
  const name = getOptionalString(value.name);
  const createdAt = getOptionalString(value.createdAt);
  const status = isFlowStatus(value.status) ? value.status : undefined;

  if (!trigger || !id || !name || !createdAt || !status) {
    return undefined;
  }

  switch (trigger) {
    case 'webhook': {
      const endpoint = getOptionalString(value.endpoint) ?? getOptionalString(value.url);

      if (!endpoint) {
        return undefined;
      }

      return {
        id,
        name,
        trigger,
        status,
        createdAt,
        route: getOptionalString(value.route) ?? getOptionalString(value.routePath),
        endpoint,
      };
    }
    case 'cron':
      return {
        id,
        name,
        trigger,
        status,
        createdAt,
        schedule: getOptionalString(value.schedule) ?? getOptionalString(value.cron),
      };
    case 'queue':
      return {
        id,
        name,
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

function readDisableFlowResponse(payload: unknown): DisableFlowResponse | undefined {
  if (
    !isRecord(payload) ||
    payload.ok !== true ||
    !('flow' in payload) ||
    !isRecord(payload.flow) ||
    typeof payload.flow.id !== 'string' ||
    !isFlowStatus(payload.flow.status)
  ) {
    return undefined;
  }

  return {
    ok: true,
    flow: {
      id: payload.flow.id,
      status: payload.flow.status,
    },
  };
}

function isDeploymentFlow(
  value: unknown,
): value is CreateDeploymentResponse['manifestJson']['flows'][number] {
  return (
    typeof value === 'object' &&
    value !== null &&
    'id' in value &&
    typeof value.id === 'string' &&
    'entrypoint' in value &&
    typeof value.entrypoint === 'string' &&
    'routePath' in value &&
    typeof value.routePath === 'string' &&
    'trigger' in value &&
    isWebhookTrigger(value.trigger)
  );
}

function isDeploymentFlowResponse(
  value: unknown,
): value is CreateDeploymentResponse['flows'][number] {
  return (
    typeof value === 'object' &&
    value !== null &&
    'id' in value &&
    typeof value.id === 'string' &&
    'flowId' in value &&
    typeof value.flowId === 'string' &&
    'routePath' in value &&
    typeof value.routePath === 'string' &&
    'status' in value &&
    typeof value.status === 'string' &&
    'url' in value &&
    (typeof value.url === 'string' || value.url === null)
  );
}

function isDeploymentResponse(payload: unknown): payload is CreateDeploymentResponse {
  return (
    typeof payload === 'object' &&
    payload !== null &&
    'id' in payload &&
    typeof payload.id === 'string' &&
    'status' in payload &&
    (payload.status === 'pending' || payload.status === 'active' || payload.status === 'failed') &&
    'manifestVersion' in payload &&
    typeof payload.manifestVersion === 'number' &&
    'manifestJson' in payload &&
    typeof payload.manifestJson === 'object' &&
    payload.manifestJson !== null &&
    'version' in payload.manifestJson &&
    typeof payload.manifestJson.version === 'number' &&
    'flows' in payload.manifestJson &&
    Array.isArray(payload.manifestJson.flows) &&
    payload.manifestJson.flows.every(isDeploymentFlow) &&
    'flowCount' in payload &&
    typeof payload.flowCount === 'number' &&
    'baseUrl' in payload &&
    (typeof payload.baseUrl === 'string' || payload.baseUrl === null) &&
    'url' in payload &&
    (typeof payload.url === 'string' || payload.url === null) &&
    'flows' in payload &&
    Array.isArray(payload.flows) &&
    payload.flows.every(isDeploymentFlowResponse) &&
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

  const baseUrl = normalizeBaseUrl(config.baseUrl ?? TRIGORA_API_BASE_URL);

  return {
    async createDeployment(request) {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/deployments`, {
          method: 'POST',
          headers: {
            Authorization: `Bearer ${config.token}`,
            'Content-Type': 'application/json',
          },
          body: JSON.stringify(request),
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(error.message);
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
    async listFlows() {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/flows`, {
          method: 'GET',
          headers: {
            Authorization: `Bearer ${config.token}`,
          },
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(error.message);
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
    async getFlow(flowId) {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/flows/${flowId}`, {
          method: 'GET',
          headers: {
            Authorization: `Bearer ${config.token}`,
          },
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(error.message);
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
    async disableFlow(flowId) {
      let response: FetchResponse;

      try {
        response = await fetchImpl(`${baseUrl}/v1/flows/${flowId}/disable`, {
          method: 'POST',
          headers: {
            Authorization: `Bearer ${config.token}`,
          },
        });
      } catch (error) {
        if (error instanceof Error) {
          throw new DeployApiNetworkError(error.message);
        }

        throw new DeployApiNetworkError('Could not reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const apiError = await readErrorResponse(response);
        throw new DeployApiRequestError(apiError, response.status);
      }

      const payload = await response.json();
      const disableFlowResponse = readDisableFlowResponse(payload);

      if (!disableFlowResponse) {
        throw new DeployApiResponseError();
      }

      return disableFlowResponse.flow;
    },
  };
}
