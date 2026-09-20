import { getDurableRuntimeHost } from './runtimeHost';

export function effect<T>(name: string, run: () => T | Promise<T>): Promise<T> {
  if (typeof name !== 'string' || name.trim() === '') {
    throw new Error('effect(name, fn) requires a non-empty string key.');
  }

  if (typeof run !== 'function') {
    throw new Error('effect(name, fn) requires a function.');
  }

  return getDurableRuntimeHost().effect(name, run);
}
