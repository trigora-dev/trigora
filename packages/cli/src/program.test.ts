import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createProgram } from './program';
import { cancelCommand } from './commands/cancel';
import { deployCommand } from './commands/deploy';
import { inspectExecutionCommand, listExecutionsCommand } from './commands/executions';
import { listProgramsCommand } from './commands/programs';
import { sendCommand } from './commands/send';
import { startCommand } from './commands/start';
import { whoAmICommand } from './commands/whoami';

vi.mock('./commands/cancel', () => ({
  cancelCommand: vi.fn(),
}));

vi.mock('./commands/deploy', () => ({
  deployCommand: vi.fn(),
}));

vi.mock('./commands/executions', () => ({
  inspectExecutionCommand: vi.fn(),
  listExecutionsCommand: vi.fn(),
}));

vi.mock('./commands/programs', () => ({
  listProgramsCommand: vi.fn(),
}));

vi.mock('./commands/send', () => ({
  sendCommand: vi.fn(),
}));

vi.mock('./commands/start', () => ({
  startCommand: vi.fn(),
}));

vi.mock('./commands/whoami', () => ({
  whoAmICommand: vi.fn(),
}));

const mockedCancelCommand = vi.mocked(cancelCommand);
const mockedDeployCommand = vi.mocked(deployCommand);
const mockedInspectExecutionCommand = vi.mocked(inspectExecutionCommand);
const mockedListExecutionsCommand = vi.mocked(listExecutionsCommand);
const mockedListProgramsCommand = vi.mocked(listProgramsCommand);
const mockedSendCommand = vi.mocked(sendCommand);
const mockedStartCommand = vi.mocked(startCommand);
const mockedWhoAmICommand = vi.mocked(whoAmICommand);

function createTestProgram() {
  const program = createProgram();
  program.exitOverride();
  program.configureOutput({
    writeErr: () => undefined,
    writeOut: () => undefined,
  });
  return program;
}

beforeEach(() => {
  mockedCancelCommand.mockReset();
  mockedDeployCommand.mockReset();
  mockedInspectExecutionCommand.mockReset();
  mockedListExecutionsCommand.mockReset();
  mockedListProgramsCommand.mockReset();
  mockedSendCommand.mockReset();
  mockedStartCommand.mockReset();
  mockedWhoAmICommand.mockReset();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('createProgram', () => {
  it('exposes the Program/Execution surface, deploy, and whoami', () => {
    const names = createProgram().commands.map((command) => command.name());

    expect(names).toEqual(
      expect.arrayContaining([
        'init',
        'dev',
        'deploy',
        'programs',
        'executions',
        'start',
        'send',
        'cancel',
        'whoami',
      ]),
    );
    expect(names).not.toEqual(
      expect.arrayContaining(['flows', 'queues', 'invocations', 'secrets', 'logs']),
    );
  });

  it('routes programs and executions commands', async () => {
    const program = createTestProgram();

    await program.parseAsync(['programs'], { from: 'user' });
    await program.parseAsync(['executions'], { from: 'user' });
    await program.parseAsync(['executions', 'inspect', 'exec_1'], { from: 'user' });

    expect(mockedListProgramsCommand).toHaveBeenCalledOnce();
    expect(mockedListExecutionsCommand).toHaveBeenCalledOnce();
    expect(mockedInspectExecutionCommand).toHaveBeenCalledWith('exec_1');
  });

  it('routes start, send, and cancel against the local runtime', async () => {
    const program = createTestProgram();

    await program.parseAsync(['start', 'approval', '--input', '{"ok":true}'], { from: 'user' });
    await program.parseAsync(['send', 'exec_1', 'approved', '--payload', '"ok"'], { from: 'user' });
    await program.parseAsync(['cancel', 'exec_1'], { from: 'user' });

    expect(mockedStartCommand).toHaveBeenCalledWith({
      programId: 'approval',
      input: '{"ok":true}',
    });
    expect(mockedSendCommand).toHaveBeenCalledWith({
      executionId: 'exec_1',
      event: 'approved',
      payload: '"ok"',
    });
    expect(mockedCancelCommand).toHaveBeenCalledWith('exec_1');
  });

  it('routes whoami to its command handler', async () => {
    const program = createTestProgram();

    await program.parseAsync(['whoami'], { from: 'user' });

    expect(mockedWhoAmICommand).toHaveBeenCalledOnce();
  });

  it('routes deploy to its command handler', async () => {
    const program = createTestProgram();

    await program.parseAsync(['deploy', '--program', 'approval'], { from: 'user' });

    expect(mockedDeployCommand).toHaveBeenCalledWith({
      program: 'approval',
    });
  });
});
