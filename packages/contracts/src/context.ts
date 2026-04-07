export type LogLevel = 'info' | 'warn' | 'error';

export type Logger = {
  info(message: string, meta?: unknown): Promise<void> | void;
  warn(message: string, meta?: unknown): Promise<void> | void;
  error(message: string, meta?: unknown): Promise<void> | void;
};

export type FlowContext<TEnv extends Record<string, string> = Record<string, string>> = {
  env: TEnv;
  log: Logger;
};
