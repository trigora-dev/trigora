import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import path from 'node:path';
import { loadFlowModule } from '../lib/loadFlowModule';
import { devCommand } from './dev';
import { triggerCommand } from './trigger';

vi.mock('node:fs', () => ({
  default: {
    statSync: vi.fn(),
    watch: vi.fn(),
  },
}));

vi.mock('node:http', () => ({
  default: {
    createServer: vi.fn(),
  },
}));

vi.mock('node:net', () => ({
  default: {
    createServer: vi.fn(),
  },
}));

vi.mock('./trigger', () => ({
  triggerCommand: vi.fn(),
}));

vi.mock('../lib/loadFlowModule', () => ({
  loadFlowModule: vi.fn(),
}));

const mockedFsWatch = vi.mocked(fs.watch);
const mockedFsStatSync = vi.mocked(fs.statSync);
const mockedHttpCreateServer = vi.mocked(http.createServer);
const mockedNetCreateServer = vi.mocked(net.createServer);
const mockedTriggerCommand = vi.mocked(triggerCommand);
const mockedLoadFlowModule = vi.mocked(loadFlowModule);

const originalConsoleLog = console.log;
const originalConsoleError = console.error;
const originalProcessOn = process.on;
const originalProcessOff = process.off;
const originalProcessExit = process.exit;
const fileVersions = new Map<string, number>();

type WatchCallback = (eventType: string) => void;

type MockWatcher = {
  close: ReturnType<typeof vi.fn>;
};

type MockHttpResponse = {
  body: string;
  headers: Record<string, string>;
  statusCode: number;
};

type MockHttpRequestOptions = {
  body?: string;
  method?: string;
  url?: string;
};

type MockHttpServer = {
  close: ReturnType<typeof vi.fn>;
  errorHandler?: (error: unknown) => void;
  listen: ReturnType<typeof vi.fn>;
  listenPort?: number;
  off: ReturnType<typeof vi.fn>;
  once: ReturnType<typeof vi.fn>;
  requestHandler: (
    req: http.IncomingMessage,
    res: http.ServerResponse<http.IncomingMessage>,
  ) => Promise<void> | void;
};

type MockNetServer = {
  close: ReturnType<typeof vi.fn>;
  errorHandler?: (error: NodeJS.ErrnoException) => void;
  listen: ReturnType<typeof vi.fn>;
  listeningHandler?: () => void;
  once: ReturnType<typeof vi.fn>;
};

function getLoggedEndpoint(): string | undefined {
  const consoleLogMock = console.log as unknown as ReturnType<typeof vi.fn>;

  return consoleLogMock.mock.calls
    .map(([value]) => value)
    .find(
      (value): value is string =>
        typeof value === 'string' && value.startsWith('http://localhost:'),
    );
}

function getFlowPath(name: string): string {
  return path.join(process.cwd(), 'flows', `${name}.ts`);
}

function getPayloadPath(name = 'payload.json'): string {
  return path.join(process.cwd(), name);
}

function setFileVersion(filePath: string, version: number): void {
  fileVersions.set(filePath, version);
}

async function flushMicrotasks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

function countLogCalls(pattern: RegExp): number {
  const consoleLogMock = console.log as unknown as ReturnType<typeof vi.fn>;

  return consoleLogMock.mock.calls.filter(
    ([value]) => typeof value === 'string' && pattern.test(value),
  ).length;
}

describe('devCommand', () => {
  const watchers = new Map<string, WatchCallback>();
  const signalHandlers = new Map<string, () => void>();
  const watcherInstances: MockWatcher[] = [];
  const httpServers: MockHttpServer[] = [];
  const occupiedPorts = new Set<number>();

  beforeEach(() => {
    vi.useFakeTimers();

    watchers.clear();
    signalHandlers.clear();
    watcherInstances.length = 0;
    httpServers.length = 0;
    occupiedPorts.clear();
    fileVersions.clear();

    console.log = vi.fn();
    console.error = vi.fn();

    mockedTriggerCommand.mockReset();
    mockedTriggerCommand.mockResolvedValue(undefined);

    mockedLoadFlowModule.mockReset();

    mockedFsStatSync.mockImplementation(
      (filePath) =>
        ({
          mtimeMs: fileVersions.get(String(filePath)) ?? 0,
        }) as fs.Stats,
    );

    mockedFsWatch.mockImplementation((filePath, callback) => {
      fileVersions.set(String(filePath), fileVersions.get(String(filePath)) ?? 0);
      watchers.set(String(filePath), callback as WatchCallback);

      const watcher = {
        close: vi.fn(),
      };

      watcherInstances.push(watcher);

      return watcher as never;
    });

    mockedNetCreateServer.mockImplementation(() => {
      const server: MockNetServer = {
        close: vi.fn((callback?: () => void) => {
          callback?.();
          return server as never;
        }),
        listen: vi.fn((port: number) => {
          if (occupiedPorts.has(port)) {
            const error = Object.assign(new Error('Port in use'), {
              code: 'EADDRINUSE',
            }) as NodeJS.ErrnoException;

            server.errorHandler?.(error);
            return server as never;
          }

          server.listeningHandler?.();
          return server as never;
        }),
        once: vi.fn((event: string, handler: (...args: unknown[]) => void) => {
          if (event === 'error') {
            server.errorHandler = handler as (error: NodeJS.ErrnoException) => void;
          }

          if (event === 'listening') {
            server.listeningHandler = handler as () => void;
          }

          return server as never;
        }),
      };

      return server as never;
    });

    mockedHttpCreateServer.mockImplementation(((requestHandler?: unknown) => {
      const server: MockHttpServer = {
        close: vi.fn(() => server as never),
        listen: vi.fn((port: number, _host: string, callback?: () => void) => {
          server.listenPort = port;
          callback?.();
          return server as never;
        }),
        off: vi.fn(() => server as never),
        once: vi.fn((event: string, handler: (...args: unknown[]) => void) => {
          if (event === 'error') {
            server.errorHandler = handler as (error: unknown) => void;
          }

          return server as never;
        }),
        requestHandler: requestHandler as MockHttpServer['requestHandler'],
      };

      httpServers.push(server);

      return server as never;
    }) as never);

    process.on = vi.fn((event: string, handler: () => void) => {
      signalHandlers.set(event, handler);
      return process;
    }) as typeof process.on;

    process.off = vi.fn((event: string) => {
      signalHandlers.delete(event);
      return process;
    }) as typeof process.off;

    process.exit = vi.fn(((code?: number) => code as never) as typeof process.exit);
  });

  afterEach(() => {
    const sigintHandler = signalHandlers.get('SIGINT');
    sigintHandler?.();

    vi.useRealTimers();
    vi.restoreAllMocks();

    console.log = originalConsoleLog;
    console.error = originalConsoleError;
    process.on = originalProcessOn;
    process.off = originalProcessOff;
    process.exit = originalProcessExit;
  });

  async function sendHttpRequest(
    port: number,
    options: MockHttpRequestOptions = {},
  ): Promise<MockHttpResponse> {
    const server = httpServers.find((candidate) => candidate.listenPort === port);

    if (!server) {
      throw new Error(`No mock HTTP server found on port ${port}.`);
    }

    const reqHandlers = new Map<string, Array<(value?: unknown) => void>>();
    const headers: Record<string, string> = {};

    const req = {
      method: options.method ?? 'POST',
      on(event: string, handler: (value?: unknown) => void) {
        reqHandlers.set(event, [...(reqHandlers.get(event) ?? []), handler]);
        return req;
      },
      url: options.url ?? '/',
    } as unknown as http.IncomingMessage;

    return new Promise<MockHttpResponse>((resolve, reject) => {
      const res = {
        end(body?: string | Buffer) {
          resolve({
            body: body ? String(body) : '',
            headers,
            statusCode: res.statusCode,
          });
          return res;
        },
        setHeader(name: string, value: string) {
          headers[name] = value;
          return res;
        },
        statusCode: 200,
      } as unknown as http.ServerResponse<http.IncomingMessage> & { statusCode: number };

      void Promise.resolve(server.requestHandler(req, res)).catch(reject);

      queueMicrotask(() => {
        const body = options.body ?? '';

        if (body.length > 0) {
          for (const handler of reqHandlers.get('data') ?? []) {
            handler(body);
          }
        }

        for (const handler of reqHandlers.get('end') ?? []) {
          handler();
        }
      });
    });
  }

  it('runs once on startup and starts watching the flow file for non-webhook flows', async () => {
    const flowPath = getFlowPath('payment');

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run: vi.fn(),
    } as never);

    void devCommand({
      filePath: flowPath,
    });

    await vi.runAllTimersAsync();

    expect(mockedTriggerCommand).toHaveBeenCalledTimes(1);
    expect(mockedTriggerCommand).toHaveBeenCalledWith({
      filePath: flowPath,
    });

    expect(mockedFsWatch).toHaveBeenCalledWith(flowPath, expect.any(Function));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/\[dev\].*running.*payment/));
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('Watching flow:'));
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('flows/payment.ts'));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringContaining('Ready. Edit the flow to rerun.'),
    );
    expect(console.log).not.toHaveBeenCalledWith(expect.stringContaining('Watching payload:'));
  });

  it('watches the payload file when provided for non-webhook flows', async () => {
    const flowPath = getFlowPath('payment');
    const payloadPath = getPayloadPath();

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run: vi.fn(),
    } as never);

    void devCommand({
      filePath: flowPath,
      payloadPath,
    });

    await vi.runAllTimersAsync();

    expect(mockedFsWatch).toHaveBeenCalledWith(flowPath, expect.any(Function));
    expect(mockedFsWatch).toHaveBeenCalledWith(payloadPath, expect.any(Function));

    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('Watching payload:'));
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('payload.json'));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringContaining('Ready. Edit the flow or payload to rerun.'),
    );
  });

  it('reruns when the flow file changes for non-webhook flows', async () => {
    const flowPath = getFlowPath('payment');

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run: vi.fn(),
    } as never);

    void devCommand({
      filePath: flowPath,
    });

    await vi.runAllTimersAsync();

    const flowWatcher = watchers.get(flowPath);
    expect(flowWatcher).toBeDefined();

    setFileVersion(flowPath, 1);
    flowWatcher?.('change');

    await vi.advanceTimersByTimeAsync(100);

    expect(mockedTriggerCommand).toHaveBeenCalledTimes(2);
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/flow changed.*rerunning/));
  });

  it('debounces repeated file changes into a single rerun for non-webhook flows', async () => {
    const flowPath = getFlowPath('payment');

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run: vi.fn(),
    } as never);

    void devCommand({
      filePath: flowPath,
    });

    await vi.runAllTimersAsync();

    const flowWatcher = watchers.get(flowPath);
    expect(flowWatcher).toBeDefined();

    setFileVersion(flowPath, 1);
    flowWatcher?.('change');
    flowWatcher?.('change');
    flowWatcher?.('change');

    await vi.advanceTimersByTimeAsync(99);
    expect(mockedTriggerCommand).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(1);
    expect(mockedTriggerCommand).toHaveBeenCalledTimes(2);
  });

  it('suppresses duplicate save events for non-webhook flows', async () => {
    const flowPath = getFlowPath('payment');

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run: vi.fn(),
    } as never);

    void devCommand({
      filePath: flowPath,
    });

    await vi.runAllTimersAsync();

    const flowWatcher = watchers.get(flowPath);
    expect(flowWatcher).toBeDefined();

    setFileVersion(flowPath, 1);
    flowWatcher?.('change');
    await vi.advanceTimersByTimeAsync(100);
    await flushMicrotasks();

    flowWatcher?.('change');
    await vi.advanceTimersByTimeAsync(100);
    await flushMicrotasks();

    expect(mockedTriggerCommand).toHaveBeenCalledTimes(2);
    expect(countLogCalls(/flow changed.*rerunning/)).toBe(1);
  });

  it('shuts down cleanly on SIGINT for non-webhook flows', async () => {
    const flowPath = getFlowPath('payment');
    const payloadPath = getPayloadPath();

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run: vi.fn(),
    } as never);

    void devCommand({
      filePath: flowPath,
      payloadPath,
    });

    await vi.runAllTimersAsync();

    const sigintHandler = signalHandlers.get('SIGINT');
    expect(sigintHandler).toBeDefined();

    sigintHandler?.();

    expect(watcherInstances).toHaveLength(2);
    expect(watcherInstances[0]?.close).toHaveBeenCalledOnce();
    expect(watcherInstances[1]?.close).toHaveBeenCalledOnce();

    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('[dev] stopped'));
    expect(process.exit).toHaveBeenCalledWith(0);
  });

  it('queues one rerun message when changes happen during an active non-webhook run', async () => {
    const flowPath = getFlowPath('payment');
    let resolveSecondRun!: () => void;

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run: vi.fn(),
    } as never);

    mockedTriggerCommand
      .mockResolvedValueOnce(undefined)
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            resolveSecondRun = resolve;
          }),
      )
      .mockResolvedValue(undefined);

    void devCommand({
      filePath: flowPath,
    });

    await vi.runAllTimersAsync();

    const flowWatcher = watchers.get(flowPath);
    expect(flowWatcher).toBeDefined();

    setFileVersion(flowPath, 1);
    flowWatcher?.('change');
    await vi.advanceTimersByTimeAsync(100);
    await flushMicrotasks();

    expect(mockedTriggerCommand).toHaveBeenCalledTimes(2);

    setFileVersion(flowPath, 2);
    flowWatcher?.('change');
    await vi.advanceTimersByTimeAsync(100);
    await flushMicrotasks();

    expect(mockedTriggerCommand).toHaveBeenCalledTimes(2);

    resolveSecondRun();
    await flushMicrotasks();

    expect(mockedTriggerCommand).toHaveBeenCalledTimes(3);
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/changes queued.*rerunning/));
  });

  it('starts a local webhook server for webhook flows and runs the flow on POST requests', async () => {
    const flowPath = getFlowPath('stripe-checkout');

    const run = vi.fn(async (_event, ctx) => {
      await ctx.log.info('New purchase', {
        email: 'customer@example.com',
      });

      return { ok: true };
    });

    mockedLoadFlowModule.mockResolvedValue({
      id: 'stripe-checkout',
      trigger: { type: 'webhook' },
      run,
    } as never);

    void devCommand({
      filePath: flowPath,
    });

    await flushMicrotasks();
    await vi.runAllTimersAsync();
    await flushMicrotasks();

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/\[dev\].*running.*stripe-checkout/),
    );
    expect(getLoggedEndpoint()).toBe('http://localhost:5252');
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('Watching flow:'));
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('flows/stripe-checkout.ts'));
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('Ready to receive events.'));

    const response = await sendHttpRequest(5252, {
      body: JSON.stringify({
        type: 'checkout.session.completed',
        data: {
          object: {
            id: 'cs_test_123',
          },
        },
      }),
    });

    await flushMicrotasks();

    expect(response.statusCode).toBe(200);
    expect(response.body).toBe(JSON.stringify({ ok: true }));
    expect(response.headers['Content-Type']).toBe('application/json; charset=utf-8');

    expect(run).toHaveBeenCalledTimes(1);

    const firstEvent = (run.mock.calls as unknown[][])[0]?.[0];
    expect(firstEvent).toMatchObject({
      type: 'webhook',
      payload: {
        type: 'checkout.session.completed',
        data: {
          object: {
            id: 'cs_test_123',
          },
        },
      },
    });

    expect(console.log).toHaveBeenCalledWith('');
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/\[request\].*POST.*\//));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/\[event\].*checkout\.session\.completed/),
    );
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/\[stripe-checkout\].*INFO.*New purchase/),
      { email: 'customer@example.com' },
    );
  });

  it('reloads webhook flows on file changes without executing them and uses the updated flow on the next request', async () => {
    const flowPath = getFlowPath('stripe-checkout');
    const initialRun = vi.fn(async () => ({ ok: 'initial' }));
    const updatedRun = vi.fn(async () => ({ ok: 'updated' }));

    mockedLoadFlowModule
      .mockResolvedValueOnce({
        id: 'stripe-checkout',
        trigger: { type: 'webhook' },
        run: initialRun,
      } as never)
      .mockResolvedValueOnce({
        id: 'stripe-checkout',
        trigger: { type: 'webhook' },
        run: updatedRun,
      } as never);

    void devCommand({
      filePath: flowPath,
    });

    await flushMicrotasks();
    await vi.runAllTimersAsync();
    await flushMicrotasks();

    const flowWatcher = watchers.get(flowPath);
    expect(flowWatcher).toBeDefined();

    setFileVersion(flowPath, 1);
    flowWatcher?.('change');

    await vi.advanceTimersByTimeAsync(100);
    await flushMicrotasks();

    expect(initialRun).not.toHaveBeenCalled();
    expect(updatedRun).not.toHaveBeenCalled();
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/flow changed.*reloaded/));
    expect(console.log).toHaveBeenCalledWith('');

    const body = {
      type: 'checkout.session.completed',
      data: {
        object: {
          id: 'cs_test_123',
        },
      },
    };

    const response = await sendHttpRequest(5252, {
      body: JSON.stringify(body),
    });

    await flushMicrotasks();

    expect(response.statusCode).toBe(200);
    expect(response.body).toBe(JSON.stringify({ ok: 'updated' }));
    expect(initialRun).not.toHaveBeenCalled();
    expect(updatedRun).toHaveBeenCalledTimes(1);

    const event = (updatedRun.mock.calls as unknown[][])[0]?.[0];
    expect(event).toMatchObject({
      type: 'webhook',
      payload: body,
    });
  });

  it('suppresses duplicate save events for webhook flow reloads', async () => {
    const flowPath = getFlowPath('stripe-checkout');
    const updatedRun = vi.fn(async () => ({ ok: 'updated' }));

    mockedLoadFlowModule
      .mockResolvedValueOnce({
        id: 'stripe-checkout',
        trigger: { type: 'webhook' },
        run: vi.fn(async () => ({ ok: 'initial' })),
      } as never)
      .mockResolvedValueOnce({
        id: 'stripe-checkout',
        trigger: { type: 'webhook' },
        run: updatedRun,
      } as never);

    void devCommand({
      filePath: flowPath,
    });

    await flushMicrotasks();
    await vi.runAllTimersAsync();
    await flushMicrotasks();

    const flowWatcher = watchers.get(flowPath);
    expect(flowWatcher).toBeDefined();

    setFileVersion(flowPath, 1);
    flowWatcher?.('change');
    await vi.advanceTimersByTimeAsync(100);
    await flushMicrotasks();

    flowWatcher?.('change');
    await vi.advanceTimersByTimeAsync(100);
    await flushMicrotasks();

    expect(mockedLoadFlowModule).toHaveBeenCalledTimes(2);
    expect(countLogCalls(/flow changed.*reloaded/)).toBe(1);

    const response = await sendHttpRequest(5252, {
      body: JSON.stringify({ type: 'checkout.session.completed' }),
    });

    await flushMicrotasks();

    expect(response.statusCode).toBe(200);
    expect(updatedRun).toHaveBeenCalledTimes(1);
  });

  it('returns 400 for invalid webhook JSON bodies', async () => {
    const flowPath = getFlowPath('stripe-checkout');

    mockedLoadFlowModule.mockResolvedValue({
      id: 'stripe-checkout',
      trigger: { type: 'webhook' },
      run: vi.fn(async () => ({ ok: true })),
    } as never);

    void devCommand({
      filePath: flowPath,
    });

    await flushMicrotasks();
    await vi.runAllTimersAsync();
    await flushMicrotasks();

    const response = await sendHttpRequest(5252, {
      body: '{',
    });

    expect(response.statusCode).toBe(400);
    expect(response.body).toBe('Invalid JSON body.');
  });

  it('falls back to the next available port when 5252 is already in use', async () => {
    const flowPath = getFlowPath('stripe-checkout');

    occupiedPorts.add(5252);

    mockedLoadFlowModule.mockResolvedValue({
      id: 'stripe-checkout',
      trigger: { type: 'webhook' },
      run: vi.fn(async () => ({ ok: true })),
    } as never);

    void devCommand({
      filePath: flowPath,
    });

    await flushMicrotasks();
    await vi.runAllTimersAsync();
    await flushMicrotasks();

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/\[dev\].*Port 5252 was in use, using 5253 instead\./),
    );
    expect(getLoggedEndpoint()).toBe('http://localhost:5253');
  });
});
