import type { FlowContext, Logger } from '@trigora/contracts';

function createLogger(): Logger {
  return {
    info(message, meta) {
      if (meta !== undefined) {
        console.log(`[info] ${message}`, meta);
        return;
      }

      console.log(`[info] ${message}`);
    },

    warn(message, meta) {
      if (meta !== undefined) {
        console.warn(`[warn] ${message}`, meta);
        return;
      }

      console.warn(`[warn] ${message}`);
    },

    error(message, meta) {
      if (meta !== undefined) {
        console.error(`[error] ${message}`, meta);
        return;
      }

      console.error(`[error] ${message}`);
    },
  };
}

export function createLocalContext(): FlowContext {
  return {
    env: {},
    log: createLogger(),
  };
}
