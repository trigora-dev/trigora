import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createProgram } from './program';
import { deleteSecretCommand, listSecretsCommand, setSecretCommand } from './commands/secrets';

vi.mock('./commands/secrets', () => ({
  deleteSecretCommand: vi.fn(),
  listSecretsCommand: vi.fn(),
  setSecretCommand: vi.fn(),
}));

const mockedDeleteSecretCommand = vi.mocked(deleteSecretCommand);
const mockedListSecretsCommand = vi.mocked(listSecretsCommand);
const mockedSetSecretCommand = vi.mocked(setSecretCommand);

beforeEach(() => {
  mockedDeleteSecretCommand.mockReset();
  mockedListSecretsCommand.mockReset();
  mockedSetSecretCommand.mockReset();
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
      [
        'secrets',
        'set',
        'STRIPE_WEBHOOK_SECRET',
        '--flow',
        '402c04b0-62c8-4d0b-942f-0ee2329436a8',
        '--value',
        'x',
      ],
      { from: 'user' },
    );

    expect(mockedSetSecretCommand).toHaveBeenCalledWith({
      flowId: '402c04b0-62c8-4d0b-942f-0ee2329436a8',
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

    await program.parseAsync(
      ['secrets', 'list', '--flow', '402c04b0-62c8-4d0b-942f-0ee2329436a8'],
      { from: 'user' },
    );
    await program.parseAsync(
      [
        'secrets',
        'delete',
        'STRIPE_WEBHOOK_SECRET',
        '--flow',
        '402c04b0-62c8-4d0b-942f-0ee2329436a8',
        '--yes',
      ],
      { from: 'user' },
    );

    expect(mockedListSecretsCommand).toHaveBeenCalledWith({
      flowId: '402c04b0-62c8-4d0b-942f-0ee2329436a8',
    });
    expect(mockedDeleteSecretCommand).toHaveBeenCalledWith({
      flowId: '402c04b0-62c8-4d0b-942f-0ee2329436a8',
      name: 'STRIPE_WEBHOOK_SECRET',
      yes: true,
    });
  });

  it('requires --flow for secrets commands', async () => {
    const program = createProgram();

    vi.spyOn(process, 'exit').mockImplementation(((code?: string | number | null) => {
      throw new Error(`process.exit unexpectedly called with "${code ?? ''}"`);
    }) as typeof process.exit);

    program.configureOutput({
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
});
