import { Command } from 'commander';
import { deployCommand } from './commands/deploy';
import { devCommand } from './commands/dev';
import { deleteSecretCommand, listSecretsCommand, setSecretCommand } from './commands/secrets';
import { inspectInvocationCommand, listInvocationsCommand } from './commands/invocations';
import { getLogCommand } from './commands/logs';
import { whoAmICommand } from './commands/whoami';
import {
  deleteFlowCommand,
  disableFlowCommand,
  enableFlowCommand,
  inspectFlowCommand,
  listFlowsCommand,
} from './commands/flows';
import { initCommand } from './commands/init';
import { triggerCommand } from './commands/trigger';
import { resolveDefaultDevFlowPath } from './lib/resolveDefaultDevFlowPath';
import { resolveFlowPath } from './lib/resolveFlowPath';

export function createProgram(): Command {
  const program = new Command();

  program.name('trigora').description('Run code when things happen').version('0.1.0');

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
    .command('trigger')
    .argument('<flow>', "The flow identifier defined in defineFlow({ id: '...' }) or a file path")
    .option('-p, --payload <path>', 'Path to JSON payload file')
    .action(async (flowNameOrPath, options) => {
      const filePath = resolveFlowPath(flowNameOrPath);

      await triggerCommand({
        filePath,
        payloadPath: options.payload,
      });
    });

  program
    .command('dev')
    .argument('[flow]', "The flow identifier defined in defineFlow({ id: '...' }) or a file path")
    .option('-p, --payload <path>', 'Path to JSON payload file')
    .action(async (flowNameOrPath, options) => {
      const filePath = flowNameOrPath
        ? resolveFlowPath(flowNameOrPath)
        : await resolveDefaultDevFlowPath();

      await devCommand({
        filePath,
        payloadPath: options.payload,
      });
    });

  program
    .command('deploy')
    .argument('[flow]', "The flow identifier defined in defineFlow({ id: '...' }) or a file path")
    .action(async (flowNameOrPath) => {
      const filePath = flowNameOrPath ? resolveFlowPath(flowNameOrPath) : undefined;

      await deployCommand({
        filePath,
      });
    });

  const flowsCommand = program.command('flows').description('Manage deployed flows');

  flowsCommand.action(async () => {
    await listFlowsCommand();
  });

  program
    .command('whoami')
    .description('Show the authenticated workspace and deploy token')
    .action(async () => {
      await whoAmICommand();
    });

  flowsCommand
    .command('inspect')
    .argument('<flow>', "The flow identifier defined in defineFlow({ id: '...' })")
    .action(async (flow) => {
      await inspectFlowCommand(flow);
    });

  flowsCommand
    .command('disable')
    .argument('<flow>', "The flow identifier defined in defineFlow({ id: '...' })")
    .action(async (flow) => {
      await disableFlowCommand(flow);
    });

  flowsCommand
    .command('enable')
    .argument('<flow>', "The flow identifier defined in defineFlow({ id: '...' })")
    .action(async (flow) => {
      await enableFlowCommand(flow);
    });

  flowsCommand
    .command('delete')
    .argument('<flow>', "The flow identifier defined in defineFlow({ id: '...' })")
    .option('-y, --yes', 'Skip confirmation prompt')
    .action(async (flow, options) => {
      await deleteFlowCommand(flow, { yes: options.yes });
    });

  const secretsCommand = program
    .command('secrets')
    .description('Manage hosted flow secrets')
    .enablePositionalOptions()
    .option('--flow <flow>', "The flow identifier defined in defineFlow({ id: '...' })");

  secretsCommand.action(async (options, command) => {
    if (!options.flow) {
      command.error("required option '--flow <flow>' not specified");
    }

    await listSecretsCommand({
      flow: options.flow,
    });
  });

  secretsCommand
    .command('set')
    .argument('<name>', 'Secret name')
    .option('--value <value>', 'Secret value for non-interactive use')
    .action(async (name, options, command) => {
      const flow = options.flow ?? command.parent?.opts().flow;

      if (!flow) {
        command.error("required option '--flow <flow>' not specified");
      }

      await setSecretCommand({
        flow,
        name,
        value: options.value,
      });
    });

  secretsCommand
    .command('delete')
    .argument('<name>', 'Secret name')
    .option('-y, --yes', 'Skip confirmation prompt')
    .action(async (name, options, command) => {
      const flow = options.flow ?? command.parent?.opts().flow;

      if (!flow) {
        command.error("required option '--flow <flow>' not specified");
      }

      await deleteSecretCommand({
        flow,
        name,
        yes: options.yes,
      });
    });

  const invocationsCommand = program
    .command('invocations')
    .description('Inspect hosted flow invocations');

  invocationsCommand
    .option('--flow <flow>', "The flow identifier defined in defineFlow({ id: '...' })")
    .option('--status <status>', 'Invocation status filter')
    .option('--range <range>', 'Time range filter like 7d or 24h')
    .action(async (options) => {
      await listInvocationsCommand({
        flow: options.flow,
        range: options.range,
        status: options.status,
      });
    });

  invocationsCommand
    .command('inspect')
    .argument('<invocation>', 'Invocation ID')
    .action(async (invocationId) => {
      await inspectInvocationCommand({
        invocationId,
      });
    });

  program
    .command('logs')
    .description('Show logs for a single invocation')
    .argument('<invocation>', 'Invocation ID')
    .action(async (invocationId) => {
      await getLogCommand(invocationId);
    });

  return program;
}
