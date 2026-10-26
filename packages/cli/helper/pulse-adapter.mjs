import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const dir = process.argv[2];
const manifestPath = process.argv[3];
if (!dir || !manifestPath) {
  console.error('usage: node pulse-adapter.mjs <directory> <manifest.json>');
  process.exit(2);
}

const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const programs = Array.isArray(manifest.programs) ? manifest.programs : [];

function send(socket, frame) {
  const body = Buffer.from(JSON.stringify(frame));
  const header = Buffer.alloc(4);
  header.writeUInt32BE(body.length, 0);
  socket.write(Buffer.concat([header, body]));
}

function toPlain(encoded) {
  if (!encoded) {
    return {};
  }
  return decode(JSON.parse(encoded));
}

function fromPlain(value) {
  return JSON.stringify(encode(value));
}

function decode(value) {
  switch (value.t) {
    case 'undefined':
      return undefined;
    case 'null':
      return null;
    case 'bool':
    case 'number':
    case 'string':
      return value.v;
    case 'object': {
      const object = {};
      for (const key of Object.keys(value.v ?? {})) {
        object[key] = decode(value.v[key]);
      }
      return object;
    }
    case 'array':
      return (value.v ?? []).map((item) => decode(item));
    default:
      throw new Error(`unsupported TCC value \`${value.t}\``);
  }
}

function encode(value) {
  if (value === undefined) {
    return { t: 'undefined' };
  }
  if (value === null) {
    return { t: 'null' };
  }
  if (typeof value === 'boolean') {
    return { t: 'bool', v: value };
  }
  if (typeof value === 'number') {
    return { t: 'number', v: value };
  }
  if (typeof value === 'string') {
    return { t: 'string', v: value };
  }
  if (Array.isArray(value)) {
    return { t: 'array', v: value.map((item) => encode(item)) };
  }
  if (typeof value === 'object') {
    const fields = {};
    for (const [key, item] of Object.entries(value)) {
      fields[key] = encode(item);
    }
    return { t: 'object', v: fields };
  }
  throw new Error(`cannot encode ${typeof value} as a TCC value`);
}

function waitForSocket() {
  const socketPath = path.join(dir, 'pulse.sock');
  const started = Date.now();
  return new Promise((resolve, reject) => {
    const timer = setInterval(() => {
      if (fs.existsSync(socketPath)) {
        clearInterval(timer);
        resolve(socketPath);
        return;
      }
      if (Date.now() - started > 10_000) {
        clearInterval(timer);
        reject(new Error('Pulse socket did not appear'));
      }
    }, 20);
  });
}

function connect(socketPath) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(socketPath);
    socket.once('connect', () => resolve(socket));
    socket.once('error', reject);
  });
}

async function register(program) {
  const loaded = await import(pathToFileURL(program.module).href);
  const handlers = loaded.handlers ?? {};
  const bearer = fs.readFileSync(path.join(dir, 'bearer'), 'utf8');
  const artifact = fs.readFileSync(program.artifact, 'utf8');
  const socketPath = await waitForSocket();
  const socket = await connect(socketPath);
  let buffer = Buffer.alloc(0);
  const waiters = [];
  const pending = [];

  function take(frame) {
    const index = waiters.findIndex((waiter) => waiter(frame));
    if (index >= 0) {
      waiters.splice(index, 1);
      return;
    }
    if (frame.type === 'effect') {
      void handleEffect(frame);
      return;
    }
    pending.push(frame);
  }

  function onData(chunk) {
    buffer = Buffer.concat([buffer, chunk]);
    while (buffer.length >= 4) {
      const length = buffer.readUInt32BE(0);
      if (buffer.length < 4 + length) {
        return;
      }
      const frame = JSON.parse(buffer.subarray(4, 4 + length).toString('utf8'));
      buffer = buffer.subarray(4 + length);
      take(frame);
    }
  }

  function waitFor(match) {
    const existing = pending.findIndex((frame) => match(frame));
    if (existing >= 0) {
      return Promise.resolve(pending.splice(existing, 1)[0]);
    }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error('pulse host timed out'));
      }, 10_000);
      waiters.push((frame) => {
        if (!match(frame)) {
          return false;
        }
        clearTimeout(timer);
        resolve(frame);
        return true;
      });
    });
  }

  async function handleEffect(frame) {
    const executionId = frame.executionId ?? '';
    const attemptId = frame.attemptId ?? '';
    const key = frame.key ?? '';
    const handler = handlers[key];
    if (typeof handler !== 'function') {
      send(socket, {
        type: 'effectError',
        executionId,
        attemptId,
        message: `no handler for \`${key}\``,
      });
      return;
    }
    try {
      const value = await handler(toPlain(frame.input ?? ''));
      send(socket, {
        type: 'effectResult',
        executionId,
        attemptId,
        value: fromPlain(value === undefined ? null : value),
      });
    } catch (error) {
      send(socket, {
        type: 'effectError',
        executionId,
        attemptId,
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }

  socket.on('data', onData);
  socket.on('error', (error) => {
    console.error(error.message);
  });
  send(socket, { type: 'auth', bearer });
  const auth = await waitFor((frame) => frame.type === 'authOk' || frame.type === 'authError');
  if (auth.type === 'authError') {
    throw new Error(auth.message ?? 'authentication failed');
  }
  send(socket, {
    type: 'register',
    artifactHash: program.hash,
    keys: program.keys,
    artifact,
  });
  const registered = await waitFor(
    (frame) => frame.type === 'registered' || frame.type === 'registerError',
  );
  if (registered.type === 'registerError') {
    throw new Error(registered.message ?? 'registration failed');
  }
}

try {
  for (const program of programs) {
    await register(program);
  }
  fs.writeFileSync(path.join(dir, 'ready.json'), JSON.stringify(manifest));
  console.log('ready');
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
