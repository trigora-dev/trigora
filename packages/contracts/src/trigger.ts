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
 * `route` controls the public hosted webhook path and is optional.
 * When omitted, hosted ingress defaults to `/${flow.id}`.
 */
export type WebhookRoute = `/${string}`;

export type WebhookTrigger = {
  type: 'webhook';
  event?: string;
  route?: WebhookRoute;
};

/**
 * Run a flow on a schedule using a cron expression.
 */
export type CronTrigger = {
  type: 'cron';
  cron: string;
};

/**
 * Trigger types supported by hosted deployments.
 */
export type HostedTrigger = WebhookTrigger | CronTrigger;

/**
 * All supported flow trigger types.
 */
export type Trigger = ManualTrigger | HostedTrigger;
