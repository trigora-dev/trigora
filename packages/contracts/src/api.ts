export type ApiErrorCode =
  | 'bad_request'
  | 'conflict'
  | 'deployment_not_found'
  | 'forbidden'
  | 'internal_error'
  | 'invalid_cron_expression'
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
  trigger: 'cron';
  schedule: string;
  timezone: 'UTC';
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

export type FlowStatusResponse = {
  ok: true;
  flow: {
    id: string;
    name: string;
    status: FlowStatus;
  };
};

export type FlowSecretRecord = {
  createdAt: string;
  name: string;
  updatedAt: string;
};

export type ListFlowSecretsResponse = {
  secrets: FlowSecretRecord[];
};

export type SetFlowSecretRequest = {
  name: string;
  value: string;
};

export type SetFlowSecretResponse = {
  ok: true;
  secret: FlowSecretRecord;
};

export type DeleteFlowSecretResponse = {
  ok: true;
  deleted: true;
  name: string;
};

export type FlowInvocationStatus = 'running' | 'succeeded' | 'failed';

export type FlowInvocationLogLevel = 'info' | 'warn' | 'error';

export type FlowInvocationRecord = {
  id: string;
  status: FlowInvocationStatus;
  startedAt: string;
  completedAt: string | null;
  durationMs: number | null;
  httpStatus: number | null;
  errorCode: string | null;
  errorMessage: string | null;
};

export type FlowInvocationLogRecord = {
  sequence: number;
  level: FlowInvocationLogLevel;
  message: string;
  timestamp: string;
  metadata?: Record<string, unknown> | null;
};

export type ListFlowInvocationsResponse = {
  invocations: FlowInvocationRecord[];
};

export type GetFlowInvocationResponse = {
  invocation: FlowInvocationRecord & {
    logs: FlowInvocationLogRecord[];
  };
};

export type ListFlowInvocationsQuery = {
  limit?: number;
  status?: FlowInvocationStatus;
};
