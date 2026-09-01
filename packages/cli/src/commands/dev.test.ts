import { afterEach, describe, expect, it, vi } from 'vitest';

import { createClient } from '@trigora/client';
import { effect, event, invoke, waitForEvent } from '@trigora/sdk';

import { LocalExecutionEngine } from '../lib/localRuntime/engine';
import { startLocalRuntimeServer } from '../lib/localRuntime/server';

const approved = event<{ reviewer: string }>('approved');

async function analyzeSource(input: { source: string }) {
  return effect(() => ({ source: input.source }));
}

async function researchAgent(input: { query: string }) {
  const sources = await effect('search', () => [input.query]);
  const reports = await Promise.all(sources.map((source) => invoke(analyzeSource, { source })));
  const approval = await waitForEvent(approved);
  return { reports, reviewer: approval.reviewer };
}

const servers: Array<{ close: () => Promise<void> }> = [];

afterEach(async () => {
  await Promise.all(servers.splice(0).map((server) => server.close()));
  vi.restoreAllMocks();
});

describe('local runtime server', () => {
  it('lets the client start, wait, send, and complete a program', async () => {
    const engine = new LocalExecutionEngine();
    engine.replacePrograms([
      {
        id: 'analyzeSource',
        exportName: 'analyzeSource',
        file: 'analyzeSource.ts',
        fn: analyzeSource,
      },
      {
        id: 'researchAgent',
        exportName: 'researchAgent',
        file: 'researchAgent.ts',
        fn: researchAgent,
      },
    ]);

    const server = await startLocalRuntimeServer({
      artifact: { artifactHash: 'test', compilerVersion: 'passthrough-local@0.9.0' },
      engine,
      host: '127.0.0.1',
      port: 0,
    });
    servers.push(server);

    const client = createClient({ url: server.url });
    const listed = await client.programs();
    expect(listed.programs.map((program) => program.id)).toEqual([
      'analyzeSource',
      'researchAgent',
    ]);

    const run = await client.start(researchAgent, { query: 'tcc' });
    expect(run.id).toMatch(/^exec_local_/);

    await run.send(approved, { reviewer: 'Omar' });
    await expect(run.result()).resolves.toEqual({
      reports: [{ source: 'tcc' }],
      reviewer: 'Omar',
    });
  });
});
