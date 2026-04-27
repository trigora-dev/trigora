import type {
  CronFlowDefinition,
  FlowDefinition,
  JsonValue,
  ManualFlowDefinition,
  WebhookFlowDefinition,
} from '@trigora/contracts';

/**
 * Define a Trigora flow.
 *
 * Flows are plain TypeScript modules with three core parts:
 * - `id`: the source identifier for the flow in your project
 * - `trigger`: how the flow is invoked
 * - `run`: the function that executes when the flow is triggered
 *
 * `run` receives the incoming `event` and a `ctx` object with logging and environment access.
 *
 * Webhook flows can return HTTP-friendly values:
 * - `Response`
 * - plain objects / arrays / numbers / booleans
 * - `string`
 * - `null` or `undefined`
 *
 * Other trigger types generally do not need to return anything meaningful.
 *
 * @example
 * ```ts
 * export default defineFlow({
 *   id: 'hello',
 *   trigger: { type: 'webhook' },
 *   async run(event, ctx) {
 *     await ctx.log.info('Received event', event.payload);
 *     return { ok: true };
 *   },
 * });
 * ```
 */
export function defineFlow<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
>(flow: ManualFlowDefinition<TPayload, TEnv>): ManualFlowDefinition<TPayload, TEnv>;

export function defineFlow<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
>(flow: WebhookFlowDefinition<TPayload, TEnv>): WebhookFlowDefinition<TPayload, TEnv>;

export function defineFlow<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
>(flow: CronFlowDefinition<TPayload, TEnv>): CronFlowDefinition<TPayload, TEnv>;

export function defineFlow(flow: FlowDefinition): FlowDefinition {
  return flow;
}
