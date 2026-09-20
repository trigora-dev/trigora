export type ApiErrorCode =
  | 'bad_request'
  | 'conflict'
  | 'forbidden'
  | 'internal_error'
  | 'not_found'
  | 'rate_limited'
  | 'unauthorized'
  | 'compiler_failed'
  | 'event_mismatch'
  | 'execution_not_cancellable'
  | 'execution_not_found'
  | 'execution_not_waiting'
  | 'invalid_input'
  | 'program_not_found'
  | 'project_not_found';

export type ApiError = {
  code: ApiErrorCode;
  message: string;
  details?: unknown;
};

export type ApiErrorResponse = {
  error: ApiError;
};
