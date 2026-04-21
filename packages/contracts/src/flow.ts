import type { FlowContext } from './context';
import type { FlowEvent } from './event';
import type { Trigger, WebhookTrigger } from './trigger';

/**
 * JSON-compatible values that can be safely serialized for webhook responses.
 */
export type JsonValue =
  | string
  | number
  | boolean
  | null
  | { [key: string]: JsonValue }
  | JsonValue[];

/**
 * Values a webhook flow can return to shape the HTTP response.
 */
export type WebhookFlowResult = Response | JsonValue | undefined;

/**
 * The function executed when a flow runs.
 *
 * - Webhook flows may return HTTP-friendly values.
 * - Other trigger types usually return `void`.
 */
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

/**
 * A Trigora flow definition.
 *
 * - `id` identifies the source flow in your project
 * - `trigger` describes how the flow starts
 * - `run` contains the flow logic
 */
export type FlowDefinition<
  TPayload = unknown,
  TEnv extends Record<string, string> = Record<string, string>,
  TTrigger extends Trigger = Trigger,
> = {
  /**
   * Source identifier for the flow in your project.
   */
  id: string;
  /**
   * Trigger configuration that determines how the flow is invoked.
   */
  trigger: TTrigger;
  /**
   * Function called when the flow is triggered.
   */
  run: FlowRunFn<TPayload, TEnv, TTrigger>;
};
