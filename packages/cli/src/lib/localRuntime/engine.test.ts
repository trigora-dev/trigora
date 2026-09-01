import { describe, expect, it } from 'vitest';

import { effect, event, invoke, waitForEvent } from '@trigora/sdk';

import { LocalExecutionEngine, LocalRuntimeError } from './engine';
import type { DiscoveredProgram } from './discoverPrograms';

const approved = event<{ reviewer: string }>('approved');

async function analyzeSource(input: { source: string }) {
  return effect('analyze', () => ({
    source: input.source,
    summary: `notes on ${input.source}`,
  }));
}

async function researchAgent(input: { query: string }) {
  const sources = await effect('search', () => [`${input.query}.example`]);
  const reports = await Promise.all(sources.map((source) => invoke(analyzeSource, { source })));
  const approval = await waitForEvent(approved);

  return effect('publish', () => ({
    reports,
    reviewer: approval.reviewer,
  }));
}

function programs(): DiscoveredProgram[] {
  return [
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
  ];
}

describe('LocalExecutionEngine', () => {
  it('runs effects, child invocations, event waits, and returns a typed result', async () => {
    const engine = new LocalExecutionEngine();
    engine.replacePrograms(programs());

    const started = await engine.start('researchAgent', { query: 'durable agents' });
    expect(started.status).toBe('waiting');
    expect(started.wait).toEqual({ type: 'event', eventName: 'approved' });

    const resumed = await engine.send(started.id, 'approved', { reviewer: 'Omar' });
    expect(resumed.status).toBe('completed');
    expect(resumed.result).toEqual({
      reports: [{ source: 'durable agents.example', summary: 'notes on durable agents.example' }],
      reviewer: 'Omar',
    });
  });

  it('rejects mismatched events while waiting', async () => {
    const engine = new LocalExecutionEngine();
    engine.replacePrograms(programs());

    const started = await engine.start('researchAgent', { query: 'durable agents' });

    await expect(engine.send(started.id, 'rejected', { reviewer: 'Omar' })).rejects.toMatchObject({
      code: 'event_mismatch',
    });
  });

  it('cancels a waiting execution', async () => {
    const engine = new LocalExecutionEngine();
    engine.replacePrograms(programs());

    const started = await engine.start('researchAgent', { query: 'durable agents' });
    const cancelled = await engine.cancel(started.id);

    expect(cancelled.status).toBe('cancelled');
    await expect(engine.send(started.id, 'approved', { reviewer: 'Omar' })).rejects.toBeInstanceOf(
      LocalRuntimeError,
    );
  });

  it('fails unknown programs', async () => {
    const engine = new LocalExecutionEngine();
    engine.replacePrograms(programs());

    await expect(engine.start('missing', {})).rejects.toMatchObject({
      code: 'program_not_found',
    });
  });
});
