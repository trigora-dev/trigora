import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { compileProgram } from '../../cli/src/tsAdapter.ts';
import { Pulse, pulseBin, repoRoot } from './host.ts';
import { fromPlain, toPlain } from './value.ts';

const require = createRequire(import.meta.url);

type Stats = { p50: number; p95: number; p99: number };

type RustReport = {
  sizes: Array<{ bytes: number; samples: number; roundTripUs: Stats }>;
  checkpoint: string[];
};

const SIZES = [
  { bytes: 4 * 1024, warmup: 20, samples: 200 },
  { bytes: 64 * 1024, warmup: 20, samples: 200 },
  { bytes: 1024 * 1024, warmup: 10, samples: 100 },
];

const SOURCE = `import { effect } from '@trigora/sdk';

export default async function program() {
  return await effect('echo', (input) => input);
}
`;

function percentile(samples: number[], p: number): number {
  if (samples.length === 0) {
    return 0;
  }
  const sorted = samples.slice().sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[index] ?? 0;
}

function stats(samples: number[]): Stats {
  return {
    p50: percentile(samples, 50),
    p95: percentile(samples, 95),
    p99: percentile(samples, 99),
  };
}

function printStats(label: string, samples: number[]): void {
  const row = stats(samples);
  console.log(
    `  ${label.padEnd(16)} p50 ${row.p50.toFixed(1).padStart(10)} µs  p95 ${row.p95.toFixed(1).padStart(10)} µs  p99 ${row.p99.toFixed(1).padStart(10)} µs`,
  );
}

function labelFor(bytes: number): string {
  return bytes >= 1024 * 1024 ? '1 MB' : `${bytes / 1024} KB`;
}

function releaseBin(): string {
  if (process.env.TRIGORA_PULSE_BIN) {
    return process.env.TRIGORA_PULSE_BIN;
  }
  const bin = path.join(repoRoot(), 'target/release/trigora-pulse');
  execFileSync('cargo', ['build', '--release', '-p', 'trigora-pulse', '--offline'], {
    cwd: repoRoot(),
    stdio: 'inherit',
    env: { ...process.env, CARGO_TARGET_DIR: path.join(repoRoot(), 'target') },
  });
  process.env.TRIGORA_PULSE_BIN = bin;
  return bin;
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

function machineHeader(): void {
  const cpu = os.cpus()[0];
  console.log('pulse stage bench');
  console.log(
    `profile release  arch ${process.arch}  cpus ${os.cpus().length}  cpu ${cpu?.model ?? process.arch}`,
  );
  console.log('isolation dedicated process; Rust and Node clocks are not subtracted');
  console.log(
    'peer node-adapter  frames 1 effect + 1 result  same echoed string after TCC conversion, lookup, call, and the event loop',
  );
}

async function measureNode(bytes: number, warmup: number, samples: number, rustRoundTrip: Stats) {
  const compiled = compileProgram(SOURCE, 'src/program.ts', 'program', { allowPromise: true });
  const pulse = await Pulse.open({
    source: SOURCE,
    bin: pulseBin(),
    timings: true,
    effectTimeoutMs: 30_000,
    artifactJson: echoArtifact(compiled.artifactJson, bytes),
  });
  try {
    for (let i = 0; i < warmup; i += 1) {
      await pulse.start(`w-${bytes}-${i}`);
      await waitForSpans(pulse, i + 1);
    }
    const hostBefore = pulse.hostSpans().length;
    const nodeBefore = pulse.nodeSpans.length;
    for (let i = 0; i < samples; i += 1) {
      await pulse.start(`m-${bytes}-${i}`);
      await waitForSpans(pulse, hostBefore + i + 1);
    }
    const host = pulse.hostSpans().slice(hostBefore);
    const node = pulse.nodeSpans.slice(nodeBefore);
    console.log(`node-adapter ${labelFor(bytes)} n=${host.length} nodeSamples=${node.length}`);
    printStats(
      'drive',
      host.map((span) => span.driveUs),
    );
    printStats(
      'encodeInput',
      host.map((span) => span.encodeInputUs),
    );
    printStats(
      'schedule',
      host.map((span) => span.scheduleUs),
    );
    printStats(
      'socketWrite',
      host.map((span) => span.socketWriteUs),
    );
    printStats(
      'socketRead',
      host.map((span) => span.socketReadUs),
    );
    printStats(
      'roundTrip',
      host.map((span) => span.roundTripUs),
    );
    printStats(
      'decodeResult',
      host.map((span) => span.decodeResultUs),
    );
    printStats(
      'journal',
      host.map((span) => span.journalUs),
    );
    printStats(
      'continuation',
      host.map((span) => span.continuationUs),
    );
    printStats(
      'commit',
      host.map((span) => span.commitUs),
    );
    printStats(
      'toPlain',
      node.map((span) => span.toPlainUs),
    );
    printStats(
      'handler',
      node.map((span) => span.handlerUs),
    );
    printStats(
      'fromPlain',
      node.map((span) => span.fromPlainUs),
    );
    const nodeRound = stats(host.map((span) => span.roundTripUs)).p50;
    console.log(
      `  integration      roundTrip p50 ${nodeRound.toFixed(1)} µs minus rust-peer ${rustRoundTrip.p50.toFixed(1)} µs = ${(nodeRound - rustRoundTrip.p50).toFixed(1)} µs (Node value conversion, lookup, call, and event loop; not IPC)`,
    );
    return { host, node, pid: pulse.pid() };
  } finally {
    await pulse.close();
  }
}

async function waitForSpans(pulse: Pulse, count: number): Promise<void> {
  const started = Date.now();
  while (pulse.hostSpans().length < count) {
    if (Date.now() - started > 180_000) {
      throw new Error(`timed out waiting for ${count} host spans, saw ${pulse.hostSpans().length}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
}

async function throughputNode(): Promise<void> {
  const compiled = compileProgram(SOURCE, 'src/program.ts', 'program', { allowPromise: true });
  const pulse = await Pulse.open({
    source: SOURCE,
    bin: pulseBin(),
    timings: true,
    artifactJson: echoArtifact(compiled.artifactJson, 32),
  });
  try {
    const count = 64;
    const started = performance.now();
    for (let i = 0; i < count; i += 1) {
      await pulse.start(`t-${i}`);
    }
    await waitForSpans(pulse, count);
    const elapsed = performance.now() - started;
    const rssKb = pulse.pid()
      ? Number(
          execFileSync('ps', ['-o', 'rss=', '-p', String(pulse.pid())], {
            encoding: 'utf8',
          }).trim(),
        )
      : 0;
    console.log(
      `node throughput ${count} small effects in ${elapsed.toFixed(1)} ms  ${(count / (elapsed / 1000)).toFixed(1)}/s  host rss ${rssKb * 1024} bytes  node heap ${process.memoryUsage().heapUsed} bytes`,
    );
  } finally {
    await pulse.close();
  }
}

function encodeBinary(value: unknown): Buffer {
  if (typeof value === 'string') {
    const body = Buffer.from(value);
    const out = Buffer.alloc(5 + body.length);
    out.writeUInt8(1, 0);
    out.writeUInt32BE(body.length, 1);
    body.copy(out, 5);
    return out;
  }
  const entries = Object.entries(value as Record<string, unknown>);
  const parts: Uint8Array[] = [Buffer.from([2])];
  const count = Buffer.alloc(4);
  count.writeUInt32BE(entries.length, 0);
  parts.push(count);
  for (const [key, item] of entries) {
    parts.push(encodeBinary(key), encodeBinary(item));
  }
  return Buffer.concat(parts);
}

function decodeBinary(buffer: Buffer, offset = 0): { value: unknown; offset: number } {
  const tag = buffer.readUInt8(offset);
  const length = buffer.readUInt32BE(offset + 1);
  if (tag === 1) {
    const start = offset + 5;
    return {
      value: buffer.subarray(start, start + length).toString('utf8'),
      offset: start + length,
    };
  }
  let cursor = offset + 5;
  const object: Record<string, unknown> = {};
  for (let i = 0; i < length; i += 1) {
    const key = decodeBinary(buffer, cursor);
    const item = decodeBinary(buffer, key.offset);
    object[String(key.value)] = item.value;
    cursor = item.offset;
  }
  return { value: object, offset: cursor };
}

function timeSamples(rounds: number, body: () => void): number[] {
  for (let i = 0; i < 20; i += 1) {
    body();
  }
  const samples: number[] = [];
  for (let i = 0; i < rounds; i += 1) {
    const started = performance.now();
    body();
    samples.push((performance.now() - started) * 1000);
  }
  return samples;
}

async function socketEcho(frame: Buffer, rounds: number): Promise<number[]> {
  const dir = path.join(repoRoot(), 'target', 'pulse', `echo${process.pid}`);
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
        const message = pending.subarray(0, 4 + length);
        pending = pending.subarray(4 + length);
        connection.write(message);
      }
    });
  });
  await new Promise<void>((resolve) => server.listen(sock, () => resolve()));
  const client = net.createConnection(sock);
  client.on('error', () => undefined);
  await new Promise<void>((resolve, reject) => {
    client.once('connect', () => resolve());
    client.once('error', reject);
  });
  let incoming = Buffer.alloc(0);
  let waiting: (() => void) | undefined;
  client.on('data', (chunk: Buffer) => {
    incoming = Buffer.concat([incoming, chunk]);
    waiting?.();
  });
  for (let i = 0; i < 20; i += 1) {
    client.write(frame);
    while (incoming.length < frame.length) {
      await new Promise<void>((resolve) => {
        waiting = resolve;
      });
    }
    incoming = incoming.subarray(frame.length);
  }
  const samples: number[] = [];
  for (let i = 0; i < rounds; i += 1) {
    const started = performance.now();
    client.write(frame);
    while (incoming.length < frame.length) {
      await new Promise<void>((resolve) => {
        waiting = resolve;
      });
    }
    incoming = incoming.subarray(frame.length);
    samples.push((performance.now() - started) * 1000);
  }
  client.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
  return samples;
}

function frame(body: Buffer): Buffer {
  const header = Buffer.alloc(4);
  header.writeUInt32BE(body.length, 0);
  return Buffer.concat([header, body]);
}

function loadAddon(): ((input: Buffer) => Buffer) | undefined {
  const include = [
    path.resolve(path.dirname(process.execPath), '../include/node'),
    '/opt/homebrew/include/node',
    '/usr/local/include/node',
  ].find((dir) => fs.existsSync(path.join(dir, 'node_api.h')));
  if (!include) {
    console.log('in-process unavailable: node_api.h was not found');
    return undefined;
  }
  const dir = path.join(repoRoot(), 'target', 'pulse', 'napi');
  fs.mkdirSync(dir, { recursive: true });
  const output = path.join(dir, 'echo.node');
  const source = path.join(path.dirname(fileURLToPath(import.meta.url)), 'echo.c');
  const args = ['-shared', '-fPIC', `-I${include}`, '-o', output, source];
  if (process.platform === 'darwin') {
    args.splice(1, 0, '-undefined', 'dynamic_lookup');
  }
  try {
    execFileSync('cc', args, { stdio: 'inherit' });
  } catch (error) {
    console.log(
      `in-process unavailable: ${error instanceof Error ? error.message : String(error)}`,
    );
    return undefined;
  }
  const addon = require(output) as { echo: (input: Buffer) => Buffer };
  return (input) => addon.echo(input);
}

async function transport(
  bytes: number,
  rounds: number,
  echo: ((input: Buffer) => Buffer) | undefined,
) {
  const text = 'x'.repeat(bytes);
  const nested = { note: 'n', payload: text };
  const tcc = Buffer.from(fromPlain(text));
  const tccObject = Buffer.from(fromPlain(nested));
  const binary = encodeBinary(text);
  const binaryObject = encodeBinary(nested);
  console.log(`transport ${labelFor(bytes)} n=${rounds}`);
  console.log(`  tcc string ${tcc.length} bytes  binary string ${binary.length} bytes`);
  console.log(`  tcc object ${tccObject.length} bytes  binary object ${binaryObject.length} bytes`);
  printStats(
    'tcc string',
    timeSamples(rounds, () => toPlain(fromPlain(text))),
  );
  printStats(
    'binary string',
    timeSamples(rounds, () => decodeBinary(encodeBinary(text))),
  );
  printStats(
    'tcc object',
    timeSamples(rounds, () => toPlain(fromPlain(nested))),
  );
  printStats(
    'binary object',
    timeSamples(rounds, () => decodeBinary(encodeBinary(nested))),
  );
  const jsonFrame = frame(
    Buffer.from(JSON.stringify({ type: 'effect', input: tcc.toString('utf8') })),
  );
  printStats('json frame', await socketEcho(jsonFrame, rounds));
  printStats('raw frame', await socketEcho(frame(tcc), rounds));
  if (echo) {
    printStats(
      'napi echo',
      timeSamples(rounds, () => echo(tcc)),
    );
  }
}

const ONE = `import { effect } from '@trigora/sdk';

export default async function program() {
  return await effect('work', () => 1);
}
`;

async function concurrency(): Promise<void> {
  const pulse = await Pulse.open({ source: ONE, bin: pulseBin(), timings: true });
  const marks: number[] = [];
  let calls = 0;
  pulse.setHandler('work', () => {
    calls += 1;
    marks.push(performance.now());
    if (calls <= 8) {
      return new Promise<number>((resolve) => setTimeout(() => resolve(1), 40));
    }
    return 1;
  });
  try {
    for (let i = 0; i < 16; i += 1) {
      await pulse.start(`a-${i}`);
    }
    await waitForSpans(pulse, 16);
    const origin = marks[0] ?? 0;
    const immediate = marks.slice(8).map((mark) => (mark - origin) * 1000);
    console.log('async concurrency  8 delayed promises beside 8 immediate handlers');
    printStats('immediate entry', immediate);
  } finally {
    await pulse.close();
  }

  const stalled = await Pulse.open({ source: ONE, bin: pulseBin(), timings: true });
  const stalledMarks: number[] = [];
  let stalledCalls = 0;
  stalled.setHandler('work', () => {
    stalledCalls += 1;
    stalledMarks.push(performance.now());
    if (stalledCalls <= 4) {
      const started = performance.now();
      while (performance.now() - started < 40) {
        // Synchronous stall of this Node process.
      }
    }
    return 1;
  });
  try {
    for (let i = 0; i < 8; i += 1) {
      await stalled.start(`s-${i}`);
    }
    await waitForSpans(stalled, 8);
    const origin = stalledMarks[0] ?? 0;
    const later = stalledMarks.slice(4).map((mark) => (mark - origin) * 1000);
    console.log('sync stall  4 blocking callbacks beside 4 later handlers');
    printStats('later entry', later);
  } finally {
    await stalled.close();
  }
}

releaseBin();
machineHeader();
execFileSync('cargo', ['bench', '-p', 'trigora-pulse', '--bench', 'stage', '--offline'], {
  cwd: repoRoot(),
  stdio: 'inherit',
  env: { ...process.env, CARGO_TARGET_DIR: path.join(repoRoot(), 'target') },
});
const rust = JSON.parse(
  fs.readFileSync(path.join(repoRoot(), 'target', 'pulse', 'bench-rust.json'), 'utf8'),
) as RustReport;
await throughputNode();
for (const size of SIZES) {
  const rustSize = rust.sizes.find((row) => row.bytes === size.bytes);
  if (!rustSize) {
    throw new Error(`missing rust sample for ${size.bytes}`);
  }
  await measureNode(size.bytes, size.warmup, size.samples, rustSize.roundTripUs);
}
const addon = loadAddon();
for (const size of SIZES) {
  await transport(size.bytes, size.samples, addon);
}
await concurrency();
