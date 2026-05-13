import type { FlowContext } from './context';
import type {
  CronEventPayload,
  CronFlowEvent,
  FlowEvent,
  ManualFlowEvent,
  WebhookFlowEvent,
} from './event';
import type { CronTrigger, ManualTrigger, Trigger, WebhookTrigger } from './trigger';

export type JsonObject = {
  [key: string]: JsonValue | undefined;
};

export type JsonValue = string | number | boolean | null | JsonObject | JsonValue[];

export type WebhookFlowResult = Response | JsonValue | undefined;

type FlowEventForTrigger<TPayload, TTrigger extends Trigger> = TTrigger extends ManualTrigger
  ? ManualFlowEvent<TPayload>
  : TTrigger extends WebhookTrigger
    ? WebhookFlowEvent<TPayload>
    : TTrigger extends CronTrigger
      ? CronFlowEvent
      : FlowEvent<TPayload>;

export type FlowRunFn<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
  TTrigger extends Trigger = Trigger,
> = (
  event: FlowEventForTrigger<TPayload, TTrigger>,
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

export type CronFlowDefinition<TEnv extends Record<string, string> = Record<string, string>> =
  BaseFlowDefinition & {
    trigger: CronTrigger;
    run: FlowRunFn<CronEventPayload, TEnv, CronTrigger>;
  };

type FlowDefinitionByTrigger<TPayload, TEnv extends Record<string, string>> =
  | ManualFlowDefinition<TPayload, TEnv>
  | WebhookFlowDefinition<TPayload, TEnv>
  | CronFlowDefinition<TEnv>;

type TriggerTypeOf<TTrigger extends Trigger> = TTrigger['type'];

export type FlowDefinition<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
  TTrigger extends Trigger = Trigger,
> = Extract<
  FlowDefinitionByTrigger<TPayload, TEnv>,
  { trigger: { type: TriggerTypeOf<TTrigger> } }
>;
