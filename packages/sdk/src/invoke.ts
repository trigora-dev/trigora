import type { DurableProgram } from './program';
import { resolveProgramId } from './program';
import { getDurableRuntimeHost } from './runtimeHost';

export function invoke<TInput, TResult>(
  program: DurableProgram<TInput, TResult> | string,
  input: TInput,
): Promise<TResult> {
  return getDurableRuntimeHost().invoke(resolveProgramId(program), input);
}
