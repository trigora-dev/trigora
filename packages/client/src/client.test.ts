import { afterEach, describe, expect, it, vi } from 'vitest';

import { createClient, TrigoraRuntimeError } from './client';
import { event } from '@trigora/sdk';

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
  vi.restoreAllMocks();
});

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

describe('createClient', () => {
  it('starts a program and returns a typed execution handle', async () => {
    const fetchMock = vi.fn(async (input: string | URL) => {
      const url = String(input);

      if (url.endsWith('/v1/executions') && url.includes('http://127.0.0.1:3477')) {
        return jsonResponse(200, {
          execution: {
            id: 'exec_1',
            programId: 'researchAgent',
            status: 'waiting',
            input: { query: 'durable agents' },
            wait: { type: 'event', event: 'approved' },
            attempt: 1,
            createdAt: '2026-08-31T00:00:00.000Z',
            updatedAt: '2026-08-31T00:00:00.000Z',
          },
        });
      }

      throw new Error(`unexpected fetch ${url}`);
    });
    globalThis.fetch = fetchMock as typeof fetch;

    async function researchAgent(input: { query: string }) {
      void input;
      return { ok: true };
    }

    const client = createClient({ url: 'http://127.0.0.1:3477' });
    const run = await client.start(researchAgent, { query: 'durable agents' });

    expect(run.id).toBe('exec_1');
    expect(fetchMock).toHaveBeenCalledWith(
      'http://127.0.0.1:3477/v1/executions',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          programId: 'researchAgent',
          input: { query: 'durable agents' },
        }),
      }),
    );
  });

  it('polls until a result is available and sends typed events', async () => {
    const approved = event<{ reviewer: string }>('approved');
    let polls = 0;
    globalThis.fetch = vi.fn(async (input: string | URL, init?: RequestInit) => {
      const url = String(input);

      if (url.endsWith('/events')) {
        expect(init?.method).toBe('POST');
        expect(init?.body).toBe(
          JSON.stringify({ name: 'approved', payload: { reviewer: 'Omar' } }),
        );
        return jsonResponse(200, {
          execution: {
            id: 'exec_2',
            programId: 'researchAgent',
            status: 'running',
            input: {},
            attempt: 1,
            createdAt: '2026-08-31T00:00:00.000Z',
            updatedAt: '2026-08-31T00:00:00.000Z',
          },
        });
      }

      if (url.endsWith('/result')) {
        polls += 1;
        if (polls < 3) {
          return jsonResponse(200, {
            result: {
              status: 'waiting',
            },
          });
        }

        return jsonResponse(200, {
          result: {
            status: 'completed',
            result: { reviewer: 'Omar' },
          },
        });
      }

      throw new Error(`unexpected fetch ${url}`);
    }) as typeof fetch;

    const client = createClient({ url: 'http://127.0.0.1:9' });
    const run = client.get<{ reviewer: string }>('exec_2');
    await run.send(approved, { reviewer: 'Omar' });
    await expect(run.result()).resolves.toEqual({ reviewer: 'Omar' });
  });

  it('surfaces runtime errors', async () => {
    globalThis.fetch = vi.fn(async () =>
      jsonResponse(404, {
        error: { code: 'program_not_found', message: 'Program "missing" was not found.' },
      }),
    ) as typeof fetch;

    const client = createClient({ url: 'http://127.0.0.1:9' });
    await expect(client.start('missing', {})).rejects.toBeInstanceOf(TrigoraRuntimeError);
  });

  it('lists executions from the runtime', async () => {
    globalThis.fetch = vi.fn(async (input: string | URL) => {
      const url = String(input);
      if (url.endsWith('/v1/executions')) {
        return jsonResponse(200, {
          executions: [
            {
              id: 'exec_3',
              programId: 'approval',
              status: 'waiting',
              input: {},
              wait: { type: 'event', event: 'approved' },
              attempt: 1,
              createdAt: '2026-08-31T00:00:00.000Z',
              updatedAt: '2026-08-31T00:00:00.000Z',
            },
          ],
        });
      }

      throw new Error(`unexpected fetch ${url}`);
    }) as typeof fetch;

    const client = createClient({ url: 'http://127.0.0.1:9' });
    await expect(client.executions()).resolves.toMatchObject({
      executions: [{ id: 'exec_3', programId: 'approval' }],
    });
  });
});
