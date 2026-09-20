import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createClient } from '@trigora/client';

import { cancelCommand } from './cancel';
import { inspectExecutionCommand, listExecutionsCommand } from './executions';
import { listProgramsCommand } from './programs';
import { sendCommand } from './send';
import { startCommand } from './start';
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
const originalEnv = { ...process.env };
const originalConsoleLog = console.log;

afterEach(async () => {
  console.log = originalConsoleLog;
  process.env = { ...originalEnv };
  await Promise.all(servers.splice(0).map((server) => server.close()));
  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
  vi.restoreAllMocks();
});

beforeEach(() => {
  console.log = vi.fn();
  delete process.env.TRIGORA_TOKEN;
});

async function startRuntime() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-cli-'));
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
  process.env.TRIGORA_RUNTIME_URL = server.url;
  return { engine, server };
}

describe('local Program/Execution commands', () => {
  it('lists programs, starts, inspects, sends, and completes', async () => {
    await startRuntime();

    const client = createClient({ url: process.env.TRIGORA_RUNTIME_URL });
    await expect(client.listProjects()).resolves.toMatchObject({
      projects: [expect.objectContaining({ slug: 'default' })],
    });

    await listProgramsCommand();
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/approval/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/javascript/));

    await startCommand({ programId: 'approval' });
    const [execution] = (await client.executions()).executions;
    expect(execution?.status).toBe('waiting');

    await listExecutionsCommand();
    await inspectExecutionCommand(execution!.id);
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/event:approved/));

    await sendCommand({
      executionId: execution!.id,
      event: 'approved',
      payload: '"ok"',
    });

    await expect(client.getResult(execution!.id)).resolves.toMatchObject({
      status: 'completed',
      result: { result: 42, approval: 'ok' },
    });
  });

  it('cancels a waiting execution', async () => {
    await startRuntime();
    await startCommand({ programId: 'approval' });
    const client = createClient({ url: process.env.TRIGORA_RUNTIME_URL });
    const [execution] = (await client.executions()).executions;

    await cancelCommand(execution!.id);

    await expect(client.getResult(execution!.id)).resolves.toMatchObject({
      status: 'cancelled',
    });
  });
});
