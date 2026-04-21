import type { FlowDefinition, Trigger } from '@trigora/contracts';

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
 *     await ctx.log.info('Received event', { payload: event.payload });
 *     return { ok: true };
 *   },
 * });
 * ```
 */
export function defineFlow<
  TPayload = unknown,
  TEnv extends Record<string, string> = Record<string, string>,
  TTrigger extends Trigger = Trigger,
>(flow: FlowDefinition<TPayload, TEnv, TTrigger>): FlowDefinition<TPayload, TEnv, TTrigger> {
  return flow;
}
