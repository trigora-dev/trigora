import { createCommandRuntime, withCloud } from '../lib/cloudRuntime';
import { printExecutionRecord, printExecutionsTable } from '../lib/runtimeOutput';
import { withRuntime } from '../lib/runtimeClient';

export async function listExecutionsCommand(): Promise<void> {
  const runtime = createCommandRuntime();
  const listed = runtime.cloud
    ? await withCloud(() => runtime.client.listExecutions())
    : await withRuntime(() => runtime.client.listExecutions());
  printExecutionsTable(listed.executions);
}

export async function inspectExecutionCommand(executionId: string): Promise<void> {
  const runtime = createCommandRuntime();
  const execution = runtime.cloud
    ? await withCloud(() => runtime.client.getExecution(executionId))
    : await withRuntime(() => runtime.client.getExecution(executionId));
  printExecutionRecord(execution);
}
