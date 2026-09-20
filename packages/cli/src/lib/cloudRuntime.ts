import { createClient, type TrigoraClient } from '@trigora/client';

import { CliDisplayError } from './cliOutput';
import { getApiToken } from './getApiToken';
import { toWhoAmIApiFailure } from './whoamiOutput';

export function isCloudAuthenticated(): boolean {
  return Boolean(getApiToken());
}

export function createCommandClient(): TrigoraClient {
  const token = getApiToken();
  if (token) {
    return createClient({ token });
  }
  return createClient();
}

export function requireCloudClient(): TrigoraClient {
  const token = getApiToken();
  if (!token) {
    throw new CliDisplayError({
      title: 'Not authenticated',
      details: [{ label: 'Reason', value: 'TRIGORA_TOKEN is not set.' }],
      hint: 'Set your API token and try again.',
    });
  }
  return createClient({ token });
}

export async function withCloud<T>(fn: () => Promise<T>): Promise<T> {
  try {
    return await fn();
  } catch (error) {
    throw toWhoAmIApiFailure(error, 'Calling Trigora Cloud');
  }
}

export function createCommandRuntime(): {
  cloud: boolean;
  client: TrigoraClient;
} {
  const token = getApiToken();
  return {
    cloud: Boolean(token),
    client: createClient(token ? { token } : {}),
  };
}
