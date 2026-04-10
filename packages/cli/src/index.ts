import { Command } from 'commander';
import { deployCommand } from './commands/deploy';
import { devCommand } from './commands/dev';
import { initCommand } from './commands/init';
import { triggerCommand } from './commands/trigger';
import { colors } from './lib/colors';
import { resolveFlowPath } from './lib/resolveFlowPath';

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

program.parseAsync(process.argv).catch((error: unknown) => {
  console.error('');

  const errorPrefix = colors.error('[error]');
  const name = colors.flow('trigora');

  console.error(`${errorPrefix} ${name} command failed`);

  if (error instanceof Error) {
    console.error(error.message);
  } else {
    console.error(error);
  }

  process.exit(1);
});
