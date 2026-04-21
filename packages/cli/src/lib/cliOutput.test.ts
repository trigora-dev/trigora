import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CliDisplayError, printSuccessSummary, renderCliError } from './cliOutput';

const originalConsoleError = console.error;
const originalConsoleLog = console.log;

beforeEach(() => {
  console.error = vi.fn();
  console.log = vi.fn();
});

afterEach(() => {
  console.error = originalConsoleError;
  console.log = originalConsoleLog;
});

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

    expect(console.error).toHaveBeenCalledWith(expect.stringMatching(/✖ Deployment failed/));
    expect(console.error).toHaveBeenCalledWith(
      expect.stringMatching(/Step\s+Uploading deployment package/),
    );
    expect(console.error).toHaveBeenCalledWith(
      expect.stringMatching(/Reason\s+Network request timed out/),
    );
    expect(console.error).toHaveBeenCalledWith(expect.stringMatching(/Try again in a moment/));
  });

  it('renders structured success summaries', () => {
    printSuccessSummary(
      'Deployment complete',
      [
        { label: 'Flow', value: 'hello' },
        { label: 'Trigger', value: 'webhook' },
      ],
      [
        {
          title: 'Endpoint',
          lines: ['https://trigora.dev/f/example'],
        },
      ],
      'Ready to receive events',
    );

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Deployment complete/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Flow\s+hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Trigger\s+webhook/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Endpoint/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/https:\/\/trigora\.dev\/f\/example/),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Ready to receive events/));
  });
});
