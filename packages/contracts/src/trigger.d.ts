export type ManualTrigger = {
  type: 'manual';
};
export type WebhookTrigger = {
  type: 'webhook';
  event?: string;
};
export type CronTrigger = {
  type: 'cron';
  cron: string;
};
export type HostedTrigger = WebhookTrigger | CronTrigger;
export type Trigger = ManualTrigger | HostedTrigger;
