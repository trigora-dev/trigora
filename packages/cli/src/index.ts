import { Command } from 'commander';
import { deployCommand } from './commands/deploy';
import { devCommand } from './commands/dev';
import { disableFlowCommand, inspectFlowCommand, listFlowsCommand } from './commands/flows';
import { initCommand } from './commands/init';
import { triggerCommand } from './commands/trigger';
import { CliDisplayError, isCliDisplayError, renderCliError } from './lib/cliOutput';
import { loadProjectEnv } from './lib/loadProjectEnv';
import { resolveFlowPath } from './lib/resolveFlowPath';

const program = new Command();

loadProjectEnv();

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
  .argument('<flow>', 'Flow name or file path')
  .option('-p, --payload <path>', 'Path to JSON payload file')
  .action(async (flowNameOrPath, options) => {
    const filePath = resolveFlowPath(flowNameOrPath);

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

program.parseAsync(process.argv).catch((error: unknown) => {
  if (isCliDisplayError(error)) {
    renderCliError(error);
    process.exit(1);
  }

  const reason = error instanceof Error ? error.message : String(error);

  renderCliError(
    new CliDisplayError({
      title: 'Command failed',
      details: [{ label: 'Reason', value: reason }],
    }),
  );

  process.exit(1);
});
