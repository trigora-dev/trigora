import { spawn, type ChildProcess } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { compileProgram, runHandler } from '../../cli/src/tsAdapter.ts';

import { fromPlain, toPlain } from './value.ts';

type Frame = {
  type: string;
  bearer?: string;
  artifactHash?: string;
  keys?: string[];
  executionId?: string;
  attemptId?: string;
  key?: string;
  input?: string;
  value?: string;
  message?: string;
};

type Waiter = (frame: Frame) => boolean;

const PROGRAM = `import { effect } from '@trigora/sdk';

export default async function program() {
  return await effect('generate', () => 42);
}
`;

export type PulseOptions = {
  source: string;
  bin: string;
  filename?: string;
  programId?: string;
  dir?: string;
  /** Replaces the compiled artifact bytes. The envelope hash is what the host loads. */
  artifactJson?: string;
  /** Called before the synchronous handler. Used to observe dispatch. */
  onEffect?: (key: string, executionId: string, attemptId: string) => void;
};

/**
 * Effect callbacks are synchronous, matching the CLI extractor.
 * `runHandler` runs on this process's event loop, so one slow callback blocks
 * every other execution using this adapter. The Rust host does not share that loop.
 * Async JavaScript callbacks are the follow-up after Phase 2.
 */
export class Pulse {
  private buffer = Buffer.alloc(0);
  private waiters: Waiter[] = [];
  private socket: net.Socket;
  private readonly child: ChildProcess | undefined;
  private readonly dir: string;
  private readonly handlers: Record<string, (input?: unknown) => unknown>;
  private readonly artifactHash: string;
  private readonly onEffect?: PulseOptions['onEffect'];

  private constructor(
    socket: net.Socket,
    child: ChildProcess | undefined,
    dir: string,
    handlers: Record<string, (input?: unknown) => unknown>,
    artifactHash: string,
    onEffect?: PulseOptions['onEffect'],
  ) {
    this.socket = socket;
    this.child = child;
    this.dir = dir;
    this.handlers = handlers;
    this.artifactHash = artifactHash;
    this.onEffect = onEffect;
    this.listen(socket);
  }

  static async open(options: PulseOptions): Promise<Pulse> {
    const compiled = compileProgram(
      options.source,
      options.filename ?? 'src/program.ts',
      options.programId ?? 'program',
    );
    const artifactJson = options.artifactJson ?? compiled.artifactJson;
    const artifact = JSON.parse(artifactJson) as {
      envelope?: { artifact_hash?: string };
    };
    const artifactHash = artifact.envelope?.artifact_hash ?? compiled.artifactHash;
    const dir = options.dir ?? socketDir();
    fs.mkdirSync(dir, { recursive: true });
    const artifactPath = path.join(dir, 'artifact.json');
    fs.writeFileSync(artifactPath, artifactJson);
    const child = spawn(options.bin, [dir, artifactPath], {
      stdio: ['ignore', 'ignore', 'pipe'],
    });
    let stderr = '';
    child.on('error', (error) => {
      stderr += error.message;
    });
    child.stderr?.on('data', (chunk: Buffer) => {
      stderr += chunk.toString('utf8');
    });
    await waitForFile(path.join(dir, 'bearer'), child, () => stderr);
    const pulse = await Pulse.connect(
      dir,
      compiled.handlers,
      artifactHash,
      Object.keys(compiled.handlers),
      child,
      options.onEffect,
    );
    return pulse;
  }

  /** A second connection. A different artifact hash must not receive this host's effects. */
  async attachPeer(
    artifactHash: string,
    keys: string[],
    onEffect?: (key: string) => void,
  ): Promise<{ close: () => void; effects: () => string[] }> {
    const seen: string[] = [];
    const peer = await Pulse.connect(this.dir, {}, artifactHash, keys, undefined, (key) => {
      seen.push(key);
      onEffect?.(key);
    });
    return {
      effects: () => seen.slice(),
      close: () => peer.socket.destroy(),
    };
  }

  /**
   * Returns after the host has registered `executionId` with the supervisor.
   * That ack is not a checkpoint and does not survive the host process.
   */
  async start(executionId: string = randomUUID()): Promise<string> {
    this.send({ type: 'start', executionId });
    const frame = await this.waitFor(
      (candidate) =>
        (candidate.type === 'started' || candidate.type === 'startError') &&
        candidate.executionId === executionId,
    );
    if (frame.type === 'startError') {
      throw new Error(frame.message ?? 'start rejected');
    }
    return executionId;
  }

  async disconnectAdapter(): Promise<void> {
    this.socket.removeAllListeners('data');
    this.socket.destroy();
    await new Promise((resolve) => setTimeout(resolve, 20));
  }

  async reconnect(): Promise<void> {
    const socket = await connectSocket(this.dir);
    this.listen(socket);
    await this.authenticate(this.artifactHash, Object.keys(this.handlers));
  }

  async close(): Promise<void> {
    this.socket.destroy();
    this.child?.kill();
    await new Promise<void>((resolve) => {
      if (!this.child || this.child.exitCode !== null) {
        resolve();
        return;
      }
      this.child.once('exit', () => resolve());
    });
  }

  directory(): string {
    return this.dir;
  }

  private static async connect(
    dir: string,
    handlers: Record<string, (input?: unknown) => unknown>,
    artifactHash: string,
    keys: string[],
    child: ChildProcess | undefined,
    onEffect?: PulseOptions['onEffect'],
  ): Promise<Pulse> {
    const socket = await connectSocket(dir);
    const pulse = new Pulse(socket, child, dir, handlers, artifactHash, onEffect);
    await pulse.authenticate(artifactHash, keys);
    return pulse;
  }

  private listen(socket: net.Socket): void {
    this.socket.removeAllListeners('data');
    this.socket = socket;
    this.buffer = Buffer.alloc(0);
    socket.on('data', (chunk: Buffer) => this.onData(chunk));
    socket.on('error', () => undefined);
  }

  private async authenticate(artifactHash: string, keys: string[]): Promise<void> {
    const bearer = fs.readFileSync(path.join(this.dir, 'bearer'), 'utf8');
    this.send({ type: 'auth', bearer });
    const auth = await this.waitFor(
      (frame) => frame.type === 'authOk' || frame.type === 'authError',
    );
    if (auth.type === 'authError') {
      throw new Error(auth.message ?? 'authentication failed');
    }
    this.send({ type: 'register', artifactHash, keys });
    const registered = await this.waitFor((frame) => frame.type === 'registered');
    if (registered.type !== 'registered') {
      throw new Error('registration failed');
    }
  }

  private send(frame: Frame): void {
    const body = Buffer.from(JSON.stringify(frame));
    const header = Buffer.alloc(4);
    header.writeUInt32BE(body.length, 0);
    this.socket.write(Buffer.concat([header, body]));
  }

  private waitFor(match: (frame: Frame) => boolean): Promise<Frame> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.waiters = this.waiters.filter((waiter) => waiter !== accept);
        reject(new Error('pulse host timed out'));
      }, 5_000);
      const accept = (frame: Frame): boolean => {
        if (!match(frame)) {
          return false;
        }
        clearTimeout(timer);
        resolve(frame);
        return true;
      };
      this.waiters.push(accept);
    });
  }

  private onData(chunk: Buffer): void {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    const frames: Frame[] = [];
    while (this.buffer.length >= 4) {
      const length = this.buffer.readUInt32BE(0);
      if (this.buffer.length < 4 + length) {
        break;
      }
      const body = this.buffer.subarray(4, 4 + length).toString('utf8');
      this.buffer = this.buffer.subarray(4 + length);
      frames.push(JSON.parse(body) as Frame);
    }
    const effects: Frame[] = [];
    for (const frame of frames) {
      const index = this.waiters.findIndex((waiter) => waiter(frame));
      if (index >= 0) {
        this.waiters.splice(index, 1);
        continue;
      }
      if (frame.type === 'effect') {
        effects.push(frame);
      }
    }
    if (effects.length > 0) {
      setImmediate(() => {
        for (const frame of effects) {
          this.handleEffect(frame);
        }
      });
    }
  }

  private handleEffect(frame: Frame): void {
    const executionId = frame.executionId ?? '';
    const attemptId = frame.attemptId ?? '';
    const key = frame.key ?? '';
    this.onEffect?.(key, executionId, attemptId);
    if (!this.socket.writable) {
      return;
    }
    const handler = this.handlers[key];
    if (!handler) {
      this.send({
        type: 'effectError',
        executionId,
        attemptId,
        message: `no handler for \`${key}\``,
      });
      return;
    }
    try {
      const value = runHandler(handler, toPlain(frame.input ?? ''));
      if (!this.socket.writable) {
        return;
      }
      this.send({
        type: 'effectResult',
        executionId,
        attemptId,
        value: fromPlain(value),
      });
    } catch (error) {
      this.send({
        type: 'effectError',
        executionId,
        attemptId,
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }
}

export function repoRoot(): string {
  return path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
}

export function pulseBin(): string {
  return process.env.TRIGORA_PULSE_BIN ?? path.join(repoRoot(), 'target/debug/trigora-pulse');
}

export const sampleProgram = PROGRAM;

function socketDir(): string {
  const root = path.join(repoRoot(), 'target', 'pulse');
  fs.mkdirSync(root, { recursive: true });
  const dir = path.join(root, `n${process.pid}${Date.now().toString(36)}`);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

async function connectSocket(dir: string): Promise<net.Socket> {
  await waitForFile(path.join(dir, 'pulse.sock'));
  const socket = net.createConnection(path.join(dir, 'pulse.sock'));
  await new Promise<void>((resolve, reject) => {
    socket.once('connect', () => resolve());
    socket.once('error', reject);
  });
  return socket;
}

async function waitForFile(
  file: string,
  child?: ChildProcess,
  stderr?: () => string,
): Promise<void> {
  for (let attempt = 0; attempt < 250; attempt += 1) {
    const message = stderr?.() ?? '';
    if (message.includes('ENOENT')) {
      throw new Error(`pulse host failed to start: ${message}`);
    }
    if (child?.exitCode !== null && child?.exitCode !== undefined) {
      throw new Error(`pulse host exited ${child.exitCode}: ${message}`);
    }
    if (fs.existsSync(file)) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  throw new Error(`timed out waiting for ${file}: ${stderr?.() ?? ''}`);
}
