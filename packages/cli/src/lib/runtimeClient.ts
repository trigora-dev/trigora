import { createClient, TrigoraRuntimeError, type TrigoraClient } from '@trigora/client';

import { CliDisplayError } from './cliOutput';

export function createRuntimeClient(): TrigoraClient {
  return createClient();
}

export function toRuntimeFailure(error: unknown): CliDisplayError {
  if (error instanceof CliDisplayError) {
    return error;
  }

  const message = error instanceof Error ? error.message : String(error);
  const hint =
    error instanceof TrigoraRuntimeError && error.status === 0
      ? 'Start `trigora dev` and try again.'
      : undefined;

  return new CliDisplayError({
    title: 'Runtime request failed',
    details: [{ label: 'Reason', value: message }],
    hint,
    message,
  });
}

export async function withRuntime<T>(fn: () => Promise<T>): Promise<T> {
  try {
    return await fn();
  } catch (error) {
    throw toRuntimeFailure(error);
  }
}
