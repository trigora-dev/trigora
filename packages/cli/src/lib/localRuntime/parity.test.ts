import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { ENGINE_FORMAT_VERSION } from '@trigora/contracts';
// Vitest loads this TypeScript client source directly.
// @ts-expect-error TS5097 — the CLI tsconfig does not allow .ts import extensions.
import { createClient } from '../../../../../../trigora-typescript/packages/client/src/client.ts';
import { afterEach, describe, expect, it } from 'vitest';

import { compileTypeScriptProgram } from './compiler';
import { startNativeRuntime } from './nativeRuntime';

const WAIT_SOURCE = `import { waitForEvent } from "@trigora/sdk";

export default async function program() {
  const approval = await waitForEvent("approved");
  return { ok: true, approval };
}
`;

const servers: Array<{ close: () => Promise<void> }> = [];
const dirs: string[] = [];

afterEach(async () => {
  await Promise.all(servers.splice(0).map((server) => server.close()));
  await Promise.all(dirs.splice(0).map((dir) => fs.rm(dir, { recursive: true, force: true })));
});

describe('local client parity', () => {
  it('runs the contract sequence from the TypeScript client', async () => {
    const { url, body } = await startServer();
    const client = createClient({ url });
    const projects = await client.listProjects();
    expect(projects.projects.some((project) => project.slug === 'default')).toBe(true);
    const created = await client.createProject({ name: 'parity-typescript' });
    expect(created.project.name).toBe('parity-typescript');
    const deployed = await client.deployProgram(body);
    expect(deployed.version.artifactHash).toBe(body.artifact.hash);
    const listed = await client.listPrograms();
    expect(listed.programs.some((program) => program.name === 'wait')).toBe(true);
    const program = await client.getProgram('wait');
    expect(program.program.currentVersion?.artifactHash).toBe(body.artifact.hash);
    const versions = await client.listProgramVersions('wait');
    expect(versions.versions.map((version) => version.artifactHash)).toContain(body.artifact.hash);
    const first = await client.start('wait', {});
    await first.send('approved', { ok: true });
    await expect(first.result()).resolves.toEqual({ ok: true, approval: { ok: true } });
    const second = await client.start('wait', {});
    await second.cancel();
    await expect(second.result()).rejects.toThrow(/cancelled/);
  });

  it('runs the same sequence from the Python and Rust clients', async () => {
    const python = await startServer();
    await runPython(python.url, python.body);
    const rust = await startServer();
    await runRust(rust.url, rust.body);
  }, 120_000);
});

async function startServer() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-parity-'));
  dirs.push(dir);
  const compiled = compileTypeScriptProgram(WAIT_SOURCE, 'wait.ts');
  const server = await startNativeRuntime({
    dbPath: path.join(dir, 'state.db'),
    host: '127.0.0.1',
    port: 0,
    programs: [],
  });
  servers.push(server);
  return {
    url: server.url,
    body: {
      name: 'wait',
      artifact: {
        hash: compiled.artifactHash,
        blob: compiled.artifactJson,
        engineFormatVersion: ENGINE_FORMAT_VERSION,
        languageSemanticsVersion: '1',
        frontendId: 'typescript',
        frontendVersion: compiled.compilerVersion,
      },
      effectBundle: { language: 'typescript' as const, files: [] },
    },
  };
}

function runPython(url: string, body: unknown): Promise<void> {
  return runChild(
    'python3',
    ['/Users/omarabd/Documents/GitHub/trigora-python/trigora-client/tests/parity_local.py'],
    {
      PYTHONPATH: '/Users/omarabd/Documents/GitHub/trigora-python/trigora-client/src',
      TRIGORA_PARITY_URL: url,
      TRIGORA_PARITY_BODY: JSON.stringify(body),
    },
  );
}

function runRust(url: string, body: unknown): Promise<void> {
  return runChild(
    'cargo',
    [
      'test',
      '--manifest-path',
      '/Users/omarabd/Documents/GitHub/trigora-rust/Cargo.toml',
      '--test',
      'parity',
      'local_v1_parity',
      '--',
      '--exact',
    ],
    {
      TRIGORA_PARITY_URL: url,
      TRIGORA_PARITY_BODY: JSON.stringify(body),
    },
  );
}

function runChild(
  command: string,
  args: string[],
  extraEnv: Record<string, string>,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      env: { ...process.env, ...extraEnv },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let output = '';
    child.stdout?.on('data', (chunk) => {
      output += String(chunk);
    });
    child.stderr?.on('data', (chunk) => {
      output += String(chunk);
    });
    child.on('error', reject);
    child.on('close', (status) => {
      if (status === 0) {
        resolve();
        return;
      }
      reject(new Error(`${command} exited ${status}\n${output}`));
    });
  });
}
