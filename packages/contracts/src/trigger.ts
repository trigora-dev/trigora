/**
 * Run a flow manually, typically during local development or testing.
 */
export type ManualTrigger = {
  type: 'manual';
};

/**
 * Run a flow from an incoming HTTP request.
 *
 * `event` can be used to describe a webhook or event name when needed.
 */
export type WebhookTrigger = {
  type: 'webhook';
  event?: string;
};

/**
 * Run a flow on a schedule using a cron expression.
 */
export type CronTrigger = {
  type: 'cron';
  cron: string;
};

/**
 * All supported flow trigger types.
 */
export type Trigger = ManualTrigger | WebhookTrigger | CronTrigger;
