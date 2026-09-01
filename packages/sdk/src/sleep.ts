import { getDurableRuntimeHost } from './runtimeHost';

export function sleep(duration: string | number): Promise<void> {
  return getDurableRuntimeHost().sleep(duration);
}
