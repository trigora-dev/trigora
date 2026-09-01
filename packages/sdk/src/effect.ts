import { getDurableRuntimeHost } from './runtimeHost';

export function effect<T>(run: () => T | Promise<T>): Promise<T>;
export function effect<T>(name: string, run: () => T | Promise<T>): Promise<T>;
export function effect<T>(
  nameOrRun: string | (() => T | Promise<T>),
  maybeRun?: () => T | Promise<T>,
): Promise<T> {
  if (typeof nameOrRun === 'function') {
    return getDurableRuntimeHost().effect(undefined, nameOrRun);
  }

  if (typeof maybeRun !== 'function') {
    throw new Error('effect(name, fn) requires a function.');
  }

  return getDurableRuntimeHost().effect(nameOrRun, maybeRun);
}
