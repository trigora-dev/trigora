import type { FlowContext } from './context';
import type { FlowEvent } from './event';
import type { CronTrigger, ManualTrigger, Trigger, WebhookTrigger } from './trigger';

export type JsonObject = {
  [key: string]: JsonValue | undefined;
};

export type JsonValue = string | number | boolean | null | JsonObject | JsonValue[];

export type WebhookFlowResult = Response | JsonValue | undefined;

export type FlowRunFn<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
  TTrigger extends Trigger = Trigger,
> = (
  event: FlowEvent<TPayload>,
  ctx: FlowContext<TEnv>,
) => TTrigger extends WebhookTrigger
  ? Promise<WebhookFlowResult> | WebhookFlowResult
  : Promise<void> | void;

type BaseFlowDefinition = {
  id: string;
};

export type ManualFlowDefinition<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
> = BaseFlowDefinition & {
  trigger: ManualTrigger;
  run: FlowRunFn<TPayload, TEnv, ManualTrigger>;
};

export type WebhookFlowDefinition<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
> = BaseFlowDefinition & {
  trigger: WebhookTrigger;
  run: FlowRunFn<TPayload, TEnv, WebhookTrigger>;
};

export type CronFlowDefinition<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
> = BaseFlowDefinition & {
  trigger: CronTrigger;
  run: FlowRunFn<TPayload, TEnv, CronTrigger>;
};

type FlowDefinitionByTrigger<TPayload, TEnv extends Record<string, string>> =
  | ManualFlowDefinition<TPayload, TEnv>
  | WebhookFlowDefinition<TPayload, TEnv>
  | CronFlowDefinition<TPayload, TEnv>;

type TriggerTypeOf<TTrigger extends Trigger> = TTrigger['type'];

export type FlowDefinition<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
  TTrigger extends Trigger = Trigger,
> = Extract<
  FlowDefinitionByTrigger<TPayload, TEnv>,
  { trigger: { type: TriggerTypeOf<TTrigger> } }
>;
