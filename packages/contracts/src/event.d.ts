import type { JsonValue } from './flow';
export type FlowRequest = {
  headers: Record<string, string>;
  method: string;
  url: string;
  rawBody: string;
};
type BaseFlowEvent<TType extends string = string> = {
  id: string;
  type: TType;
  timestamp: string;
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
export type ManualFlowEvent<TPayload = JsonValue> = BaseFlowEvent<'manual'> & {
  payload: TPayload;
};
export type WebhookFlowEvent<TPayload = JsonValue> = BaseFlowEvent & {
  payload: TPayload;
  request: FlowRequest;
};
export type CronFlowEvent = BaseFlowEvent<'cron'> & {
  payload: CronEventPayload;
};
export type QueueEventPayload = JsonValue;
export type QueueFlowEvent<TPayload = JsonValue> = BaseFlowEvent<'queue'> & {
  payload: TPayload;
  queue: string;
  messageId: string;
  /**
   * 1-based delivery attempt count for this message.
   */
  attempt: number;
  /**
   * Maximum attempts from the flow retry policy, or `1` when retry is omitted.
   */
  maxAttempts: number;
};
export {};
