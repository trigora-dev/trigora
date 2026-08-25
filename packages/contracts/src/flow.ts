import type { FlowContext } from './context';
import type {
  CronEventPayload,
  CronFlowEvent,
  FlowEvent,
  ManualFlowEvent,
  QueueFlowEvent,
  WebhookFlowEvent,
} from './event';
import type { RetryPolicy } from './retry';
import type { CronTrigger, ManualTrigger, QueueTrigger, Trigger, WebhookTrigger } from './trigger';

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
      : TTrigger extends QueueTrigger
        ? QueueFlowEvent<TPayload>
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
  /**
   * Optional retry policy. Omitted means a single attempt.
   * `attempts > 1` is supported for queue flows only.
   */
  retry?: RetryPolicy;
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

export type QueueFlowDefinition<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
> = BaseFlowDefinition & {
  /**
   * Trigger configuration that determines how the flow is invoked.
   */
  trigger: QueueTrigger;
  /**
   * Function called when the flow runs.
   */
  run: FlowRunFn<TPayload, TEnv, QueueTrigger>;
};

type FlowDefinitionByTrigger<TPayload, TEnv extends Record<string, string>> =
  | ManualFlowDefinition<TPayload, TEnv>
  | WebhookFlowDefinition<TPayload, TEnv>
  | CronFlowDefinition<TEnv>
  | QueueFlowDefinition<TPayload, TEnv>;

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
  { trigger: { type: TriggerTypeOf<TTrigger> } }
>;
