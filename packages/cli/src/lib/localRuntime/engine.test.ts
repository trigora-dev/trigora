import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { compileTypeScriptProgram } from './compiler';
import { LocalExecutionEngine, LocalRuntimeError } from './engine';
import type { DiscoveredProgram } from './discoverPrograms';

const SOURCE = `import { effect, waitForEvent } from "@trigora/sdk";

export default async function approval() {
  const result = await effect("generate", () => 42);
  const approval = await waitForEvent("approved");
  return { result, approval };
}
`;

const tempDirs: string[] = [];

afterEach(async () => {
  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
});

async function makeEngine(): Promise<{ engine: LocalExecutionEngine; program: DiscoveredProgram }> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-engine-'));
  tempDirs.push(dir);
  const compiled = compileTypeScriptProgram(SOURCE, 'approval.ts');
  const program: DiscoveredProgram = {
    id: 'approval',
    exportName: 'approval',
    file: 'approval.ts',
    source: SOURCE,
    ...compiled,
  };
  const engine = new LocalExecutionEngine(path.join(dir, 'state.db'));
  engine.replacePrograms([program]);
  return { engine, program };
}

describe('LocalExecutionEngine', () => {
  it('runs an effect, waits, resumes, and completes', async () => {
    const { engine } = await makeEngine();
    const started = await engine.start('approval', {});
    expect(started.status).toBe('waiting');
    expect(started.wait).toEqual({ type: 'event', event: 'approved' });

    const resumed = await engine.send(started.id, 'approved', 'ok');
    expect(resumed.status).toBe('completed');
    expect(resumed.result).toEqual({ result: 42, approval: 'ok' });
  });

  it('restores a waiting execution from sqlite after a new process', async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-engine-'));
    tempDirs.push(dir);
    const dbPath = path.join(dir, 'state.db');
    const compiled = compileTypeScriptProgram(SOURCE, 'approval.ts');
    const program: DiscoveredProgram = {
      id: 'approval',
      exportName: 'approval',
      file: 'approval.ts',
      source: SOURCE,
      ...compiled,
    };

    const first = new LocalExecutionEngine(dbPath);
    first.replacePrograms([program]);
    const started = await first.start('approval', {});
    expect(started.status).toBe('waiting');

    const second = new LocalExecutionEngine(dbPath);
    second.replacePrograms([program]);
    const restored = second.restoredWaiting();
    expect(restored).toHaveLength(1);
    expect(restored[0]?.id).toBe(started.id);

    const resumed = await second.send(started.id, 'approved', 'ok');
    expect(resumed.status).toBe('completed');
    expect(resumed.result).toEqual({ result: 42, approval: 'ok' });
  });

  it('rejects mismatched events while waiting', async () => {
    const { engine } = await makeEngine();
    const started = await engine.start('approval', {});

    await expect(engine.send(started.id, 'rejected', 'ok')).rejects.toMatchObject({
      code: 'event_mismatch',
    });
  });

  it('cancels a waiting execution', async () => {
    const { engine } = await makeEngine();
    const started = await engine.start('approval', {});
    const cancelled = await engine.cancel(started.id);

    expect(cancelled.status).toBe('cancelled');
    await expect(engine.send(started.id, 'approved', 'ok')).rejects.toBeInstanceOf(
      LocalRuntimeError,
    );
  });

  it('lists executions from sqlite', async () => {
    const { engine } = await makeEngine();
    const started = await engine.start('approval', {});

    expect(engine.listExecutions().map((execution) => execution.id)).toEqual([started.id]);
  });

  it('fails unknown programs', async () => {
    const { engine } = await makeEngine();
    await expect(engine.start('missing', {})).rejects.toMatchObject({
      code: 'program_not_found',
    });
  });
});
