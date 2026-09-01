import http from 'node:http';
import net from 'node:net';

import type {
  ArtifactIdentity,
  JsonValue,
  RuntimeErrorResponse,
  StartExecutionRequest,
} from '@trigora/contracts';
import { LocalExecutionEngine, LocalRuntimeError } from './engine';
import { toProgramIdentity } from './discoverPrograms';

export type LocalRuntimeServer = {
  close: () => Promise<void>;
  port: number;
  url: string;
};

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error;
}

export async function findAvailablePort(host: string, startPort: number): Promise<number> {
  let port = startPort;

  while (true) {
    const isAvailable = await new Promise<boolean>((resolve, reject) => {
      const probe = net.createServer();

      probe.once('error', (error) => {
        probe.close();

        if (isNodeError(error) && error.code === 'EADDRINUSE') {
          resolve(false);
          return;
        }

        reject(error);
      });

      probe.once('listening', () => {
        probe.close(() => resolve(true));
      });

      probe.listen(port, host);
    });

    if (isAvailable) {
      return port;
    }

    port += 1;
  }
}

async function readJsonBody(req: http.IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];

  return new Promise((resolve, reject) => {
    req.on('data', (chunk) => {
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
    });
    req.on('end', () => {
      const raw = Buffer.concat(chunks).toString('utf-8').trim();
      if (!raw) {
        resolve(undefined);
        return;
      }

      try {
        resolve(JSON.parse(raw) as unknown);
      } catch {
        reject(new LocalRuntimeError('invalid_input', 'Request body must be valid JSON.'));
      }
    });
    req.on('error', reject);
  });
}

function sendJson(res: http.ServerResponse, status: number, body: unknown): void {
  res.statusCode = status;
  res.setHeader('Content-Type', 'application/json; charset=utf-8');
  res.end(JSON.stringify(body));
}

function errorStatus(code: LocalRuntimeError['code']): number {
  if (code === 'program_not_found' || code === 'execution_not_found') {
    return 404;
  }

  if (code === 'invalid_input') {
    return 400;
  }

  return 409;
}

function sendError(res: http.ServerResponse, error: unknown): void {
  if (error instanceof LocalRuntimeError) {
    const body: RuntimeErrorResponse = {
      error: {
        code: error.code,
        message: error.message,
      },
    };
    sendJson(res, errorStatus(error.code), body);
    return;
  }

  const message = error instanceof Error ? error.message : String(error);
  const body: RuntimeErrorResponse = {
    error: {
      code: 'invalid_input',
      message,
    },
  };
  sendJson(res, 500, body);
}

export async function startLocalRuntimeServer(options: {
  artifact: ArtifactIdentity;
  engine: LocalExecutionEngine;
  host: string;
  port: number;
}): Promise<LocalRuntimeServer> {
  const selectedPort = options.port === 0 ? 0 : await findAvailablePort(options.host, options.port);
  const runtime = {
    url: `http://${options.host}:${selectedPort}`,
  };

  const server = http.createServer((req, res) => {
    void (async () => {
      const requestUrl = new URL(req.url ?? '/', runtime.url);
      const method = req.method ?? 'GET';
      const pathName = requestUrl.pathname;

      try {
        if (method === 'GET' && pathName === '/v1/health') {
          sendJson(res, 200, { ok: true });
          return;
        }

        if (method === 'GET' && pathName === '/v1/programs') {
          sendJson(res, 200, {
            artifact: options.artifact,
            programs: options.engine.listPrograms().map(toProgramIdentity),
            runtime: { url: runtime.url },
          });
          return;
        }

        if (method === 'POST' && pathName === '/v1/executions') {
          const body = ((await readJsonBody(req)) ?? {}) as StartExecutionRequest;
          if (!body.programId || typeof body.programId !== 'string') {
            throw new LocalRuntimeError('invalid_input', '`programId` is required.');
          }

          const execution = await options.engine.start(body.programId, body.input ?? {});
          sendJson(res, 201, { execution });
          return;
        }

        const executionMatch = /^\/v1\/executions\/([^/]+)$/.exec(pathName);
        if (method === 'GET' && executionMatch?.[1]) {
          sendJson(res, 200, {
            execution: options.engine.getExecution(decodeURIComponent(executionMatch[1])),
          });
          return;
        }

        const eventMatch = /^\/v1\/executions\/([^/]+)\/events$/.exec(pathName);
        if (method === 'POST' && eventMatch?.[1]) {
          const body = ((await readJsonBody(req)) ?? {}) as { name?: string; payload?: JsonValue };
          if (!body.name || typeof body.name !== 'string') {
            throw new LocalRuntimeError('invalid_input', '`name` is required.');
          }

          const execution = await options.engine.send(
            decodeURIComponent(eventMatch[1]),
            body.name,
            body.payload ?? {},
          );
          sendJson(res, 200, { execution });
          return;
        }

        const cancelMatch = /^\/v1\/executions\/([^/]+)\/cancel$/.exec(pathName);
        if (method === 'POST' && cancelMatch?.[1]) {
          const execution = await options.engine.cancel(decodeURIComponent(cancelMatch[1]));
          sendJson(res, 200, { execution });
          return;
        }

        sendJson(res, 404, {
          error: { code: 'execution_not_found', message: 'Not found.' },
        } satisfies RuntimeErrorResponse);
      } catch (error) {
        sendError(res, error);
      }
    })();
  });

  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(selectedPort, options.host, () => {
      server.off('error', reject);
      resolve();
    });
  });

  const address = server.address();
  const boundPort = typeof address === 'object' && address ? address.port : selectedPort;
  runtime.url = `http://${options.host}:${boundPort}`;

  return {
    port: boundPort,
    url: runtime.url,
    close: () =>
      new Promise((resolve, reject) => {
        server.close((error) => {
          if (error) {
            reject(error);
            return;
          }
          resolve();
        });
      }),
  };
}
