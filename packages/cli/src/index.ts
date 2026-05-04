import { CliDisplayError, isCliDisplayError, renderCliError } from './lib/cliOutput';
import { loadProjectEnv } from './lib/loadProjectEnv';
import { createProgram } from './program';

loadProjectEnv();

const program = createProgram();

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
