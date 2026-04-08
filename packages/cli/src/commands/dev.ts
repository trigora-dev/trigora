import fs from 'node:fs';
import { triggerCommand } from './trigger';

type DevOptions = {
  filePath: string;
  payloadPath?: string;
};

export async function devCommand(options: DevOptions): Promise<void> {
  console.log(`[dev] watching ${options.filePath}`);

  let isRunning = false;
  let rerunRequested = false;
  let shuttingDown = false;

  const watchers: fs.FSWatcher[] = [];

  async function run() {
    if (shuttingDown) return;

    if (isRunning) {
      rerunRequested = true;
      return;
    }

    isRunning = true;

    try {
      console.log('[dev] running flow');
      await triggerCommand(options);
    } finally {
      isRunning = false;

      if (rerunRequested && !shuttingDown) {
        rerunRequested = false;
        await run();
      }
    }
  }

  function shutdown() {
    if (shuttingDown) return;

    shuttingDown = true;

    for (const watcher of watchers) {
      watcher.close();
    }

    process.off('SIGINT', handleSignals);
    process.off('SIGTERM', handleSignals);

    console.log('\n[dev] stopped');
    process.exit(0);
  }

  const handleSignals = () => {
    shutdown();
  };

  function watchFile(filePath: string, label: string) {
    const watcher = fs.watch(filePath, (eventType) => {
      if (eventType !== 'change' || shuttingDown) return;

      void (async () => {
        try {
          console.log(`[dev] ${label} changed, re-running...`);
          await run();
        } catch (error) {
          console.error('[dev] failed during re-run');

          if (error instanceof Error) {
            console.error(error.message);
            return;
          }

          console.error(error);
        }
      })();
    });

    watchers.push(watcher);
  }

  process.on('SIGINT', handleSignals);
  process.on('SIGTERM', handleSignals);

  await run();

  watchFile(options.filePath, 'flow');

  if (options.payloadPath) {
    watchFile(options.payloadPath, 'payload');
  }

  await new Promise<void>(() => {});
}
