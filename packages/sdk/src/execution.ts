import { getDurableRuntimeHost, type ExecutionInfo } from './runtimeHost';

export type { ExecutionInfo };

export const execution: ExecutionInfo = {
  get id() {
    return getDurableRuntimeHost().getExecution().id;
  },
  get attempt() {
    return getDurableRuntimeHost().getExecution().attempt;
  },
  get programId() {
    return getDurableRuntimeHost().getExecution().programId;
  },
  get signal() {
    return getDurableRuntimeHost().getExecution().signal;
  },
};
