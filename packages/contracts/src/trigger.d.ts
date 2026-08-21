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
 * Run a flow when a message is consumed from a workspace queue.
 *
 * Deploy auto-provisions/binds the named queue to this flow.
 */
export type QueueTrigger = {
  type: 'queue';
  /** Workspace-scoped queue name. Deploy auto-provisions/binds this queue. */
  queue: string;
};
/**
 * Trigger types supported by hosted deployments.
 */
export type HostedTrigger = WebhookTrigger | CronTrigger | QueueTrigger;
/**
 * All supported flow trigger types.
 */
export type Trigger = ManualTrigger | HostedTrigger;
