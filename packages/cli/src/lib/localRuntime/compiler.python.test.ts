import { EventEmitter } from 'node:events';

import { beforeEach, describe, expect, it, vi } from 'vitest';

import { CliDisplayError } from '../cliOutput';

const spawn = vi.fn();

vi.mock('node:child_process', () => ({
  spawn,
}));

function fakePython(stdout: string, status = 0): void {
  spawn.mockImplementationOnce(() => {
    const child = new EventEmitter() as EventEmitter & {
      stdout: EventEmitter;
      stderr: EventEmitter;
      stdin: { end: () => void };
    };
    child.stdout = new EventEmitter();
    child.stderr = new EventEmitter();
    child.stdin = { end: () => undefined };
    queueMicrotask(() => {
      child.stdout.emit('data', stdout);
      child.emit('close', status);
    });
    return child;
  });
}

describe('Python compiler pin', () => {
  beforeEach(() => {
    spawn.mockReset();
  });

  it('tells you to install tcc-engine when the compiler is missing', async () => {
    fakePython(JSON.stringify({ status: 'missing' }));
    const { compilePythonProgram } = await import('./compiler');
    const error = await compilePythonProgram(
      'async def approval():\n    return 1\n',
      'approval.py',
    ).catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(CliDisplayError);
    expect(error).toMatchObject({
      message: 'Python compiler support is not installed.',
      details: expect.arrayContaining([
        expect.objectContaining({
          value: 'python3 -m pip install trigora',
        }),
      ]),
    });
  });

  it('rejects a tcc-engine version other than the CLI pin', async () => {
    fakePython(JSON.stringify({ status: 'ok', version: '0.2.0' }));
    const { compilePythonProgram } = await import('./compiler');
    await expect(
      compilePythonProgram('async def approval():\n    return 1\n', 'approval.py'),
    ).rejects.toThrow(/Python compiler 0\.1\.0-rc\.1/);
  });
});
