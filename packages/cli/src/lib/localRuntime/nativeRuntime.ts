import { spawn, type ChildProcess } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import http from 'node:http';
import type { AddressInfo } from 'node:net';

import { ENGINE_FORMAT_VERSION, type JsonValue } from '@trigora/contracts';

import type { DiscoveredProgram } from './discoverPrograms';
import { resolveLocalBinary } from './localBinary';

export type NativeRuntimeEvent = {
  type: string;
  name?: string;
  execution: {
    id: string;
    programId: string;
    status: string;
    wait?: { type: string; event?: string; wakeAt?: string; executionId?: string };
  };
};

export type NativeRuntime = {
  url: string;
  port: number;
  restored: number;
  close: () => Promise<void>;
  replacePrograms: (programs: DiscoveredProgram[]) => Promise<void>;
};

type ReadyMessage = {
  type: 'ready';
  port: number;
  controlPort: number;
  controlToken: string;
  restored?: number;
};

export async function startNativeRuntime(options: {
  dbPath: string;
  host: string;
  port: number;
  programs: DiscoveredProgram[];
  onEvent?: (event: NativeRuntimeEvent) => void;
}): Promise<NativeRuntime> {
  const secret = randomUUID();
  const programs = new Map(options.programs.map((program) => [program.id, program]));
  const effectServer = http.createServer((req, res) => {
    void handleEffect(req, res, secret, programs);
  });
  await listen(effectServer, '127.0.0.1', 0);
  const effectPort = (effectServer.address() as AddressInfo).port;

  const binary = resolveLocalBinary();
  const child = spawn(
    binary,
    [
      '--db',
      options.dbPath,
      '--host',
      options.host,
      '--port',
      String(options.port),
      '--effect-url',
      `http://127.0.0.1:${effectPort}/effects`,
      '--effect-secret',
      secret,
    ],
    { stdio: ['ignore', 'pipe', 'pipe'] },
  );
  const stderr: Buffer[] = [];
  child.stderr?.on('data', (chunk: Buffer) => {
    stderr.push(chunk);
  });

  let controlPort = 0;
  let controlToken = '';
  const ready = await waitForReady(child, stderr, options.onEvent);
  controlPort = ready.controlPort;
  controlToken = ready.controlToken;

  const runtime: NativeRuntime = {
    url: `http://${options.host}:${ready.port}`,
    port: ready.port,
    restored: ready.restored ?? 0,
    replacePrograms: async (next) => {
      programs.clear();
      for (const program of next) {
        programs.set(program.id, program);
      }
      await pushPrograms(controlPort, controlToken, next);
    },
    close: () => closeRuntime(child, effectServer),
  };
  await runtime.replacePrograms(options.programs);
  return runtime;
}

function listen(server: http.Server, host: string, port: number): Promise<void> {
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, host, () => {
      server.off('error', reject);
      resolve();
    });
  });
}

function waitForReady(
  child: ChildProcess,
  stderr: Buffer[],
  onEvent: ((event: NativeRuntimeEvent) => void) | undefined,
): Promise<ReadyMessage> {
  return new Promise((resolve, reject) => {
    let buffer = '';
    let settled = false;
    const fail = (error: Error) => {
      if (settled) {
        return;
      }
      settled = true;
      reject(error);
    };
    const timer = setTimeout(() => {
      fail(
        new Error(`trigora-local did not become ready.\n${Buffer.concat(stderr).toString('utf8')}`),
      );
    }, 15_000);

    child.once('error', (error) => {
      clearTimeout(timer);
      fail(error);
    });
    child.once('exit', (code) => {
      clearTimeout(timer);
      fail(
        new Error(
          `trigora-local exited before it was ready (${code ?? 'signal'}).\n${Buffer.concat(stderr).toString('utf8')}`,
        ),
      );
    });
    child.stdout?.on('data', (chunk: Buffer) => {
      buffer += chunk.toString('utf8');
      let newline = buffer.indexOf('\n');
      while (newline !== -1) {
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        newline = buffer.indexOf('\n');
        if (!line) {
          continue;
        }
        let event: { type?: string };
        try {
          event = JSON.parse(line) as { type?: string };
        } catch {
          continue;
        }
        if (event.type === 'ready' && !settled) {
          settled = true;
          clearTimeout(timer);
          resolve(event as ReadyMessage);
          continue;
        }
        if (event.type !== 'ready') {
          onEvent?.(event as NativeRuntimeEvent);
        }
      }
    });
  });
}

async function pushPrograms(
  port: number,
  token: string,
  programs: DiscoveredProgram[],
): Promise<void> {
  const response = await fetch(`http://127.0.0.1:${port}/programs`, {
    method: 'POST',
    headers: {
      authorization: `Bearer ${token}`,
      'content-type': 'application/json',
    },
    body: JSON.stringify({
      programs: programs.map((program) => ({
        id: program.id,
        artifactJson: program.artifactJson,
        artifactHash: program.artifactHash,
        language: publicLanguage(program.language),
        frontendId: publicLanguage(program.language),
        frontendVersion: program.compilerVersion,
        languageSemanticsVersion: languageSemantics(program.language),
        engineFormatVersion: ENGINE_FORMAT_VERSION,
      })),
    }),
  });
  if (!response.ok) {
    throw new Error(`Program registration failed (${response.status}). ${await response.text()}`);
  }
}

function handleEffect(
  req: http.IncomingMessage,
  res: http.ServerResponse,
  secret: string,
  programs: Map<string, DiscoveredProgram>,
): Promise<void> {
  return new Promise((resolve) => {
    const chunks: Buffer[] = [];
    req.on('data', (chunk: Buffer) => {
      chunks.push(chunk);
    });
    req.on('end', () => {
      const unauthorized = req.headers.authorization !== `Bearer ${secret}`;
      if (unauthorized || req.method !== 'POST') {
        sendJson(res, 401, { error: 'Effect secret was rejected.' });
        resolve();
        return;
      }
      let body: { programId?: string; key?: string; input?: unknown };
      try {
        body = JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}') as typeof body;
      } catch {
        sendJson(res, 400, { error: 'Request body must be valid JSON.' });
        resolve();
        return;
      }
      const handler = programs.get(body.programId ?? '')?.effects[body.key ?? ''];
      if (!handler) {
        sendJson(res, 200, { error: `No effect handler for \`${body.key ?? ''}\`.` });
        resolve();
        return;
      }
      try {
        const value = handler(body.input ?? {}) as JsonValue;
        sendJson(res, 200, { value: toJsonValue(value) });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        sendJson(res, 200, { error: message });
      }
      resolve();
    });
  });
}

function sendJson(res: http.ServerResponse, status: number, body: unknown): void {
  const payload = JSON.stringify(body);
  res.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'content-length': Buffer.byteLength(payload),
    connection: 'close',
  });
  res.end(payload);
}

function toJsonValue(value: unknown): JsonValue {
  if (value === undefined) {
    return null;
  }
  return JSON.parse(JSON.stringify(value)) as JsonValue;
}

function publicLanguage(language: DiscoveredProgram['language']): 'typescript' | 'python' | 'rust' {
  if (language === 'python' || language === 'rust') {
    return language;
  }
  return 'typescript';
}

function languageSemantics(language: DiscoveredProgram['language']): string {
  if (language === 'python') {
    return 'py.subset.v1';
  }
  if (language === 'rust') {
    return 'rust.subset.v1';
  }
  return 'ts.subset.v1';
}

function closeRuntime(child: ChildProcess, effectServer: http.Server): Promise<void> {
  return new Promise((resolve) => {
    const finish = () => {
      effectServer.close(() => resolve());
    };
    if (child.exitCode !== null || child.signalCode !== null) {
      finish();
      return;
    }
    child.once('exit', finish);
    child.kill();
  });
}
