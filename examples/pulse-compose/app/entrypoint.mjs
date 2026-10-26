import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const dir = process.env.PULSE_DIR ?? '/pulse';
const adapter = process.env.TRIGORA_PULSE_ADAPTER;
process.env.TRIGORA_PULSE_EXPECT_VERSION ??= '0.1.0';
process.env.TRIGORA_PULSE_EXPECT_PROTOCOL ??= '1';

const prepared = spawnSync('trigora', ['pulse', 'connect', '--dir', dir, '--prepare'], {
  encoding: 'utf8',
  stdio: ['ignore', 'pipe', 'inherit'],
});
if (prepared.status !== 0) {
  process.exit(prepared.status ?? 1);
}
const manifestPath = prepared.stdout.trim().split('\n').at(-1);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const wanted = process.env.PULSE_PROGRAM || 'program.ts';
manifest.programs = manifest.programs.filter((program) => String(program.file).endsWith(wanted));
if (manifest.programs.length !== 1) {
  console.error(`PULSE_PROGRAM ${wanted} did not match one program`);
  process.exit(1);
}
const selected = path.join(path.dirname(manifestPath), 'selected.json');
fs.writeFileSync(selected, JSON.stringify(manifest));

let registered = false;
const server = http.createServer((request, response) => {
  if (request.url === '/ready' && registered) {
    response.writeHead(200);
    response.end('ready');
    return;
  }
  response.writeHead(503);
  response.end('not ready');
});
server.listen(8080, '127.0.0.1');

const { serve, encodePayload } = await import(pathToFileURL(adapter).href);
await serve({
  dir,
  manifestPath: selected,
  onReady: async (sessions) => {
    registered = true;
    const session = sessions[0];
    const resumeId = process.env.PULSE_RESUME_EXECUTION_ID;
    const startId = process.env.PULSE_EXECUTION_ID;
    if (resumeId) {
      session.send({ type: 'resume', executionId: resumeId });
      const resumed = await session.waitFor(
        (frame) => frame.type === 'resumed' || frame.type === 'resumeError',
      );
      if (resumed.type === 'resumeError') {
        console.log(resumed.message ?? 'resume failed');
        return;
      }
      await release(session, resumeId);
      return;
    }
    if (startId) {
      session.send({ type: 'start', executionId: startId });
      const started = await session.waitFor(
        (frame) => frame.type === 'started' || frame.type === 'startError',
      );
      if (started.type === 'startError') {
        throw new Error(started.message ?? 'start failed');
      }
    }
  },
});

async function release(session, executionId) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    session.send({
      type: 'event',
      executionId,
      name: 'continue',
      payload: encodePayload('ok'),
    });
    const frame = await session.waitFor(
      (item) => item.type === 'eventOk' || item.type === 'eventError',
    );
    if (frame.type === 'eventOk') {
      return;
    }
    if (frame.message !== 'execution is not waiting') {
      throw new Error(frame.message ?? 'event failed');
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error('execution did not resume');
}
