import type {
  CronEventPayload,
  CronFlowDefinition,
  FlowDefinition,
  FlowRunFn,
  JsonValue,
  ManualFlowDefinition,
  ManualTrigger,
  Trigger,
  WebhookTrigger,
  WebhookFlowDefinition,
  CronTrigger,
} from '@trigora/contracts';

type DefineFlowInput<TPayload, TEnv extends Record<string, string>, TTrigger extends Trigger> = {
  id: string;
  trigger: TTrigger;
  run: TTrigger extends ManualTrigger
    ? FlowRunFn<TPayload, TEnv, ManualTrigger>
    : TTrigger extends WebhookTrigger
      ? FlowRunFn<TPayload, TEnv, WebhookTrigger>
      : TTrigger extends CronTrigger
        ? FlowRunFn<CronEventPayload, TEnv, CronTrigger>
        : never;
};

type DefineFlowOutput<
  TPayload,
  TEnv extends Record<string, string>,
  TTrigger extends Trigger,
> = TTrigger extends ManualTrigger
  ? ManualFlowDefinition<TPayload, TEnv>
  : TTrigger extends WebhookTrigger
    ? WebhookFlowDefinition<TPayload, TEnv>
    : TTrigger extends CronTrigger
      ? CronFlowDefinition<TEnv>
      : FlowDefinition<TPayload, TEnv, TTrigger>;

/**
 * Define a Trigora flow.
 *
 * Flows are plain TypeScript modules with three core parts:
 * - `id`: the internal identifier for the flow in your project
 * - `trigger`: how the flow is invoked
 * - `run`: the function that executes when the flow is triggered
 *
 * `run` receives the incoming `event` and a `ctx` object with logging and environment access.
 *
 * For webhook flows, `trigger.route` controls the public hosted path and defaults to `/${id}`.
 * This is separate from the flow `id`, which is still what the CLI uses for commands like
 * `trigora deploy hello` or `trigora flows inspect hello`.
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
 *   id: 'stripe-webhook',
 *   trigger: { type: 'webhook', route: '/hooks/stripe' },
 *   async run(event, ctx) {
 *     await ctx.log.info('Received event', event.payload);
 *     return { ok: true };
 *   },
 * });
 * ```
 */
export function defineFlow<
  TTrigger extends Trigger,
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
>(flow: DefineFlowInput<TPayload, TEnv, TTrigger>): DefineFlowOutput<TPayload, TEnv, TTrigger>;

export function defineFlow<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
>(flow: ManualFlowDefinition<TPayload, TEnv>): ManualFlowDefinition<TPayload, TEnv>;

export function defineFlow<
  TPayload = JsonValue,
  TEnv extends Record<string, string> = Record<string, string>,
>(flow: WebhookFlowDefinition<TPayload, TEnv>): WebhookFlowDefinition<TPayload, TEnv>;

export function defineFlow<TEnv extends Record<string, string> = Record<string, string>>(
  flow: CronFlowDefinition<TEnv>,
): CronFlowDefinition<TEnv>;

export function defineFlow(flow: unknown): unknown {
  return flow;
}
