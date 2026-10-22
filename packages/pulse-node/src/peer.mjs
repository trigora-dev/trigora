import fs from 'node:fs';
import net from 'node:net';

const dir = process.argv[2];
const artifactHash = process.argv[3];
const keys = process.argv.slice(4);
if (!dir || !artifactHash || keys.length === 0) {
  console.error('usage: node peer.mjs <directory> <artifactHash> <key>...');
  process.exit(2);
}

const bearer = fs.readFileSync(`${dir}/bearer`, 'utf8');
const socket = net.createConnection(`${dir}/pulse.sock`);
socket.on('error', (error) => {
  console.error(error.message);
  process.exit(1);
});

let buffer = Buffer.alloc(0);

function send(frame) {
  const body = Buffer.from(JSON.stringify(frame));
  const header = Buffer.alloc(4);
  header.writeUInt32BE(body.length, 0);
  socket.write(Buffer.concat([header, body]));
}

socket.on('connect', () => {
  send({ type: 'auth', bearer });
});

socket.on('data', (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  while (buffer.length >= 4) {
    const length = buffer.readUInt32BE(0);
    if (buffer.length < 4 + length) {
      return;
    }
    const frame = JSON.parse(buffer.subarray(4, 4 + length).toString('utf8'));
    buffer = buffer.subarray(4 + length);
    if (frame.type === 'authOk') {
      send({ type: 'register', artifactHash, keys });
    } else if (frame.type === 'authError') {
      console.error(frame.message);
      process.exit(1);
    } else if (frame.type === 'effect') {
      send({
        type: 'effectResult',
        executionId: frame.executionId,
        attemptId: frame.attemptId,
        value: JSON.stringify({ t: 'number', v: 1 }),
      });
    }
  }
});
