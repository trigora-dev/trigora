import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';

import { compileProgram } from '../../cli/src/tsAdapter.ts';
import { Pulse, pulseBin, repoRoot } from './host.ts';
import { fromPlain, toPlain } from './value.ts';

const SIZES = [4 * 1024, 64 * 1024, 1024 * 1024];

const SOURCE = `import { effect } from '@trigora/sdk';

export default async function program() {
  await effect('echo', (input) => input);
  return await effect('done', () => 1);
}
`;

function percentile(samples: number[], p: number): number {
  const sorted = samples.slice().sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[index] ?? 0;
}

function report(label: string, bytes: number, samples: number[]): void {
  const kb = bytes >= 1024 * 1024 ? '1 MB' : `${bytes / 1024} KB`;
  console.log(
    `${label.padEnd(16)} ${kb.padStart(6)}  p50 ${percentile(samples, 50).toFixed(3)} ms  p95 ${percentile(samples, 95).toFixed(3)} ms  p99 ${percentile(samples, 99).toFixed(3)} ms  n=${samples.length}`,
  );
}

function echoArtifact(artifactJson: string, bytes: number): string {
  const artifact = JSON.parse(artifactJson) as {
    program: { functions: Array<{ instructions: unknown[]; spans: unknown[] }> };
  };
  const payload = 'x'.repeat(bytes);
  const instructions = [
    { op: 'LoadConst', value: { t: 'string', v: payload } },
    { op: 'LoadConst', value: { t: 'string', v: 'echo' } },
    { op: 'Effect', has_input: true },
    { op: 'Pop' },
    { op: 'LoadConst', value: { t: 'string', v: 'done' } },
    { op: 'Effect' },
    { op: 'Pop' },
    { op: 'LoadConst', value: { t: 'number', v: 1 } },
    { op: 'Return' },
  ];
  const fn = artifact.program.functions[0];
  if (!fn) {
    throw new Error('compiled artifact has no function');
  }
  fn.instructions = instructions;
  fn.spans = instructions.map(() => null);
  return JSON.stringify(artifact);
}

async function timeEncode(bytes: number, rounds: number): Promise<number[]> {
  const text = 'x'.repeat(bytes);
  const samples: number[] = [];
  for (let i = 0; i < rounds; i += 1) {
    const started = performance.now();
    toPlain(fromPlain(text));
    samples.push(performance.now() - started);
  }
  return samples;
}

async function timeSocket(bytes: number, rounds: number): Promise<number[]> {
  const payload = Buffer.from(fromPlain('x'.repeat(bytes)));
  const dir = path.join(repoRoot(), 'target', 'pulse', `bench${process.pid}`);
  fs.mkdirSync(dir, { recursive: true });
  const sock = path.join(dir, 'echo.sock');
  fs.rmSync(sock, { force: true });
  const server = net.createServer((connection) => {
    let pending = Buffer.alloc(0);
    connection.on('data', (chunk: Buffer) => {
      pending = Buffer.concat([pending, chunk]);
      while (pending.length >= 4) {
        const length = pending.readUInt32BE(0);
        if (pending.length < 4 + length) {
          return;
        }
        const frame = pending.subarray(0, 4 + length);
        pending = pending.subarray(4 + length);
        connection.write(frame);
      }
    });
  });
  await new Promise<void>((resolve) => server.listen(sock, () => resolve()));
  const client = net.createConnection(sock);
  await new Promise<void>((resolve, reject) => {
    client.once('connect', () => resolve());
    client.once('error', reject);
  });
  const samples: number[] = [];
  let incoming = Buffer.alloc(0);
  let waiting: (() => void) | undefined;
  client.on('data', (chunk: Buffer) => {
    incoming = Buffer.concat([incoming, chunk]);
    waiting?.();
  });
  const header = Buffer.alloc(4);
  header.writeUInt32BE(payload.length, 0);
  const frame = Buffer.concat([header, payload]);
  for (let i = 0; i < rounds; i += 1) {
    const started = performance.now();
    client.write(frame);
    const need = frame.length;
    while (incoming.length < need) {
      await new Promise<void>((resolve) => {
        waiting = resolve;
      });
    }
    incoming = incoming.subarray(need);
    samples.push(performance.now() - started);
  }
  client.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
  return samples;
}

async function timeDispatch(bytes: number, rounds: number): Promise<number[]> {
  const compiled = compileProgram(SOURCE, 'src/program.ts', 'program');
  let resolveDone: (() => void) | undefined;
  const pulse = await Pulse.open({
    source: SOURCE,
    bin: pulseBin(),
    artifactJson: echoArtifact(compiled.artifactJson, bytes),
    onEffect: (key) => {
      if (key === 'done') {
        resolveDone?.();
      }
    },
  });
  const samples: number[] = [];
  try {
    for (let i = 0; i < rounds; i += 1) {
      const done = new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('dispatch timed out')), 10_000);
        resolveDone = () => {
          clearTimeout(timer);
          resolve();
        };
      });
      const started = performance.now();
      await pulse.start();
      await done;
      samples.push(performance.now() - started);
    }
  } finally {
    await pulse.close();
  }
  return samples;
}

function releaseBin(): string {
  if (process.env.TRIGORA_PULSE_BIN) {
    return process.env.TRIGORA_PULSE_BIN;
  }
  const bin = path.join(repoRoot(), 'target/release/trigora-pulse');
  if (!fs.existsSync(bin)) {
    execFileSync('cargo', ['build', '--release', '-p', 'trigora-pulse'], {
      cwd: repoRoot(),
      stdio: 'inherit',
      env: { ...process.env, CARGO_TARGET_DIR: path.join(repoRoot(), 'target') },
    });
  }
  process.env.TRIGORA_PULSE_BIN = bin;
  return bin;
}

const rounds = Number(process.env.PULSE_BENCH_ROUNDS ?? '40');
releaseBin();
console.log('pulse adapter boundary. Latencies are measurements, not a gate.');
for (const bytes of SIZES) {
  const encodeRounds = bytes >= 1024 * 1024 ? Math.min(rounds, 20) : rounds;
  report('encode/decode', bytes, await timeEncode(bytes, encodeRounds));
  report('socket', bytes, await timeSocket(bytes, encodeRounds));
  report('dispatch', bytes, await timeDispatch(bytes, Math.min(encodeRounds, 20)));
}
