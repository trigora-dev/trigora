export type FlowEvent<TPayload = unknown> = {
  id?: string;
  type?: string;
  timestamp?: string;
  payload: TPayload;
};
