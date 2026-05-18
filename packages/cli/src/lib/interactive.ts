import { stdin, stdout } from 'node:process';
import { CliDisplayError } from './cliOutput';
import readline from 'node:readline/promises';

function createInteractiveFailure(reason: string, hint?: string): CliDisplayError {
  return new CliDisplayError({
    title: 'Interactive input required',
    details: [{ label: 'Reason', value: reason }],
    hint,
    message: reason,
  });
}

export async function promptForSecretValue(secretName: string): Promise<string> {
  if (!stdin.isTTY || !stdout.isTTY) {
    throw createInteractiveFailure(
      `A value is required to set "${secretName}".`,
      'Re-run with --value in non-interactive environments.',
    );
  }

  stdout.write('\nEnter value (input hidden): ');

  return new Promise<string>((resolve, reject) => {
    const input = stdin;
    const previousEncoding = input.readableEncoding;
    const wasRaw = input.isRaw;
    let value = '';

    function cleanup() {
      input.off('data', onData);

      if (typeof input.setRawMode === 'function') {
        input.setRawMode(Boolean(wasRaw));
      }

      if (previousEncoding && previousEncoding !== 'utf8') {
        input.setEncoding(previousEncoding);
      }

      input.pause();
    }

    function finishWithValue() {
      stdout.write('\n');
      cleanup();
      resolve(value);
    }

    function cancelInput() {
      stdout.write('\n');
      cleanup();
      reject(
        createInteractiveFailure(
          `Secret entry for "${secretName}" was canceled.`,
          'Run the command again to continue.',
        ),
      );
    }

    function onData(chunk: string | Buffer) {
      const text = typeof chunk === 'string' ? chunk : chunk.toString('utf8');

      for (const char of text) {
        if (char === '\u0003') {
          cancelInput();
          return;
        }

        if (char === '\r' || char === '\n') {
          finishWithValue();
          return;
        }

        if (char === '\u0008' || char === '\u007f') {
          value = value.slice(0, -1);
          continue;
        }

        value += char;
      }
    }

    if (typeof input.setRawMode === 'function') {
      input.setRawMode(true);
    }

    input.setEncoding('utf8');
    input.resume();
    input.on('data', onData);
  });
}

export async function confirmAction(message: string): Promise<boolean> {
  if (!stdin.isTTY || !stdout.isTTY) {
    throw createInteractiveFailure(
      'Confirmation is required before deleting this secret.',
      'Re-run with --yes to confirm in non-interactive environments.',
    );
  }

  const rl = readline.createInterface({
    input: stdin,
    output: stdout,
    terminal: true,
  });

  try {
    const answer = await rl.question(`${message} [y/N] `);
    return /^(y|yes)$/i.test(answer.trim());
  } finally {
    rl.close();
  }
}

export async function promptForTypedConfirmation(options: {
  expectedValue: string;
  message: string;
  nonInteractiveHint: string;
  nonInteractiveReason: string;
}): Promise<boolean> {
  if (!stdin.isTTY || !stdout.isTTY) {
    throw createInteractiveFailure(options.nonInteractiveReason, options.nonInteractiveHint);
  }

  const rl = readline.createInterface({
    input: stdin,
    output: stdout,
    terminal: true,
  });

  try {
    stdout.write(`\n${options.message}\n\n`);
    const answer = await rl.question(`Type "${options.expectedValue}" to confirm: `);
    return answer.trim() === options.expectedValue;
  } finally {
    rl.close();
  }
}
