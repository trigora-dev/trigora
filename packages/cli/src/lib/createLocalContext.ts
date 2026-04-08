import type { FlowContext, Logger } from '@trigora/contracts';

function createLogger(flowId: string): Logger {
  return {
    info(message, meta) {
      if (meta !== undefined) {
        console.log(`[${flowId}] INFO ${message}`, meta);
        return;
      }

      console.log(`[${flowId}] INFO ${message}`);
    },

    warn(message, meta) {
      if (meta !== undefined) {
        console.warn(`[${flowId}] WARN ${message}`, meta);
        return;
      }

      console.warn(`[${flowId}] WARN ${message}`);
    },

    error(message, meta) {
      if (meta !== undefined) {
        console.error(`[${flowId}] ERROR ${message}`, meta);
        return;
      }

      console.error(`[${flowId}] ERROR ${message}`);
    },
  };
}

export function createLocalContext(flowId: string): FlowContext {
  return {
    env: {},
    log: createLogger(flowId),
  };
}
