import type { CreateDeploymentRequest, CreateDeploymentResponse } from '@trigora/contracts';

export type DeployApiClient = {
  createDeployment(request: CreateDeploymentRequest): Promise<CreateDeploymentResponse>;
};

export const TRIGORA_API_BASE_URL = 'https://api.trigora.dev';

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

      if (
        typeof payload !== 'object' ||
        payload === null ||
        !('deploymentId' in payload) ||
        typeof payload.deploymentId !== 'string' ||
        !('status' in payload) ||
        (payload.status !== 'pending' && payload.status !== 'active' && payload.status !== 'failed')
      ) {
        throw new Error('Trigora deploy API returned an invalid deployment response.');
      }

      const dashboardUrl =
        'dashboardUrl' in payload && typeof payload.dashboardUrl === 'string'
          ? payload.dashboardUrl
          : undefined;

      return {
        deploymentId: payload.deploymentId,
        status: payload.status,
        dashboardUrl,
      };
    },
  };
}
