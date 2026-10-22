import http from 'node:http';

import { Pulse, pulseBin, sampleProgram } from '@trigora/pulse-node';

/**
 * One route starts an execution and returns its id.
 * `started` means the host accepted the id. It is not a checkpoint,
 * and it does not survive if the `trigora-pulse` process dies.
 *
 * Effect callbacks are synchronous. A slow callback blocks this Node process.
 * The Rust host keeps driving other work.
 */
export function createExecutionServer(pulse: { start: () => Promise<string> }): http.Server {
  return http.createServer((req, res) => {
    if (req.method !== 'POST' || req.url !== '/executions') {
      res.writeHead(404);
      res.end();
      return;
    }
    void pulse
      .start()
      .then((executionId) => {
        const body = JSON.stringify({ executionId });
        res.writeHead(200, {
          'content-type': 'application/json',
          'content-length': Buffer.byteLength(body),
        });
        res.end(body);
      })
      .catch((error: unknown) => {
        const message = error instanceof Error ? error.message : 'start failed';
        res.writeHead(500);
        res.end(message);
      });
  });
}

const bin = pulseBin();
if (process.argv[1]?.endsWith('server.ts') || process.argv[1]?.endsWith('server.js')) {
  const pulse = await Pulse.open({ source: sampleProgram, bin });
  const server = createExecutionServer(pulse);
  server.listen(3478, '127.0.0.1', () => {
    console.log('pulse example listening on http://127.0.0.1:3478');
  });
}
