import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { triggerCommand } from './trigger';
import { createLocalContext } from '../lib/createLocalContext';
import { loadFlowModule } from '../lib/loadFlowModule';
import type { FlowRunFn } from '@trigora/contracts';

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

type TestRunFn = FlowRunFn<unknown, Record<string, string>>;
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

    expect(console.log).toHaveBeenCalledWith('[payment] execution started');
  });

  it('loads payload from a JSON file and passes it to the flow', async () => {
    const tempDir = await makeTempDir();
    const payloadPath = path.join(tempDir, 'payload.json');

    await fs.writeFile(
      payloadPath,
      JSON.stringify({ userId: '123', amount: 50 }),
      'utf-8',
    );

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

  it('throws a helpful error for invalid JSON payload files', async () => {
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

    expect(console.error).toHaveBeenCalledWith(
      expect.stringMatching(/^\[payment\] execution failed \(\d+ms\)$/),
    );
    expect(console.error).toHaveBeenCalledWith('something went wrong');
  });
});