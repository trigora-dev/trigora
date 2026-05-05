import { Command } from 'commander';
import { deployCommand } from './commands/deploy';
import { devCommand } from './commands/dev';
import { deleteSecretCommand, listSecretsCommand, setSecretCommand } from './commands/secrets';
import { getLogCommand, listLogsCommand } from './commands/logs';
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
    .argument('<flow>', 'Flow name or file path')
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
    .argument('[flow]', 'Flow name or file path')
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
    .argument('[flow]', 'Flow name or file path')
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

  flowsCommand
    .command('inspect')
    .argument('<flowId>', 'Deployed flow ID')
    .action(async (flowId) => {
      await inspectFlowCommand(flowId);
    });

  flowsCommand
    .command('disable')
    .argument('<flowId>', 'Deployed flow ID')
    .action(async (flowId) => {
      await disableFlowCommand(flowId);
    });

  flowsCommand
    .command('enable')
    .argument('<flowId>', 'Deployed flow ID')
    .action(async (flowId) => {
      await enableFlowCommand(flowId);
    });

  const secretsCommand = program.command('secrets').description('Manage hosted flow secrets');

  secretsCommand
    .command('set')
    .argument('<name>', 'Secret name')
    .requiredOption('--flow <flowId>', 'Hosted flow ID')
    .option('--value <value>', 'Secret value for non-interactive use')
    .action(async (name, options) => {
      await setSecretCommand({
        flowId: options.flow,
        name,
        value: options.value,
      });
    });

  secretsCommand
    .command('list')
    .requiredOption('--flow <flowId>', 'Hosted flow ID')
    .action(async (options) => {
      await listSecretsCommand({
        flowId: options.flow,
      });
    });

  secretsCommand
    .command('delete')
    .argument('<name>', 'Secret name')
    .requiredOption('--flow <flowId>', 'Hosted flow ID')
    .option('-y, --yes', 'Skip confirmation prompt')
    .action(async (name, options) => {
      await deleteSecretCommand({
        flowId: options.flow,
        name,
        yes: options.yes,
      });
    });

  const logsCommand = program.command('logs').description('Inspect hosted flow invocations');

  logsCommand
    .command('list')
    .requiredOption('--flow <flowId>', 'Hosted flow ID')
    .action(async (options) => {
      await listLogsCommand({
        flowId: options.flow,
      });
    });

  logsCommand
    .command('get')
    .argument('<invocationId>', 'Invocation ID')
    .requiredOption('--flow <flowId>', 'Hosted flow ID')
    .action(async (invocationId, options) => {
      await getLogCommand({
        flowId: options.flow,
        invocationId,
      });
    });

  return program;
}
