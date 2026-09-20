import { createCommandRuntime, withCloud } from '../lib/cloudRuntime';
import { withRuntime } from '../lib/runtimeClient';
import { printProgramsTable } from '../lib/runtimeOutput';

export async function listProgramsCommand(): Promise<void> {
  const runtime = createCommandRuntime();
  const listed = runtime.cloud
    ? await withCloud(() => runtime.client.listPrograms())
    : await withRuntime(() => runtime.client.listPrograms());

  printProgramsTable(
    listed.programs.map((program) => ({
      id: program.name,
      language: program.language ?? 'unknown',
      file: program.currentVersionId ?? '',
    })),
  );
}
