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
export type Trigger = ManualTrigger | WebhookTrigger | CronTrigger;
