import type { JsonValue } from '@trigora/contracts';

import { CliDisplayError } from '../lib/cliOutput';
import { createCommandRuntime, withCloud } from '../lib/cloudRuntime';
import { parseJsonValue } from '../lib/loadJsonFile';
import { withRuntime } from '../lib/runtimeClient';
import { printExecutionRecord } from '../lib/runtimeOutput';

type SendOptions = {
  executionId: string;
  event: string;
  payload?: string;
};

async function readPayload(value?: string): Promise<JsonValue> {
  if (value === undefined) {
    return {};
  }

  try {
    return await parseJsonValue(value);
  } catch (error) {
    throw new CliDisplayError({
      title: 'Invalid payload',
      details: [{ label: 'Reason', value: error instanceof Error ? error.message : String(error) }],
    });
  }
}

export async function sendCommand(options: SendOptions): Promise<void> {
  const payload = await readPayload(options.payload);
  const runtime = createCommandRuntime();
  if (runtime.cloud) {
    const { execution } = await withCloud(() =>
      runtime.client.sendEvent(options.executionId, options.event, payload),
    );
    printExecutionRecord(execution);
    return;
  }

  const run = runtime.client.get(options.executionId);
  await withRuntime(() => run.send(options.event, payload));
  const execution = await withRuntime(() => runtime.client.getExecution(options.executionId));
  printExecutionRecord(execution);
}
