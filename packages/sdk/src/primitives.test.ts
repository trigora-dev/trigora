import { describe, expect, it, vi } from 'vitest';

import { defineConfig } from './defineConfig';
import { effect } from './effect';
import { event, resolveEventName } from './event';
import { execution } from './execution';
import { invoke } from './invoke';
import { resolveProgramId } from './program';
import { sleep } from './sleep';
import { waitForEvent } from './waitForEvent';
import { runWithDurableRuntime, type DurableRuntimeHost } from './runtimeHost';

function createHost(overrides: Partial<DurableRuntimeHost> = {}): DurableRuntimeHost {
  return {
    effect: async (_name, run) => run(),
    sleep: async () => undefined,
    waitForEvent: async () => ({}) as never,
    invoke: async () => undefined as never,
    getExecution: () => ({
      id: 'exec_test',
      attempt: 1,
      programId: 'researchAgent',
      signal: new AbortController().signal,
    }),
    ...overrides,
  };
}

describe('durable sdk primitives', () => {
  it('throws when used outside an execution', () => {
    expect(() => {
      void effect(() => 1);
    }).toThrow(/inside a Trigora execution/);
    expect(() => execution.id).toThrow(/inside a Trigora execution/);
  });

  it('runs named and unnamed effects through the host', async () => {
    const host = createHost({
      effect: vi.fn(async (_name, run) => run()),
    });

    await runWithDurableRuntime(host, async () => {
      await expect(effect(() => 2)).resolves.toBe(2);
      await expect(effect('search', () => 'ok')).resolves.toBe('ok');
    });

    expect(host.effect).toHaveBeenCalledTimes(2);
    expect(host.effect).toHaveBeenNthCalledWith(1, undefined, expect.any(Function));
    expect(host.effect).toHaveBeenNthCalledWith(2, 'search', expect.any(Function));
  });

  it('exposes execution metadata from the host', async () => {
    const signal = new AbortController().signal;
    const host = createHost({
      getExecution: () => ({
        id: 'exec_42',
        attempt: 3,
        programId: 'researchAgent',
        signal,
      }),
    });

    await runWithDurableRuntime(host, async () => {
      expect(execution.id).toBe('exec_42');
      expect(execution.attempt).toBe(3);
      expect(execution.programId).toBe('researchAgent');
      expect(execution.signal).toBe(signal);
    });
  });

  it('forwards sleep, waitForEvent, and invoke', async () => {
    const host = createHost({
      sleep: vi.fn(async () => undefined),
      waitForEvent: vi.fn(async () => ({ reviewer: 'Omar' })) as DurableRuntimeHost['waitForEvent'],
      invoke: vi.fn(async () => ({ summary: 'done' })) as DurableRuntimeHost['invoke'],
    });
    const approved = event<{ reviewer: string }>('approved');

    async function analyzeSource(input: { source: string }) {
      return { summary: input.source };
    }

    await runWithDurableRuntime(host, async () => {
      await sleep('5s');
      await expect(waitForEvent(approved)).resolves.toEqual({ reviewer: 'Omar' });
      await expect(invoke(analyzeSource, { source: 'https://example.com' })).resolves.toEqual({
        summary: 'done',
      });
    });

    expect(host.sleep).toHaveBeenCalledWith('5s');
    expect(host.waitForEvent).toHaveBeenCalledWith('approved', undefined);
    expect(host.invoke).toHaveBeenCalledWith('analyzeSource', { source: 'https://example.com' });
  });

  it('creates typed event definitions', () => {
    const approved = event<{ reviewer: string }>('approved');
    expect(approved.name).toBe('approved');
    expect(resolveEventName(approved)).toBe('approved');
    expect(resolveEventName('rejected')).toBe('rejected');
    expect(() => event('')).toThrow(/non-empty/);
  });

  it('resolves named program functions', async () => {
    async function researchAgent() {
      return { ok: true };
    }

    expect(resolveProgramId(researchAgent)).toBe('researchAgent');
    expect(resolveProgramId('analyzeSource')).toBe('analyzeSource');
    expect(() => resolveProgramId('')).toThrow(/non-empty/);
    expect(() => resolveProgramId(async () => undefined)).toThrow(/anonymous/);
  });

  it('passes config through defineConfig', () => {
    const config = defineConfig({
      programs: './src/programs/**/*.ts',
      runtime: { port: 3477 },
    });

    expect(config.programs).toBe('./src/programs/**/*.ts');
    expect(config.runtime?.port).toBe(3477);
  });
});
