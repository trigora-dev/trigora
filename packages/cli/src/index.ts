import { triggerCommand } from './commands/trigger';

async function main(): Promise<void> {
  const [, , command, filePath] = process.argv;

  if (command === 'trigger') {
    if (!filePath) {
      console.error('Usage: trigora trigger <path-to-flow-file>');
      process.exit(1);
    }

    await triggerCommand({ filePath });
    return;
  }

  console.log('Trigora CLI');
  console.log('');
  console.log('Available commands:');
  console.log('  trigger <path-to-flow-file>');
}

main().catch((error) => {
  console.error('Unexpected CLI error');

  if (error instanceof Error) {
    console.error(error.message);
    process.exit(1);
  }

  console.error(error);
  process.exit(1);
});
