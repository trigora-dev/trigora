import type { CreateDeploymentRequest, CreateDeploymentResponse } from '@trigora/contracts';

export type DeployApiClient = {
  createDeployment(request: CreateDeploymentRequest): Promise<CreateDeploymentResponse>;
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

function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, '');
}

function isErrorPayload(value: unknown): value is { message: string } {
  return (
    typeof value === 'object' &&
    value !== null &&
    'message' in value &&
    typeof value.message === 'string'
  );
}

async function readErrorMessage(response: FetchResponse): Promise<string> {
  const payload = await response.json().catch(async () => {
    const text = await response.text().catch(() => '');
    return text;
  });

  if (isErrorPayload(payload)) {
    return payload.message;
  }

  if (typeof payload === 'string' && payload.trim()) {
    return payload;
  }

  return `Request failed with status ${response.status}.`;
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
          throw new Error(`Failed to reach the Trigora deploy API: ${error.message}`);
        }

        throw new Error('Failed to reach the Trigora deploy API.');
      }

      if (!response.ok) {
        const message = await readErrorMessage(response);
        throw new Error(`Trigora deploy API request failed: ${message}`);
      }

      const payload = await response.json();

      if (!isDeploymentResponse(payload)) {
        throw new Error('Internal server error.');
      }

      return payload;
    },
  };
}
