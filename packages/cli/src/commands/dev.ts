import fs from 'node:fs';
import path from 'node:path';

import { colors } from '../lib/colors';
import { triggerCommand } from './trigger';

type DevOptions = {
  filePath: string;
  payloadPath?: string;
};

export async function devCommand(options: DevOptions): Promise<void> {
  const relativeFlowPath = path.relative(process.cwd(), options.filePath);
  const relativePayloadPath = options.payloadPath
    ? path.relative(process.cwd(), options.payloadPath)
    : undefined;

  const devPrefix = colors.dev('[dev]');

  let isRunning = false;
  let rerunRequested = false;
  let shuttingDown = false;

  const watchers: fs.FSWatcher[] = [];
  const debounceTimers = new Map<string, NodeJS.Timeout>();

  function printDivider() {
    console.log('');
    console.log('────────── rerun ──────────');
  }

  function printWatchingState() {
    console.log(`${devPrefix} watching for changes...`);
  }

  async function run(reason?: string) {
    if (shuttingDown) return;

    if (isRunning) {
      rerunRequested = true;
      return;
    }

    isRunning = true;

    try {
      if (reason) {
        console.log(`${devPrefix} ${colors.warn(reason)}`);
      }

      printDivider();
      await triggerCommand(options);
    } finally {
      isRunning = false;

      if (!shuttingDown) {
        printWatchingState();
      }

      if (rerunRequested && !shuttingDown) {
        rerunRequested = false;
        await run('queued changes detected → re-running...');
      }
    }
  }

  function shutdown() {
    if (shuttingDown) return;
    shuttingDown = true;

    for (const watcher of watchers) {
      watcher.close();
    }

    for (const timer of debounceTimers.values()) {
      clearTimeout(timer);
    }

    process.off('SIGINT', handleSignals);
    process.off('SIGTERM', handleSignals);

    console.log(`\n${devPrefix} stopped`);
    process.exit(0);
  }

  const handleSignals = () => shutdown();

  function scheduleRerun(key: string, reason: string) {
    const existingTimer = debounceTimers.get(key);
    if (existingTimer) {
      clearTimeout(existingTimer);
    }

    const timer = setTimeout(() => {
      debounceTimers.delete(key);

      void (async () => {
        try {
          await run(reason);
        } catch (error) {
          console.error(`${devPrefix} ${colors.error('re-run failed')}`);

          if (error instanceof Error) {
            console.error(error.message);
            return;
          }

          console.error(error);
        }
      })();
    }, 100);

    debounceTimers.set(key, timer);
  }

  function watchFile(filePath: string, label: string) {
    const watcher = fs.watch(filePath, (eventType) => {
      if (eventType !== 'change' || shuttingDown) return;

      scheduleRerun(filePath, `${label} changed → re-running...`);
    });

    watchers.push(watcher);
  }

  process.on('SIGINT', handleSignals);
  process.on('SIGTERM', handleSignals);

  console.log(`${devPrefix} watching flow: ${relativeFlowPath}`);

  if (relativePayloadPath) {
    console.log(`${devPrefix} watching payload: ${relativePayloadPath}`);
  }

  console.log(`${devPrefix} ready`);

  await run();

  watchFile(options.filePath, 'flow');

  if (options.payloadPath) {
    watchFile(options.payloadPath, 'payload');
  }

  await new Promise<void>(() => {});
}