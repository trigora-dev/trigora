import { createCommandRuntime, withCloud } from '../lib/cloudRuntime';
import { withRuntime } from '../lib/runtimeClient';
import { printExecutionRecord } from '../lib/runtimeOutput';

export async function cancelCommand(executionId: string): Promise<void> {
  const runtime = createCommandRuntime();
  if (runtime.cloud) {
    const { execution } = await withCloud(() => runtime.client.cancelExecution(executionId));
    printExecutionRecord(execution);
    return;
  }

  await withRuntime(() => runtime.client.get(executionId).cancel());
  const execution = await withRuntime(() => runtime.client.getExecution(executionId));
  printExecutionRecord(execution);
}
