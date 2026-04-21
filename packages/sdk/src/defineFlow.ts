import type { FlowDefinition, Trigger } from '@trigora/contracts';

export function defineFlow<
  TPayload = unknown,
  TEnv extends Record<string, string> = Record<string, string>,
  TTrigger extends Trigger = Trigger,
>(flow: FlowDefinition<TPayload, TEnv, TTrigger>): FlowDefinition<TPayload, TEnv, TTrigger> {
  return flow;
}
