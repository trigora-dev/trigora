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
/**
 * JSON-compatible values that can be safely serialized for webhook responses.
 */
export type JsonValue = string | number | boolean | null | JsonObject | JsonValue[];
/**
 * Values a webhook flow can return to shape the HTTP response.
 */
export type WebhookFlowResult = Response | JsonValue | undefined;
type FlowEventForTrigger<TPayload, TTrigger extends Trigger> = TTrigger extends ManualTrigger
  ? ManualFlowEvent<TPayload>
  : TTrigger extends WebhookTrigger
    ? WebhookFlowEvent<TPayload>
    : TTrigger extends CronTrigger
      ? CronFlowEvent
      : FlowEvent<TPayload>;
/**
 * The function executed when a flow runs.
 *
 * - Webhook flows may return HTTP-friendly values.
 * - Other trigger types usually return `void`.
 */
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
  /**
   * Source identifier for the flow in your project.
   */
  id: string;
};
export type ManualFlowDefinition<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
> = BaseFlowDefinition & {
  /**
   * Trigger configuration that determines how the flow is invoked.
   */
  trigger: ManualTrigger;
  /**
   * Function called when the flow runs.
   */
  run: FlowRunFn<TPayload, TEnv, ManualTrigger>;
};
export type WebhookFlowDefinition<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
> = BaseFlowDefinition & {
  /**
   * Trigger configuration that determines how the flow is invoked.
   */
  trigger: WebhookTrigger;
  /**
   * Function called when the flow runs.
   */
  run: FlowRunFn<TPayload, TEnv, WebhookTrigger>;
};
export type CronFlowDefinition<TEnv extends Record<string, string> = Record<string, string>> =
  BaseFlowDefinition & {
    /**
     * Trigger configuration that determines how the flow is invoked.
     */
    trigger: CronTrigger;
    /**
     * Function called when the flow runs.
     */
    run: FlowRunFn<CronEventPayload, TEnv, CronTrigger>;
  };
type FlowDefinitionByTrigger<TPayload, TEnv extends Record<string, string>> =
  | ManualFlowDefinition<TPayload, TEnv>
  | WebhookFlowDefinition<TPayload, TEnv>
  | CronFlowDefinition<TEnv>;
type TriggerTypeOf<TTrigger extends Trigger> = TTrigger['type'];
/**
 * A Trigora flow definition.
 *
 * - `id` identifies the source flow in your project
 * - `trigger` describes how the flow starts
 * - `run` contains the flow logic
 */
export type FlowDefinition<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
  TTrigger extends Trigger = Trigger,
> = Extract<
  FlowDefinitionByTrigger<TPayload, TEnv>,
  {
    trigger: {
      type: TriggerTypeOf<TTrigger>;
    };
  }
>;
export {};
