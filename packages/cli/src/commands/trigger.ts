import type { JsonValue } from '@trigora/contracts';
import { createLocalContext } from '../lib/createLocalContext';
import { colors } from '../lib/colors';
import { loadJsonFile } from '../lib/loadJsonFile';
import { loadFlowModule } from '../lib/loadFlowModule';

type TriggerOptions = {
  filePath: string;
  payloadPath?: string;
};

async function loadPayload(filePath?: string): Promise<JsonValue> {
  if (!filePath) return {};

  return loadJsonFile(filePath);
}

function formatFlowName(flowId: string): string {
  return colors.flow(colors.heading(`"${flowId}"`));
}

function printTriggerStart(flowId: string): void {
  console.log(colors.label(`Running flow ${formatFlowName(flowId)}...`));
}

function printTriggerResult(title: string, flowId: string, durationMs: number): void {
  console.log('');
  console.log(`${colors.success('✔')} ${title}`);
  console.log('');
  console.log(`${colors.label('Flow'.padEnd(8))} ${formatFlowName(flowId)}`);
  console.log(`${colors.label('Duration'.padEnd(8))} ${durationMs}ms`);
}

function printTriggerFailure(flowId: string, durationMs: number): void {
  console.error('');
  console.error(`${colors.error('✖')} Run failed`);
  console.error('');
  console.error(`${colors.label('Flow'.padEnd(8))} ${formatFlowName(flowId)}`);
  console.error(`${colors.label('Duration'.padEnd(8))} ${durationMs}ms`);
}

export async function triggerCommand(options: TriggerOptions): Promise<void> {
  const flow = await loadFlowModule(options.filePath);
  const ctx = createLocalContext(flow.id);
  const payload = await loadPayload(options.payloadPath);

  const event = {
    id: `evt_local_${Date.now()}`,
    type: 'manual' as const,
    timestamp: new Date().toISOString(),
    payload,
  };

  printTriggerStart(flow.id);

  const startedAt = Date.now();

  try {
    await flow.run(event, ctx);

    const durationMs = Date.now() - startedAt;
    printTriggerResult('Run complete', flow.id, durationMs);
  } catch (error) {
    const durationMs = Date.now() - startedAt;
    printTriggerFailure(flow.id, durationMs);

    if (error instanceof Error) {
      console.error(error.message);
      return;
    }

    console.error(error);
  }
}
