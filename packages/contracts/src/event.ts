import type { JsonValue } from './flow';

export type FlowRequest = {
  headers: Record<string, string>;
  method: string;
  url: string;
  rawBody: string;
};

export type FlowEvent<TPayload = JsonValue> = {
  id?: string;
  type?: string;
  timestamp?: string;
  payload: TPayload;
  request?: FlowRequest;
};
