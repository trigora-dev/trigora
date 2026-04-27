import { createLocalContext } from '../lib/createLocalContext';
import { colors } from '../lib/colors';
import { loadJsonFile } from '../lib/loadJsonFile';
import { loadFlowModule } from '../lib/loadFlowModule';

type TriggerOptions = {
  filePath: string;
  payloadPath?: string;
};

async function loadPayload(filePath?: string) {
  if (!filePath) return {};

  return loadJsonFile(filePath);
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

  const prefix = colors.flow(`[${flow.id}]`);
  const runLabel = colors.run('RUN');

  console.log(`${prefix} ${runLabel} starting`);

  const startedAt = Date.now();

  try {
    await flow.run(event, ctx);

    const durationMs = Date.now() - startedAt;
    console.log(`${prefix} ${runLabel} ${colors.success('succeeded')} (${durationMs}ms)`);
  } catch (error) {
    const durationMs = Date.now() - startedAt;
    console.error(`${prefix} ${runLabel} ${colors.error('failed')} (${durationMs}ms)`);

    if (error instanceof Error) {
      console.error(error.message);
      return;
    }

    console.error(error);
  }
}
