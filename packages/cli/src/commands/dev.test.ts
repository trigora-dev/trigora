import { afterEach, describe, expect, it, vi } from 'vitest';

import { createClient } from '@trigora/client';

import { compileTypeScriptProgram } from '../lib/localRuntime/compiler';
import { LocalExecutionEngine } from '../lib/localRuntime/engine';
import { startLocalRuntimeServer } from '../lib/localRuntime/server';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

const SOURCE = `import { effect, waitForEvent } from "@trigora/sdk";

export default async function approval() {
  const result = await effect("generate", () => 42);
  const approval = await waitForEvent("approved");
  return { result, approval };
}
`;

const servers: Array<{ close: () => Promise<void> }> = [];
const tempDirs: string[] = [];

afterEach(async () => {
  await Promise.all(servers.splice(0).map((server) => server.close()));
  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
  vi.restoreAllMocks();
});

describe('local runtime server', () => {
  it('lets the client start, wait, send, and complete a program', async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-dev-'));
    tempDirs.push(dir);
    const compiled = compileTypeScriptProgram(SOURCE, 'approval.ts');
    const engine = new LocalExecutionEngine(path.join(dir, 'state.db'));
    engine.replacePrograms([
      {
        id: 'approval',
        exportName: 'approval',
        file: 'approval.ts',
        source: SOURCE,
        ...compiled,
      },
    ]);

    const server = await startLocalRuntimeServer({
      artifact: { artifactHash: compiled.artifactHash, compilerVersion: compiled.compilerVersion },
      engine,
      host: '127.0.0.1',
      port: 0,
    });
    servers.push(server);

    const client = createClient({ url: server.url });
    const listed = await client.programs();
    expect(listed.programs.map((program) => program.id)).toEqual(['approval']);

    async function approval() {
      return { result: 42, approval: 'ok' };
    }

    const run = await client.start(approval, {});
    expect(run.id).toMatch(/^exec_local_/);

    const executions = await client.executions();
    expect(executions.executions.map((execution) => execution.id)).toEqual([run.id]);
    await expect(client.getExecution(run.id)).resolves.toMatchObject({
      id: run.id,
      status: 'waiting',
    });

    await run.send('approved', 'ok');
    await expect(run.result()).resolves.toEqual({
      result: 42,
      approval: 'ok',
    });
  });
});
