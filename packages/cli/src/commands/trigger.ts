import type { JsonValue, ManualFlowDefinition, QueueFlowDefinition } from '@trigora/contracts';
import { createLocalContext } from '../lib/createLocalContext';
import { createLocalQueueEvent } from '../lib/createLocalQueueEvent';
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
  const loadedFlow = await loadFlowModule(options.filePath);

  if (loadedFlow.trigger.type !== 'manual' && loadedFlow.trigger.type !== 'queue') {
    throw new Error(
      `Flow "${loadedFlow.id}" uses trigger "${loadedFlow.trigger.type}". trigora trigger supports manual- and queue-triggered flows.`,
    );
  }

  const ctx = createLocalContext(loadedFlow.id);
  const payload = await loadPayload(options.payloadPath);

  printTriggerStart(loadedFlow.id);

  const startedAt = Date.now();

  try {
    if (loadedFlow.trigger.type === 'manual') {
      const flow = loadedFlow as ManualFlowDefinition;
      await flow.run(
        {
          id: `evt_local_${Date.now()}`,
          type: 'manual',
          timestamp: new Date().toISOString(),
          payload,
        },
        ctx,
      );
    } else {
      const flow = loadedFlow as QueueFlowDefinition;
      await flow.run(
        createLocalQueueEvent({
          payload,
          queue: flow.trigger.queue,
          maxAttempts: flow.retry?.attempts ?? 1,
        }),
        ctx,
      );
    }

    const durationMs = Date.now() - startedAt;
    printTriggerResult('Run complete', loadedFlow.id, durationMs);
  } catch (error) {
    const durationMs = Date.now() - startedAt;
    printTriggerFailure(loadedFlow.id, durationMs);

    if (error instanceof Error) {
      console.error(error.message);
      return;
    }

    console.error(error);
  }
}
