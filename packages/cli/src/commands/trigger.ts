import { createLocalContext } from '../lib/createLocalContext';
import { loadFlowModule } from '../lib/loadFlowModule';

type TriggerOptions = {
  filePath: string;
};

export async function triggerCommand(options: TriggerOptions): Promise<void> {
  const flow = await loadFlowModule(options.filePath);
  const ctx = createLocalContext();

  console.log(`[${flow.id}] execution started`);

  const startedAt = Date.now();

  try {
    await flow.run(
      {
        id: 'evt_local',
        type: 'manual',
        timestamp: new Date().toISOString(),
        payload: {},
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
