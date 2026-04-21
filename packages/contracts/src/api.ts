export type ApiErrorCode =
  | 'bad_request'
  | 'conflict'
  | 'deployment_not_found'
  | 'forbidden'
  | 'internal_error'
  | 'not_found'
  | 'rate_limited'
  | 'unauthorized';

export type ApiErrorStep =
  | 'uploading_package'
  | 'worker_creation'
  | 'dispatch_setup'
  | 'activating';

export type ApiErrorResponse = {
  error: {
    code: ApiErrorCode;
    message: string;
    step?: ApiErrorStep;
    details?: unknown;
  };
};

export type FlowTriggerType = 'webhook' | 'cron' | 'queue';

export type FlowStatus = 'ready' | 'disabled' | 'failed';

type BaseFlowRecord = {
  createdAt: string;
  id: string;
  name: string;
  status: FlowStatus;
};

export type WebhookFlowRecord = BaseFlowRecord & {
  endpoint: string;
  route?: string;
  trigger: 'webhook';
};

export type CronFlowRecord = BaseFlowRecord & {
  schedule?: string;
  trigger: 'cron';
};

export type QueueFlowRecord = BaseFlowRecord & {
  queue?: string;
  trigger: 'queue';
};

export type FlowRecord = WebhookFlowRecord | CronFlowRecord | QueueFlowRecord;

export type ListFlowsResponse = {
  flows: FlowRecord[];
};

export type GetFlowResponse = {
  flow: FlowRecord;
};

export type DisableFlowResponse = {
  ok: true;
  flow: {
    id: string;
    status: FlowStatus;
  };
};
