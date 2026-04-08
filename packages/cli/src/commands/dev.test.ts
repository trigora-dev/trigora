import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import fs from 'node:fs';
import { devCommand } from './dev';
import { triggerCommand } from './trigger';

vi.mock('node:fs', () => ({
  default: {
    watch: vi.fn(),
  },
}));

vi.mock('./trigger', () => ({
  triggerCommand: vi.fn(),
}));

const mockedFsWatch = vi.mocked(fs.watch);
const mockedTriggerCommand = vi.mocked(triggerCommand);

const originalConsoleLog = console.log;
const originalConsoleError = console.error;
const originalProcessOn = process.on;
const originalProcessOff = process.off;
const originalProcessExit = process.exit;

type WatchCallback = (eventType: string) => void;

type MockWatcher = {
  close: ReturnType<typeof vi.fn>;
};

describe('devCommand', () => {
  const watchers = new Map<string, WatchCallback>();
  const signalHandlers = new Map<string, () => void>();
  const watcherInstances: MockWatcher[] = [];

  beforeEach(() => {
    vi.useFakeTimers();

    watchers.clear();
    signalHandlers.clear();
    watcherInstances.length = 0;

    console.log = vi.fn();
    console.error = vi.fn();

    mockedTriggerCommand.mockReset();
    mockedTriggerCommand.mockResolvedValue(undefined);

    mockedFsWatch.mockImplementation((filePath, callback) => {
      watchers.set(String(filePath), callback as WatchCallback);

      const watcher = {
        close: vi.fn(),
      };

      watcherInstances.push(watcher);

      return watcher as never;
    });

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
    vi.useRealTimers();
    vi.restoreAllMocks();

    console.log = originalConsoleLog;
    console.error = originalConsoleError;
    process.on = originalProcessOn;
    process.off = originalProcessOff;
    process.exit = originalProcessExit;
  });

  it('runs once on startup and starts watching the flow file', async () => {
    void devCommand({
      filePath: '/project/flows/payment.ts',
    });

    await vi.runAllTimersAsync();

    expect(mockedTriggerCommand).toHaveBeenCalledTimes(1);
    expect(mockedTriggerCommand).toHaveBeenCalledWith({
      filePath: '/project/flows/payment.ts',
    });

    expect(mockedFsWatch).toHaveBeenCalledWith('/project/flows/payment.ts', expect.any(Function));

    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('[dev]'));
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('watching flow:'));
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('ready'));
  });

  it('watches the payload file when provided', async () => {
    void devCommand({
      filePath: '/project/flows/payment.ts',
      payloadPath: '/project/payload.json',
    });

    await vi.runAllTimersAsync();

    expect(mockedFsWatch).toHaveBeenCalledWith('/project/flows/payment.ts', expect.any(Function));
    expect(mockedFsWatch).toHaveBeenCalledWith('/project/payload.json', expect.any(Function));

    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('watching payload:'));
  });

  it('reruns when the flow file changes', async () => {
    void devCommand({
      filePath: '/project/flows/payment.ts',
    });

    await vi.runAllTimersAsync();

    const flowWatcher = watchers.get('/project/flows/payment.ts');
    expect(flowWatcher).toBeDefined();

    flowWatcher?.('change');

    await vi.advanceTimersByTimeAsync(100);

    expect(mockedTriggerCommand).toHaveBeenCalledTimes(2);
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/flow changed.*re-running/));
  });

  it('reruns when the payload file changes', async () => {
    void devCommand({
      filePath: '/project/flows/payment.ts',
      payloadPath: '/project/payload.json',
    });

    await vi.runAllTimersAsync();

    const payloadWatcher = watchers.get('/project/payload.json');
    expect(payloadWatcher).toBeDefined();

    payloadWatcher?.('change');

    await vi.advanceTimersByTimeAsync(100);

    expect(mockedTriggerCommand).toHaveBeenCalledTimes(2);
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/payload changed.*re-running/));
  });

  it('ignores non-change watch events', async () => {
    void devCommand({
      filePath: '/project/flows/payment.ts',
    });

    await vi.runAllTimersAsync();

    const flowWatcher = watchers.get('/project/flows/payment.ts');
    expect(flowWatcher).toBeDefined();

    flowWatcher?.('rename');

    await vi.advanceTimersByTimeAsync(100);

    expect(mockedTriggerCommand).toHaveBeenCalledTimes(1);
  });

  it('debounces repeated file changes into a single rerun', async () => {
    void devCommand({
      filePath: '/project/flows/payment.ts',
    });

    await vi.runAllTimersAsync();

    const flowWatcher = watchers.get('/project/flows/payment.ts');
    expect(flowWatcher).toBeDefined();

    flowWatcher?.('change');
    flowWatcher?.('change');
    flowWatcher?.('change');

    await vi.advanceTimersByTimeAsync(99);
    expect(mockedTriggerCommand).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(1);
    expect(mockedTriggerCommand).toHaveBeenCalledTimes(2);
  });

  it('shuts down cleanly on SIGINT', async () => {
    void devCommand({
      filePath: '/project/flows/payment.ts',
      payloadPath: '/project/payload.json',
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

  it('prints a helpful error when a rerun fails', async () => {
    mockedTriggerCommand
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(new Error('rerun exploded'));

    void devCommand({
      filePath: '/project/flows/payment.ts',
    });

    await vi.runAllTimersAsync();

    const flowWatcher = watchers.get('/project/flows/payment.ts');
    expect(flowWatcher).toBeDefined();

    flowWatcher?.('change');

    await vi.advanceTimersByTimeAsync(100);

    expect(console.error).toHaveBeenCalledWith(expect.stringMatching(/\[dev\].*re-run failed/));
    expect(console.error).toHaveBeenCalledWith('rerun exploded');
  });
});
