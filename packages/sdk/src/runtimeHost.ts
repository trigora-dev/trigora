import { AsyncLocalStorage } from 'node:async_hooks';

import type { JsonValue, WaitForEventOptions } from '@trigora/contracts';

export type ExecutionInfo = {
  id: string;
  attempt: number;
  programId: string;
  signal: AbortSignal;
};

export type DurableRuntimeHost = {
  effect<T>(name: string | undefined, run: () => T | Promise<T>): Promise<T>;
  sleep(duration: string | number): Promise<void>;
  waitForEvent<T>(name: string, options?: WaitForEventOptions): Promise<T>;
  invoke<TInput, TResult>(programId: string, input: TInput): Promise<TResult>;
  getExecution(): ExecutionInfo;
};

const runtimeStorage = new AsyncLocalStorage<DurableRuntimeHost>();

export function runWithDurableRuntime<T>(
  host: DurableRuntimeHost,
  fn: () => Promise<T>,
): Promise<T> {
  return runtimeStorage.run(host, fn);
}

export function getDurableRuntimeHost(): DurableRuntimeHost {
  const host = runtimeStorage.getStore();

  if (!host) {
    throw new Error(
      'Durable primitives can only run inside a Trigora execution. Start the program with `@trigora/client` while `trigora dev` is running.',
    );
  }

  return host;
}

export type { JsonValue, WaitForEventOptions };
