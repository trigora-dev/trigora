import { Command } from 'commander';
import { triggerCommand } from './commands/trigger';
import { devCommand } from './commands/dev';
import { resolveFlowPath } from './lib/resolveFlowPath';

const program = new Command();

program.name('trigora').description('Run code when things happen').version('0.0.0');

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

program.parse();
