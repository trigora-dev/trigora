import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { FlowRunFn, ManualTrigger } from '@trigora/contracts';
import { createLocalContext } from '../lib/createLocalContext';
import { loadFlowModule } from '../lib/loadFlowModule';
import { triggerCommand } from './trigger';

vi.mock('../lib/loadFlowModule', () => ({
  loadFlowModule: vi.fn(),
}));

vi.mock('../lib/createLocalContext', () => ({
  createLocalContext: vi.fn(),
}));

const mockedLoadFlowModule = vi.mocked(loadFlowModule);
const mockedCreateLocalContext = vi.mocked(createLocalContext);

const originalConsoleLog = console.log;
const originalConsoleError = console.error;

const tempDirs: string[] = [];

type TestRunFn = FlowRunFn<unknown, Record<string, string>, ManualTrigger>;
type TestEvent = Parameters<TestRunFn>[0];

async function makeTempDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-trigger-command-'));
  tempDirs.push(dir);
  return dir;
}

function getFirstEventArg(run: ReturnType<typeof vi.fn<TestRunFn>>): TestEvent {
  expect(run).toHaveBeenCalledOnce();

  const firstCall = run.mock.calls[0];
  expect(firstCall).toBeDefined();

  const eventArg = firstCall?.[0];
  expect(eventArg).toBeDefined();

  return eventArg as TestEvent;
}

beforeEach(() => {
  console.log = vi.fn();
  console.error = vi.fn();
});

afterEach(async () => {
  vi.clearAllMocks();

  console.log = originalConsoleLog;
  console.error = originalConsoleError;

  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
});

describe('triggerCommand', () => {
  it('runs the flow with an empty payload when no payload file is provided', async () => {
    const run = vi.fn<TestRunFn>(async () => undefined);

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run,
    });

    mockedCreateLocalContext.mockReturnValue({
      env: {},
      log: {
        info: vi.fn(),
        warn: vi.fn(),
        error: vi.fn(),
      },
    });

    await triggerCommand({
      filePath: './flows/payment.ts',
    });

    expect(mockedLoadFlowModule).toHaveBeenCalledWith('./flows/payment.ts');
    expect(mockedCreateLocalContext).toHaveBeenCalledWith('payment');

    const eventArg = getFirstEventArg(run);
    expect(eventArg.payload).toEqual({});
    expect(eventArg.type).toBe('manual');
    expect(eventArg.id).toMatch(/^evt_local_/);
    expect(eventArg.timestamp).toEqual(expect.any(String));

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Running flow .*payment.*\.\.\./),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Run complete/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Flow\s+.*payment/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Duration\s+\d+ms/));
  });

  it('loads payload from a JSON file and passes it to the flow', async () => {
    const tempDir = await makeTempDir();
    const payloadPath = path.join(tempDir, 'payload.json');

    await fs.writeFile(payloadPath, JSON.stringify({ userId: '123', amount: 50 }), 'utf-8');

    const run = vi.fn<TestRunFn>(async () => undefined);

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run,
    });

    mockedCreateLocalContext.mockReturnValue({
      env: {},
      log: {
        info: vi.fn(),
        warn: vi.fn(),
        error: vi.fn(),
      },
    });

    await triggerCommand({
      filePath: './flows/payment.ts',
      payloadPath,
    });

    const eventArg = getFirstEventArg(run);
    expect(eventArg.payload).toEqual({ userId: '123', amount: 50 });
  });

  it('rejects with a helpful error for invalid JSON payload files', async () => {
    const tempDir = await makeTempDir();
    const payloadPath = path.join(tempDir, 'payload.json');

    await fs.writeFile(payloadPath, '{ invalid json', 'utf-8');

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run: vi.fn<TestRunFn>(async () => undefined),
    });

    mockedCreateLocalContext.mockReturnValue({
      env: {},
      log: {
        info: vi.fn(),
        warn: vi.fn(),
        error: vi.fn(),
      },
    });

    await expect(
      triggerCommand({
        filePath: './flows/payment.ts',
        payloadPath,
      }),
    ).rejects.toThrow(`Invalid JSON in payload file "${payloadPath}".`);
  });

  it('rejects with a helpful error when the payload file cannot be read', async () => {
    const missingPayloadPath = path.join(os.tmpdir(), `trigora-missing-payload-${Date.now()}.json`);

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run: vi.fn<TestRunFn>(async () => undefined),
    });

    mockedCreateLocalContext.mockReturnValue({
      env: {},
      log: {
        info: vi.fn(),
        warn: vi.fn(),
        error: vi.fn(),
      },
    });

    await expect(
      triggerCommand({
        filePath: './flows/payment.ts',
        payloadPath: missingPayloadPath,
      }),
    ).rejects.toThrow(
      new RegExp(
        `^Failed to read payload file "${missingPayloadPath.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}"`,
      ),
    );
  });

  it('prints a helpful error when the flow throws', async () => {
    const run = vi.fn<TestRunFn>(async () => {
      throw new Error('something went wrong');
    });

    mockedLoadFlowModule.mockResolvedValue({
      id: 'payment',
      trigger: { type: 'manual' },
      run,
    });

    mockedCreateLocalContext.mockReturnValue({
      env: {},
      log: {
        info: vi.fn(),
        warn: vi.fn(),
        error: vi.fn(),
      },
    });

    await triggerCommand({
      filePath: './flows/payment.ts',
    });

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Running flow .*payment.*\.\.\./),
    );
    expect(console.error).toHaveBeenCalledWith(expect.stringMatching(/✖ Run failed/));
    expect(console.error).toHaveBeenCalledWith(expect.stringMatching(/Flow\s+.*payment/));
    expect(console.error).toHaveBeenCalledWith(expect.stringMatching(/Duration\s+\d+ms/));
    expect(console.error).toHaveBeenCalledWith('something went wrong');
  });

  it('runs queue flows with a synthetic local QueueFlowEvent', async () => {
    const run = vi.fn(async (_event: unknown, _ctx: unknown) => undefined);

    mockedLoadFlowModule.mockResolvedValue({
      id: 'orders-processor',
      trigger: { type: 'queue', queue: 'orders' },
      retry: { attempts: 5, backoff: 'exponential' },
      run,
    });

    mockedCreateLocalContext.mockReturnValue({
      env: {},
      log: {
        info: vi.fn(),
        warn: vi.fn(),
        error: vi.fn(),
      },
    });

    const tempDir = await makeTempDir();
    const payloadPath = path.join(tempDir, 'payload.json');
    await fs.writeFile(payloadPath, JSON.stringify({ orderId: 'ord_1' }), 'utf-8');

    await triggerCommand({
      filePath: './flows/orders-processor.ts',
      payloadPath,
    });

    expect(run).toHaveBeenCalledOnce();
    const eventArg = run.mock.calls[0]?.[0] as {
      type: string;
      payload: { orderId: string };
      queue: string;
      messageId: string;
      attempt: number;
      maxAttempts: number;
    };
    expect(eventArg.type).toBe('queue');
    expect(eventArg.payload).toEqual({ orderId: 'ord_1' });
    expect(eventArg.queue).toBe('orders');
    expect(eventArg.messageId).toMatch(/^local_/);
    expect(eventArg.attempt).toBe(1);
    expect(eventArg.maxAttempts).toBe(5);
  });

  it('rejects webhook flows with a clear error', async () => {
    mockedLoadFlowModule.mockResolvedValue({
      id: 'stripe-checkout',
      trigger: { type: 'webhook' },
      run: vi.fn(async () => undefined),
    });

    await expect(
      triggerCommand({
        filePath: './flows/stripe-checkout.ts',
      }),
    ).rejects.toThrow(
      'Flow "stripe-checkout" uses trigger "webhook". trigora trigger supports manual- and queue-triggered flows.',
    );
  });
});
