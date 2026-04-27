import http from 'node:http';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';

import type { FlowDefinition, JsonValue } from '@trigora/contracts';
import { colors } from '../lib/colors';
import { createLocalContext } from '../lib/createLocalContext';
import { loadFlowModule } from '../lib/loadFlowModule';
import { triggerCommand } from './trigger';

type DevOptions = {
  filePath: string;
  payloadPath?: string;
};

const DEFAULT_WEBHOOK_PORT = 5252;
const FILE_CHANGE_DEBOUNCE_MS = 100;

type QueueRun<T> = {
  execute: () => Promise<T>;
  reason?: string;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
};

type WatchManager = {
  isShuttingDown: () => boolean;
  startSignalHandling: () => void;
  shutdown: () => void;
  watchFile: (filePath: string, label: string) => void;
};

type DevStartupOptions = {
  endpointUrl?: string;
  flowId: string;
  flowPath: string;
  payloadPath?: string;
  readyMessage: string;
};

class FlowRunError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'FlowRunError';
  }
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error;
}

function getWebhookEventName(body: unknown): string {
  if (typeof body !== 'object' || body === null) {
    return 'webhook';
  }

  if (typeof (body as { type?: unknown }).type === 'string') {
    return (body as { type: string }).type;
  }

  return 'webhook';
}

async function findAvailablePort(startPort: number): Promise<number> {
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

      probe.listen(port, '127.0.0.1');
    });

    if (isAvailable) {
      return port;
    }

    port += 1;
  }
}

async function readJsonRequest(req: http.IncomingMessage): Promise<JsonValue> {
  const chunks: Buffer[] = [];

  return new Promise((resolve, reject) => {
    req.on('data', (chunk) => {
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
    });

    req.on('end', () => {
      const rawBody = Buffer.concat(chunks).toString('utf-8').trim();

      if (rawBody.length === 0) {
        resolve({});
        return;
      }

      try {
        resolve(JSON.parse(rawBody));
      } catch {
        reject(new Error('Invalid JSON body.'));
      }
    });

    req.on('error', reject);
  });
}

async function writeWebhookResponse(
  res: http.ServerResponse<http.IncomingMessage>,
  result: unknown,
): Promise<void> {
  if (result instanceof Response) {
    res.statusCode = result.status;

    result.headers.forEach((value, key) => {
      res.setHeader(key, value);
    });

    res.end(await result.text());
    return;
  }

  if (result === undefined) {
    res.statusCode = 200;
    res.end();
    return;
  }

  if (typeof result === 'string') {
    res.statusCode = 200;
    res.setHeader('Content-Type', 'text/plain; charset=utf-8');
    res.end(result);
    return;
  }

  res.statusCode = 200;
  res.setHeader('Content-Type', 'application/json; charset=utf-8');
  res.end(JSON.stringify(result));
}

function logRerunFailure(devPrefix: string, error: unknown): void {
  console.error(`${devPrefix} ${colors.error('re-run failed')}`);

  if (error instanceof Error) {
    console.error(error.message);
    return;
  }

  console.error(error);
}

function printDevRerunMessage(devPrefix: string, reason: string): void {
  console.log('');
  console.log(`${devPrefix} ${colors.warn(reason)}`);
}

function logReloadFailure(devPrefix: string, error: unknown): void {
  console.error(`${devPrefix} ${colors.error('reload failed')}`);

  if (error instanceof Error) {
    console.error(error.message);
    return;
  }

  console.error(error);
}

function printDevStartup(devPrefix: string, options: DevStartupOptions): void {
  console.log(`${devPrefix} running ${colors.flow(options.flowId)}`);
  console.log('');

  if (options.endpointUrl) {
    console.log(colors.label('Local webhook endpoint:'));
    console.log(colors.flow(options.endpointUrl));
    console.log('');
  }

  console.log(colors.label('Watching flow:'));
  console.log(options.flowPath);

  if (options.payloadPath) {
    console.log('');
    console.log(colors.label('Watching payload:'));
    console.log(options.payloadPath);
  }

  console.log('');
  console.log(colors.success(options.readyMessage));
}

function createWatchManager(options: {
  devPrefix: string;
  onFileChange: (label: string) => Promise<void>;
  onShutdown?: () => void;
}): WatchManager {
  let shuttingDown = false;

  const watchers: fs.FSWatcher[] = [];
  const debounceTimers = new Map<string, NodeJS.Timeout>();
  const lastHandledVersion = new Map<string, number>();

  function getFileVersion(filePath: string): number | undefined {
    try {
      return fs.statSync(filePath).mtimeMs;
    } catch {
      return undefined;
    }
  }

  function shutdown() {
    if (shuttingDown) return;
    shuttingDown = true;

    options.onShutdown?.();

    for (const watcher of watchers) {
      watcher.close();
    }

    for (const timer of debounceTimers.values()) {
      clearTimeout(timer);
    }

    process.off('SIGINT', handleSignals);
    process.off('SIGTERM', handleSignals);

    console.log(`\n${options.devPrefix} stopped`);
    process.exit(0);
  }

  const handleSignals = () => shutdown();

  function startSignalHandling() {
    process.on('SIGINT', handleSignals);
    process.on('SIGTERM', handleSignals);
  }

  function watchFile(filePath: string, label: string) {
    const watcher = fs.watch(filePath, (eventType) => {
      if (eventType !== 'change' || shuttingDown) return;

      const existingTimer = debounceTimers.get(filePath);
      if (existingTimer) {
        clearTimeout(existingTimer);
      }

      const timer = setTimeout(() => {
        debounceTimers.delete(filePath);
        const fileVersion = getFileVersion(filePath);
        const previousVersion = lastHandledVersion.get(filePath);

        if (fileVersion !== undefined) {
          if (previousVersion === fileVersion) {
            return;
          }

          lastHandledVersion.set(filePath, fileVersion);
        }

        void options.onFileChange(label);
      }, FILE_CHANGE_DEBOUNCE_MS);

      debounceTimers.set(filePath, timer);
    });

    watchers.push(watcher);
  }

  return {
    isShuttingDown: () => shuttingDown,
    startSignalHandling,
    shutdown,
    watchFile,
  };
}

async function loadWebhookFlow(filePath: string): Promise<FlowDefinition> {
  const flow = await loadFlowModule(filePath);

  if (flow.trigger?.type !== 'webhook') {
    throw new Error(`Flow "${flow.id}" is no longer a webhook flow. Restart trigora dev.`);
  }

  return flow;
}

async function runWebhookFlow(flow: FlowDefinition, body: JsonValue): Promise<unknown> {
  const ctx = createLocalContext(flow.id);
  const event = {
    id: `evt_local_${Date.now()}`,
    type: 'webhook' as const,
    timestamp: new Date().toISOString(),
    payload: body,
  };

  try {
    return await flow.run(event, ctx);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`${colors.flow(`[${flow.id}]`)} ${colors.error('ERROR')} ${message}`);

    throw new FlowRunError(message);
  }
}

async function runStandardDevMode(options: DevOptions, flowId: string): Promise<void> {
  const relativeFlowPath = path.relative(process.cwd(), options.filePath);
  const relativePayloadPath = options.payloadPath
    ? path.relative(process.cwd(), options.payloadPath)
    : undefined;

  const devPrefix = colors.dev('[dev]');

  let isRunning = false;
  let rerunRequested = false;
  let watchManager!: WatchManager;

  async function run(reason?: string) {
    if (watchManager.isShuttingDown()) return;

    if (isRunning) {
      rerunRequested = true;
      return;
    }

    isRunning = true;

    try {
      if (reason) {
        printDevRerunMessage(devPrefix, reason);
      }

      console.log('');
      await triggerCommand(options);
    } finally {
      isRunning = false;

      if (rerunRequested && !watchManager.isShuttingDown()) {
        rerunRequested = false;
        await run('changes queued → rerunning');
      }
    }
  }

  watchManager = createWatchManager({
    devPrefix,
    onFileChange: async (label) => {
      try {
        await run(`${label} changed → rerunning`);
      } catch (error) {
        logRerunFailure(devPrefix, error);
      }
    },
  });

  watchManager.startSignalHandling();

  printDevStartup(devPrefix, {
    flowId,
    flowPath: relativeFlowPath,
    payloadPath: relativePayloadPath,
    readyMessage: relativePayloadPath
      ? 'Ready. Edit the flow or payload to rerun.'
      : 'Ready. Edit the flow to rerun.',
  });

  await run();

  watchManager.watchFile(options.filePath, 'flow');

  if (options.payloadPath) {
    watchManager.watchFile(options.payloadPath, 'payload');
  }

  await new Promise<void>(() => {});
}

async function runWebhookDevMode(options: DevOptions, flow: FlowDefinition): Promise<void> {
  const relativeFlowPath = path.relative(process.cwd(), options.filePath);
  const devPrefix = colors.dev('[dev]');
  const requestPrefix = colors.dev('[request]');
  const eventPrefix = colors.dev('[event]');

  let isRunning = false;
  let rerunRequested = false;
  let server: http.Server | undefined;
  let watchManager!: WatchManager;
  let currentFlow = flow;

  const queuedRuns: Array<QueueRun<unknown>> = [];

  async function enqueueRun<T>(execute: () => Promise<T>, reason?: string): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      queuedRuns.push({
        execute: execute as () => Promise<unknown>,
        reason,
        resolve: resolve as (value: unknown) => void,
        reject,
      });
      rerunRequested = true;
      void processQueue();
    });
  }

  async function processQueue(): Promise<void> {
    if (watchManager.isShuttingDown() || isRunning) {
      return;
    }

    const nextRun = queuedRuns.shift();

    if (!nextRun) {
      rerunRequested = false;
      return;
    }

    isRunning = true;

    try {
      if (nextRun.reason) {
        printDevRerunMessage(devPrefix, nextRun.reason);
      }

      const result = await nextRun.execute();
      nextRun.resolve(result);
    } catch (error) {
      nextRun.reject(error);
    } finally {
      isRunning = false;
      rerunRequested = queuedRuns.length > 0;

      if (rerunRequested && !watchManager.isShuttingDown()) {
        await processQueue();
      }
    }
  }

  watchManager = createWatchManager({
    devPrefix,
    onShutdown: () => {
      server?.close();
    },
    onFileChange: async (label) => {
      try {
        currentFlow = await loadWebhookFlow(options.filePath);
        printDevRerunMessage(devPrefix, `${label} changed → reloaded`);
      } catch (error) {
        logReloadFailure(devPrefix, error);
      }
    },
  });

  watchManager.startSignalHandling();

  const selectedPort = await findAvailablePort(DEFAULT_WEBHOOK_PORT);

  if (selectedPort !== DEFAULT_WEBHOOK_PORT) {
    console.log(
      `${devPrefix} ${colors.warn(`Port ${DEFAULT_WEBHOOK_PORT} was in use, using ${selectedPort} instead.`)}`,
    );
  }

  server = http.createServer(async (req, res) => {
    const requestPath = new URL(req.url ?? '/', 'http://localhost').pathname;

    if (req.method !== 'POST') {
      res.statusCode = 405;
      res.end('Method Not Allowed');
      return;
    }

    if (requestPath !== '/') {
      res.statusCode = 404;
      res.end('Not Found');
      return;
    }

    let parsedBody: JsonValue;

    try {
      parsedBody = await readJsonRequest(req);
    } catch {
      res.statusCode = 400;
      res.end('Invalid JSON body.');
      return;
    }

    console.log('');
    console.log(`${requestPrefix} ${colors.info(req.method)} ${requestPath}`);
    console.log(`${eventPrefix} ${colors.flow(getWebhookEventName(parsedBody))}`);
    console.log('');

    try {
      const flow = currentFlow;
      const result = await enqueueRun(() => runWebhookFlow(flow, parsedBody));
      await writeWebhookResponse(res, result);
    } catch (error) {
      res.statusCode = 500;
      res.end(error instanceof Error ? error.message : String(error));
    }
  });

  await new Promise<void>((resolve, reject) => {
    server?.once('error', reject);
    server?.listen(selectedPort, '127.0.0.1', () => {
      server?.off('error', reject);
      resolve();
    });
  });

  printDevStartup(devPrefix, {
    endpointUrl: `http://localhost:${selectedPort}`,
    flowId: flow.id,
    flowPath: relativeFlowPath,
    readyMessage: 'Ready to receive events.',
  });

  watchManager.watchFile(options.filePath, 'flow');

  await new Promise<void>(() => {});
}

export async function devCommand(options: DevOptions): Promise<void> {
  const flow = await loadFlowModule(options.filePath);

  if (flow.trigger?.type !== 'webhook') {
    await runStandardDevMode(options, flow.id);
    return;
  }

  await runWebhookDevMode(options, flow);
}
