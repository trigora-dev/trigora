import type { JsonValue } from '@trigora/contracts';

import { CliDisplayError } from '../lib/cliOutput';
import { createCommandRuntime, withCloud } from '../lib/cloudRuntime';
import { parseJsonValue } from '../lib/loadJsonFile';
import { withRuntime } from '../lib/runtimeClient';
import { printExecutionRecord } from '../lib/runtimeOutput';

type StartOptions = {
  programId: string;
  input?: string;
};

async function readInput(value?: string): Promise<JsonValue> {
  if (value === undefined) {
    return {};
  }

  try {
    return await parseJsonValue(value);
  } catch (error) {
    throw new CliDisplayError({
      title: 'Invalid input',
      details: [{ label: 'Reason', value: error instanceof Error ? error.message : String(error) }],
    });
  }
}

export async function startCommand(options: StartOptions): Promise<void> {
  const input = await readInput(options.input);
  const runtime = createCommandRuntime();
  if (runtime.cloud) {
    const { execution } = await withCloud(() =>
      runtime.client.startExecution({
        programId: options.programId,
        input,
      }),
    );
    printExecutionRecord(execution);
    return;
  }

  const run = await withRuntime(() => runtime.client.start(options.programId, input));
  const execution = await withRuntime(() => runtime.client.getExecution(run.id));
  printExecutionRecord(execution);
}
