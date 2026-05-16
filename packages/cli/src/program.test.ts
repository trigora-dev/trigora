import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createProgram } from './program';
import { getLogCommand, listLogsCommand } from './commands/logs';
import { deleteSecretCommand, listSecretsCommand, setSecretCommand } from './commands/secrets';
import { whoAmICommand } from './commands/whoami';

vi.mock('./commands/logs', () => ({
  getLogCommand: vi.fn(),
  listLogsCommand: vi.fn(),
}));

vi.mock('./commands/secrets', () => ({
  deleteSecretCommand: vi.fn(),
  listSecretsCommand: vi.fn(),
  setSecretCommand: vi.fn(),
}));

vi.mock('./commands/whoami', () => ({
  whoAmICommand: vi.fn(),
}));

const mockedGetLogCommand = vi.mocked(getLogCommand);
const mockedListLogsCommand = vi.mocked(listLogsCommand);
const mockedDeleteSecretCommand = vi.mocked(deleteSecretCommand);
const mockedListSecretsCommand = vi.mocked(listSecretsCommand);
const mockedSetSecretCommand = vi.mocked(setSecretCommand);
const mockedWhoAmICommand = vi.mocked(whoAmICommand);

beforeEach(() => {
  mockedGetLogCommand.mockReset();
  mockedListLogsCommand.mockReset();
  mockedDeleteSecretCommand.mockReset();
  mockedListSecretsCommand.mockReset();
  mockedSetSecretCommand.mockReset();
  mockedWhoAmICommand.mockReset();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('createProgram', () => {
  it('routes secrets set arguments to the command handler', async () => {
    const program = createProgram();

    program.exitOverride();
    program.configureOutput({
      writeErr: () => undefined,
      writeOut: () => undefined,
    });

    await program.parseAsync(
      ['secrets', 'set', 'STRIPE_WEBHOOK_SECRET', '--flow', 'stripe-checkout', '--value', 'x'],
      { from: 'user' },
    );

    expect(mockedSetSecretCommand).toHaveBeenCalledWith({
      flow: 'stripe-checkout',
      name: 'STRIPE_WEBHOOK_SECRET',
      value: 'x',
    });
  });

  it('routes secrets list and delete options to their handlers', async () => {
    const program = createProgram();

    program.exitOverride();
    program.configureOutput({
      writeErr: () => undefined,
      writeOut: () => undefined,
    });

    await program.parseAsync(['secrets', 'list', '--flow', 'stripe-checkout'], { from: 'user' });
    await program.parseAsync(
      ['secrets', 'delete', 'STRIPE_WEBHOOK_SECRET', '--flow', 'stripe-checkout', '--yes'],
      { from: 'user' },
    );

    expect(mockedListSecretsCommand).toHaveBeenCalledWith({
      flow: 'stripe-checkout',
    });
    expect(mockedDeleteSecretCommand).toHaveBeenCalledWith({
      flow: 'stripe-checkout',
      name: 'STRIPE_WEBHOOK_SECRET',
      yes: true,
    });
  });

  it('routes logs list and get options to their handlers', async () => {
    const program = createProgram();

    program.exitOverride();
    program.configureOutput({
      writeErr: () => undefined,
      writeOut: () => undefined,
    });

    await program.parseAsync(['logs', 'list', '--flow', 'stripe-checkout'], {
      from: 'user',
    });
    await program.parseAsync(['logs', 'get', 'inv_123', '--flow', 'stripe-checkout'], {
      from: 'user',
    });

    expect(mockedListLogsCommand).toHaveBeenCalledWith({
      flow: 'stripe-checkout',
    });
    expect(mockedGetLogCommand).toHaveBeenCalledWith({
      flow: 'stripe-checkout',
      invocationId: 'inv_123',
    });
  });

  it('routes whoami to its command handler', async () => {
    const program = createProgram();

    program.exitOverride();
    program.configureOutput({
      writeErr: () => undefined,
      writeOut: () => undefined,
    });

    await program.parseAsync(['whoami'], { from: 'user' });

    expect(mockedWhoAmICommand).toHaveBeenCalledOnce();
  });

  it('requires --flow for secrets commands', async () => {
    const program = createProgram();

    vi.spyOn(process, 'exit').mockImplementation(((code?: string | number | null) => {
      throw new Error(`process.exit unexpectedly called with "${code ?? ''}"`);
    }) as typeof process.exit);
    vi.spyOn(process.stderr, 'write').mockImplementation(() => true);

    program.configureOutput({
      outputError: () => undefined,
      writeErr: () => undefined,
      writeOut: () => undefined,
    });

    await expect(
      program.parseAsync(['secrets', 'set', 'STRIPE_WEBHOOK_SECRET', '--value', 'x'], {
        from: 'user',
      }),
    ).rejects.toThrow('process.exit unexpectedly called with "1"');
    expect(mockedSetSecretCommand).not.toHaveBeenCalled();
  });

  it('requires --flow for logs commands', async () => {
    const program = createProgram();

    vi.spyOn(process, 'exit').mockImplementation(((code?: string | number | null) => {
      throw new Error(`process.exit unexpectedly called with "${code ?? ''}"`);
    }) as typeof process.exit);
    vi.spyOn(process.stderr, 'write').mockImplementation(() => true);

    program.configureOutput({
      outputError: () => undefined,
      writeErr: () => undefined,
      writeOut: () => undefined,
    });

    await expect(program.parseAsync(['logs', 'list'], { from: 'user' })).rejects.toThrow(
      'process.exit unexpectedly called with "1"',
    );
    expect(mockedListLogsCommand).not.toHaveBeenCalled();
  });
});
