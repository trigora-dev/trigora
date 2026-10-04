import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { ENGINE_FORMAT_VERSION } from '@trigora/contracts';
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
const here = path.dirname(fileURLToPath(import.meta.url));
const sibling = (...parts: string[]) => path.resolve(here, '../../../../../../', ...parts);
const typescriptClient = sibling('trigora-typescript/packages/client/src/client.ts');
const pythonParity = sibling('trigora-python/trigora-client/tests/parity_local.py');
const pythonSrc = sibling('trigora-python/trigora-client/src');
const rustManifest = sibling('trigora-rust/Cargo.toml');

type ParityClient = {
  listProjects(): Promise<{ projects: Array<{ slug: string }> }>;
  createProject(body: { name: string }): Promise<{ project: { name: string } }>;
  deployProgram(body: {
    name: string;
    artifact: {
      hash: string;
      blob: string;
      engineFormatVersion: number;
      languageSemanticsVersion: string;
      frontendId: string;
      frontendVersion: string;
    };
    effectBundle: { language: 'typescript'; files: unknown[] };
  }): Promise<{ version: { artifactHash: string } }>;
  listPrograms(): Promise<{ programs: Array<{ name: string }> }>;
  getProgram(programId: string): Promise<{
    program: { currentVersion?: { artifactHash: string } };
  }>;
  listProgramVersions(programId: string): Promise<{ versions: Array<{ artifactHash: string }> }>;
  start(
    programId: string,
    input: Record<string, never>,
  ): Promise<{
    send(event: string, payload: unknown): Promise<void>;
    result(): Promise<unknown>;
    cancel(): Promise<void>;
  }>;
};

afterEach(async () => {
  await Promise.all(servers.splice(0).map((server) => server.close()));
  await Promise.all(dirs.splice(0).map((dir) => fs.rm(dir, { recursive: true, force: true })));
});

describe('local client parity', () => {
  it.skipIf(!existsSync(typescriptClient))(
    'runs the contract sequence from the TypeScript client',
    async () => {
      const { url, body } = await startServer();
      const client = await loadClient(url);
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
      expect(versions.versions.map((version) => version.artifactHash)).toContain(
        body.artifact.hash,
      );
      const first = await client.start('wait', {});
      await first.send('approved', { ok: true });
      await expect(first.result()).resolves.toEqual({ ok: true, approval: { ok: true } });
      const second = await client.start('wait', {});
      await second.cancel();
      await expect(second.result()).rejects.toThrow(/cancelled/);
    },
  );

  it.skipIf(!existsSync(pythonParity) || !existsSync(rustManifest))(
    'runs the same sequence from the Python and Rust clients',
    async () => {
      const python = await startServer();
      await runPython(python.url, python.body);
      const rust = await startServer();
      await runRust(rust.url, rust.body);
    },
    120_000,
  );
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

async function loadClient(url: string): Promise<ParityClient> {
  const imported = (await import(pathToFileURL(typescriptClient).href)) as {
    createClient: (options: { url: string }) => ParityClient;
  };
  return imported.createClient({ url });
}

function runPython(url: string, body: unknown): Promise<void> {
  return runChild('python3', [pythonParity], {
    PYTHONPATH: pythonSrc,
    TRIGORA_PARITY_URL: url,
    TRIGORA_PARITY_BODY: JSON.stringify(body),
  });
}

function runRust(url: string, body: unknown): Promise<void> {
  return runChild(
    'cargo',
    [
      'test',
      '--manifest-path',
      rustManifest,
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
