import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CliDisplayError, printSuccessSummary, renderCliError } from './cliOutput';

const originalConsoleError = console.error;
const originalConsoleLog = console.log;
const errorSpy = vi.fn();
const logSpy = vi.fn();

beforeEach(() => {
  errorSpy.mockReset();
  logSpy.mockReset();
  console.error = errorSpy;
  console.log = logSpy;
});

afterEach(() => {
  console.error = originalConsoleError;
  console.log = originalConsoleLog;
});

function plain(spy: ReturnType<typeof vi.fn>): string {
  return spy.mock.calls
    .map((call) => String(call[0] ?? ''))
    .join('\n')
    .replace(/\u001b\[[0-9;]*m/g, '');
}

describe('cliOutput', () => {
  it('renders structured error blocks', () => {
    renderCliError(
      new CliDisplayError({
        title: 'Deployment failed',
        details: [
          { label: 'Step', value: 'Uploading deployment package' },
          { label: 'Reason', value: 'Network request timed out' },
        ],
        hint: 'Try again in a moment.',
      }),
    );

    const text = plain(errorSpy);
    expect(text).toMatch(/✖ Deployment failed/);
    expect(text).toMatch(/Step\s+Uploading deployment package/);
    expect(text).toMatch(/Reason\s+Network request timed out/);
    expect(text).toMatch(/Try again in a moment/);
  });

  it('renders structured success summaries', () => {
    printSuccessSummary(
      'Deployment complete',
      [
        { label: 'Program', value: 'hello' },
        { label: 'Version', value: 'v1' },
      ],
      [
        {
          title: 'Program ID',
          lines: ['prg_example'],
        },
      ],
      'Ready to receive events',
    );

    const text = plain(logSpy);
    expect(text).toMatch(/✔ Deployment complete/);
    expect(text).toMatch(/Program\s+hello/);
    expect(text).toMatch(/Version\s+v1/);
    expect(text).toMatch(/Program ID/);
    expect(text).toMatch(/prg_example/);
    expect(text).toMatch(/Ready to receive events/);
  });
});
