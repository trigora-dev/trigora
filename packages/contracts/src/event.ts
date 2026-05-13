import type { JsonValue } from './flow';

export type FlowRequest = {
  headers: Record<string, string>;
  method: string;
  url: string;
  rawBody: string;
};

type BaseFlowEvent = {
  id?: string;
  type?: string;
  timestamp?: string;
};

export type CronEventPayload = {
  cron: string;
  scheduledAt: string;
  timezone: 'UTC';
};

export type FlowEvent<TPayload = JsonValue> = BaseFlowEvent & {
  payload: TPayload;
  request?: FlowRequest;
};

export type ManualFlowEvent<TPayload = JsonValue> = BaseFlowEvent & {
  payload: TPayload;
  request?: undefined;
  type?: 'manual';
};

export type WebhookFlowEvent<TPayload = JsonValue> = BaseFlowEvent & {
  payload: TPayload;
  request?: FlowRequest;
  type?: 'webhook';
};

export type CronFlowEvent = BaseFlowEvent & {
  payload: CronEventPayload;
  request?: undefined;
  type?: 'cron';
};
