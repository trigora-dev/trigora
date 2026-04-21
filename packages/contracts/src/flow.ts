import type { FlowContext } from './context';
import type { FlowEvent } from './event';
import type { Trigger, WebhookTrigger } from './trigger';

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | { [key: string]: JsonValue }
  | JsonValue[];

export type WebhookFlowResult = Response | JsonValue | undefined;

export type FlowRunFn<
  TPayload = unknown,
  TEnv extends Record<string, string> = Record<string, string>,
  TTrigger extends Trigger = Trigger,
> = (
  event: FlowEvent<TPayload>,
  ctx: FlowContext<TEnv>,
) => TTrigger extends WebhookTrigger
  ? Promise<WebhookFlowResult> | WebhookFlowResult
  : Promise<void> | void;

export type FlowDefinition<
  TPayload = unknown,
  TEnv extends Record<string, string> = Record<string, string>,
  TTrigger extends Trigger = Trigger,
> = {
  id: string;
  trigger: TTrigger;
  run: FlowRunFn<TPayload, TEnv, TTrigger>;
};
