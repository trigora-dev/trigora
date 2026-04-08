import fs from 'node:fs/promises';
import { createLocalContext } from '../lib/createLocalContext';
import { loadFlowModule } from '../lib/loadFlowModule';

type TriggerOptions = {
  filePath: string;
  payloadPath?: string;
};

async function loadPayload(path?: string) {
  if (!path) return {};

  const raw = await fs.readFile(path, 'utf-8');

  try {
    return JSON.parse(raw);
  } catch {
    throw new Error(`Invalid JSON in payload file: ${path}`);
  }
}

export async function triggerCommand(options: TriggerOptions): Promise<void> {
  const flow = await loadFlowModule(options.filePath);
  const ctx = createLocalContext(flow.id);
  const payload = await loadPayload(options.payloadPath);

  console.log(`[${flow.id}] execution started`);

  const startedAt = Date.now();

  try {
    await flow.run(
      {
        id: `evt_local_${Date.now()}`,
        type: 'manual',
        timestamp: new Date().toISOString(),
        payload,
      },
      ctx,
    );

    const durationMs = Date.now() - startedAt;
    console.log(`[${flow.id}] execution succeeded (${durationMs}ms)`);
  } catch (error) {
    const durationMs = Date.now() - startedAt;
    console.error(`[${flow.id}] execution failed (${durationMs}ms)`);

    if (error instanceof Error) {
      console.error(error.message);
      return;
    }

    console.error(error);
  }
}
