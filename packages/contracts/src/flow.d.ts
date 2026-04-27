import type { FlowContext } from './context';
import type { FlowEvent } from './event';
import type { Trigger } from './trigger';
export type JsonObject = {
  [key: string]: JsonValue | undefined;
};
export type JsonValue = string | number | boolean | null | JsonObject | JsonValue[];
export type FlowRunFn<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
> = (event: FlowEvent<TPayload>, ctx: FlowContext<TEnv>) => Promise<void> | void;
export type FlowDefinition<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
> = {
  id: string;
  trigger: Trigger;
  run: FlowRunFn<TPayload, TEnv>;
};
