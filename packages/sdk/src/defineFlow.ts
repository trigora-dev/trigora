import type { FlowDefinition } from '@trigora/contracts';

export function defineFlow<
  TPayload = unknown,
  TEnv extends Record<string, string> = Record<string, string>,
>(flow: FlowDefinition<TPayload, TEnv>): FlowDefinition<TPayload, TEnv> {
  return flow;
}
