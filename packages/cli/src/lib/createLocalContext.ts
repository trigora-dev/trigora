import type { FlowContext, Logger } from '@trigora/contracts';
import { colors } from './colors';

function createLogger(flowId: string): Logger {
  const prefix = colors.flow(`[${flowId}]`);

  return {
    info(message, meta) {
      console.log(`${prefix} ${colors.info('INFO')} ${message}`, meta ?? '');
    },
    warn(message, meta) {
      console.warn(`${prefix} ${colors.warn('WARN')} ${message}`, meta ?? '');
    },
    error(message, meta) {
      console.error(`${prefix} ${colors.error('ERROR')} ${message}`, meta ?? '');
    },
  };
}

export function createLocalContext(flowId: string): FlowContext {
  return {
    env: {},
    log: createLogger(flowId),
  };
}
