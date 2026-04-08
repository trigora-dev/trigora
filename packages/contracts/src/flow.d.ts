import type { FlowContext } from './context';
import type { FlowEvent } from './event';
import type { Trigger } from './trigger';
export type FlowRunFn<
  TPayload = unknown,
  TEnv extends Record<string, string> = Record<string, string>,
> = (event: FlowEvent<TPayload>, ctx: FlowContext<TEnv>) => Promise<void> | void;
export type FlowDefinition<
  TPayload = unknown,
  TEnv extends Record<string, string> = Record<string, string>,
> = {
  id: string;
  trigger: Trigger;
  run: FlowRunFn<TPayload, TEnv>;
};
