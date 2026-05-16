import { Command } from 'commander';
import { deployCommand } from './commands/deploy';
import { devCommand } from './commands/dev';
import { deleteSecretCommand, listSecretsCommand, setSecretCommand } from './commands/secrets';
import { getLogCommand, listLogsCommand } from './commands/logs';
import { whoAmICommand } from './commands/whoami';
import {
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

  const secretsCommand = program.command('secrets').description('Manage hosted flow secrets');

  secretsCommand
    .command('set')
    .argument('<name>', 'Secret name')
    .requiredOption('--flow <flow>', "The flow identifier defined in defineFlow({ id: '...' })")
    .option('--value <value>', 'Secret value for non-interactive use')
    .action(async (name, options) => {
      await setSecretCommand({
        flow: options.flow,
        name,
        value: options.value,
      });
    });

  secretsCommand
    .command('list')
    .requiredOption('--flow <flow>', "The flow identifier defined in defineFlow({ id: '...' })")
    .action(async (options) => {
      await listSecretsCommand({
        flow: options.flow,
      });
    });

  secretsCommand
    .command('delete')
    .argument('<name>', 'Secret name')
    .requiredOption('--flow <flow>', "The flow identifier defined in defineFlow({ id: '...' })")
    .option('-y, --yes', 'Skip confirmation prompt')
    .action(async (name, options) => {
      await deleteSecretCommand({
        flow: options.flow,
        name,
        yes: options.yes,
      });
    });

  const logsCommand = program.command('logs').description('Inspect hosted flow invocations');

  logsCommand
    .command('list')
    .requiredOption('--flow <flow>', "The flow identifier defined in defineFlow({ id: '...' })")
    .action(async (options) => {
      await listLogsCommand({
        flow: options.flow,
      });
    });

  logsCommand
    .command('get')
    .argument('<invocationId>', 'Invocation ID')
    .requiredOption('--flow <flow>', "The flow identifier defined in defineFlow({ id: '...' })")
    .action(async (invocationId, options) => {
      await getLogCommand({
        flow: options.flow,
        invocationId,
      });
    });

  return program;
}
