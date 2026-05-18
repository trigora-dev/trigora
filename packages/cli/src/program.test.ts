import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createProgram } from './program';
import { deleteFlowCommand } from './commands/flows';
import { inspectInvocationCommand, listInvocationsCommand } from './commands/invocations';
import { getLogCommand } from './commands/logs';
import { deleteSecretCommand, listSecretsCommand, setSecretCommand } from './commands/secrets';
import { whoAmICommand } from './commands/whoami';

vi.mock('./commands/flows', () => ({
  deleteFlowCommand: vi.fn(),
}));

vi.mock('./commands/invocations', () => ({
  inspectInvocationCommand: vi.fn(),
  listInvocationsCommand: vi.fn(),
}));

vi.mock('./commands/logs', () => ({
  getLogCommand: vi.fn(),
}));

vi.mock('./commands/secrets', () => ({
  deleteSecretCommand: vi.fn(),
  listSecretsCommand: vi.fn(),
  setSecretCommand: vi.fn(),
}));

vi.mock('./commands/whoami', () => ({
  whoAmICommand: vi.fn(),
}));

const mockedDeleteFlowCommand = vi.mocked(deleteFlowCommand);
const mockedInspectInvocationCommand = vi.mocked(inspectInvocationCommand);
const mockedListInvocationsCommand = vi.mocked(listInvocationsCommand);
const mockedGetLogCommand = vi.mocked(getLogCommand);
const mockedDeleteSecretCommand = vi.mocked(deleteSecretCommand);
const mockedListSecretsCommand = vi.mocked(listSecretsCommand);
const mockedSetSecretCommand = vi.mocked(setSecretCommand);
const mockedWhoAmICommand = vi.mocked(whoAmICommand);

beforeEach(() => {
  mockedDeleteFlowCommand.mockReset();
  mockedInspectInvocationCommand.mockReset();
  mockedListInvocationsCommand.mockReset();
  mockedGetLogCommand.mockReset();
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

    await program.parseAsync(['secrets', '--flow', 'stripe-checkout'], { from: 'user' });
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

  it('routes invocations list options to their handler', async () => {
    const program = createProgram();

    program.exitOverride();
    program.configureOutput({
      writeErr: () => undefined,
      writeOut: () => undefined,
    });

    await program.parseAsync(
      ['invocations', '--flow', 'stripe-checkout', '--status', 'failed', '--range', '7d'],
      {
        from: 'user',
      },
    );

    expect(mockedListInvocationsCommand).toHaveBeenCalledWith({
      flow: 'stripe-checkout',
      range: '7d',
      status: 'failed',
    });
  });

  it('routes invocation inspect and logs commands to their handlers', async () => {
    const program = createProgram();

    program.exitOverride();
    program.configureOutput({
      writeErr: () => undefined,
      writeOut: () => undefined,
    });

    await program.parseAsync(['invocations', 'inspect', 'inv_123'], {
      from: 'user',
    });
    await program.parseAsync(['logs', 'inv_123'], { from: 'user' });

    expect(mockedInspectInvocationCommand).toHaveBeenCalledWith({
      invocationId: 'inv_123',
    });
    expect(mockedGetLogCommand).toHaveBeenCalledWith('inv_123');
  });

  it('routes flows delete to its handler', async () => {
    const program = createProgram();

    program.exitOverride();
    program.configureOutput({
      writeErr: () => undefined,
      writeOut: () => undefined,
    });

    await program.parseAsync(['flows', 'delete', 'hello', '--yes'], { from: 'user' });

    expect(mockedDeleteFlowCommand).toHaveBeenCalledWith('hello', {
      yes: true,
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

  it('requires --flow for secrets root command', async () => {
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

    await expect(program.parseAsync(['secrets'], { from: 'user' })).rejects.toThrow(
      'process.exit unexpectedly called with "1"',
    );
    expect(mockedListSecretsCommand).not.toHaveBeenCalled();
  });
});
