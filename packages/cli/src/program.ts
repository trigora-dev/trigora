import { Command } from 'commander';
import { cancelCommand } from './commands/cancel';
import { deployCommand } from './commands/deploy';
import { devCommand } from './commands/dev';
import { inspectExecutionCommand, listExecutionsCommand } from './commands/executions';
import { initCommand } from './commands/init';
import { listProgramsCommand } from './commands/programs';
import { sendCommand } from './commands/send';
import { startCommand } from './commands/start';
import { whoAmICommand } from './commands/whoami';

export function createProgram(): Command {
  const program = new Command();

  program.name('trigora').description('Local durable execution runtime').version('0.9.0');

  program
    .command('init')
    .description('Initialize a new Trigora project')
    .option('-f, --force', 'Overwrite existing files')
    .action(async (options) => {
      await initCommand({
        force: options.force,
      });
    });

  program
    .command('dev')
    .description('Start the local durable execution runtime')
    .option('--host <host>', 'Local runtime host')
    .option('--port <port>', 'Local runtime port')
    .action(async (options) => {
      const port = options.port === undefined ? undefined : Number(options.port);

      if (port !== undefined && (!Number.isInteger(port) || port <= 0)) {
        throw new Error('`--port` must be a positive integer.');
      }

      await devCommand({
        host: options.host,
        port,
      });
    });

  program
    .command('deploy')
    .description('Compile programs locally and deploy them to Trigora Cloud')
    .option('--program <name>', 'Deploy a single program')
    .action(async (options) => {
      await deployCommand({
        program: options.program,
      });
    });

  program
    .command('programs')
    .description('List programs')
    .action(async () => {
      await listProgramsCommand();
    });

  const executionsCommand = program.command('executions').description('List executions');

  executionsCommand.action(async () => {
    await listExecutionsCommand();
  });

  executionsCommand
    .command('inspect')
    .description('Inspect an execution')
    .argument('<execution>', 'Execution ID')
    .action(async (executionId) => {
      await inspectExecutionCommand(executionId);
    });

  program
    .command('start')
    .description('Start a program execution')
    .argument('<program>', 'Program id')
    .option('--input <json>', 'JSON input or path to a JSON file')
    .action(async (programId, options) => {
      await startCommand({
        programId,
        input: options.input,
      });
    });

  program
    .command('send')
    .description('Send an event to a waiting execution')
    .argument('<execution>', 'Execution ID')
    .argument('<event>', 'Event name')
    .option('--payload <json>', 'JSON payload or path to a JSON file')
    .action(async (executionId, event, options) => {
      await sendCommand({
        executionId,
        event,
        payload: options.payload,
      });
    });

  program
    .command('cancel')
    .description('Cancel an execution')
    .argument('<execution>', 'Execution ID')
    .action(async (executionId) => {
      await cancelCommand(executionId);
    });

  program
    .command('whoami')
    .description('Show the authenticated workspace and API token')
    .action(async () => {
      await whoAmICommand();
    });

  return program;
}
