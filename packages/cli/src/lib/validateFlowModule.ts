import type { FlowDefinition, RetryPolicy, Trigger } from '@trigora/contracts';

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function normalizeWebhookRoute(route: string, filePath: string): `/${string}` {
  const normalizedRoute = route.trim();

  if (normalizedRoute.length === 0) {
    throw new Error(
      `Invalid flow in "${filePath}": "trigger.route" must be a non-empty string when provided.`,
    );
  }

  if (!normalizedRoute.startsWith('/')) {
    throw new Error(`Invalid flow in "${filePath}": "trigger.route" must start with "/".`);
  }

  if (normalizedRoute !== '/' && normalizedRoute.endsWith('/')) {
    throw new Error(
      `Invalid flow in "${filePath}": "trigger.route" must not end with "/" unless the route is "/".`,
    );
  }

  if (normalizedRoute.includes('//')) {
    throw new Error(
      `Invalid flow in "${filePath}": "trigger.route" must not contain empty path segments.`,
    );
  }

  return normalizedRoute as `/${string}`;
}

function validateRetry(
  retry: unknown,
  trigger: Trigger,
  filePath: string,
): RetryPolicy | undefined {
  if (retry === undefined) {
    return undefined;
  }

  if (!isObject(retry)) {
    throw new Error(`Invalid flow in "${filePath}": "retry" must be an object when provided.`);
  }

  if (
    typeof retry.attempts !== 'number' ||
    !Number.isInteger(retry.attempts) ||
    retry.attempts < 1 ||
    retry.attempts > 20
  ) {
    throw new Error(
      `Invalid flow in "${filePath}": "retry.attempts" must be an integer between 1 and 20.`,
    );
  }

  if (retry.backoff !== 'exponential') {
    throw new Error(
      `Invalid flow in "${filePath}": "retry.backoff" must be "exponential" when provided.`,
    );
  }

  if (retry.attempts > 1 && trigger.type !== 'queue') {
    throw new Error(
      `Invalid flow in "${filePath}": "retry.attempts" greater than 1 is only supported for queue flows.`,
    );
  }

  return {
    attempts: retry.attempts,
    backoff: 'exponential',
  };
}

function validateTrigger(trigger: unknown, filePath: string): Trigger {
  if (!isObject(trigger)) {
    throw new Error(`Invalid flow in "${filePath}": "trigger" must be an object.`);
  }

  if (typeof trigger.type !== 'string') {
    throw new Error(`Invalid flow in "${filePath}": "trigger.type" must be a string.`);
  }

  switch (trigger.type) {
    case 'manual':
      return { type: 'manual' };

    case 'webhook': {
      if ('event' in trigger && trigger.event !== undefined && typeof trigger.event !== 'string') {
        throw new Error(
          `Invalid flow in "${filePath}": "trigger.event" must be a string when provided.`,
        );
      }

      if ('route' in trigger && trigger.route !== undefined && typeof trigger.route !== 'string') {
        throw new Error(
          `Invalid flow in "${filePath}": "trigger.route" must be a string when provided.`,
        );
      }

      return {
        type: 'webhook',
        event: typeof trigger.event === 'string' ? trigger.event : undefined,
        route:
          typeof trigger.route === 'string'
            ? normalizeWebhookRoute(trigger.route, filePath)
            : undefined,
      };
    }

    case 'cron': {
      if (typeof trigger.cron !== 'string' || trigger.cron.trim().length === 0) {
        throw new Error(
          `Invalid flow in "${filePath}": cron triggers require a non-empty "trigger.cron" string.`,
        );
      }

      return {
        type: 'cron',
        cron: trigger.cron,
      };
    }

    case 'queue': {
      if (typeof trigger.queue !== 'string' || trigger.queue.trim().length === 0) {
        throw new Error(
          `Invalid flow in "${filePath}": queue triggers require a non-empty "trigger.queue" string.`,
        );
      }

      return {
        type: 'queue',
        queue: trigger.queue.trim(),
      };
    }

    default:
      throw new Error(
        `Invalid flow in "${filePath}": unsupported trigger type "${trigger.type}". Expected "manual", "webhook", "cron", or "queue".`,
      );
  }
}

export function validateFlowModule(filePath: string, value: unknown): FlowDefinition {
  if (value === undefined) {
    throw new Error(`No default export found in "${filePath}". Expected a default exported flow.`);
  }

  if (!isObject(value)) {
    throw new Error(`Invalid flow in "${filePath}": default export must be an object.`);
  }

  if (typeof value.id !== 'string' || value.id.trim().length === 0) {
    throw new Error(`Invalid flow in "${filePath}": "id" must be a non-empty string.`);
  }

  if (typeof value.run !== 'function') {
    throw new Error(`Invalid flow in "${filePath}": "run" must be a function.`);
  }

  const trigger = validateTrigger(value.trigger, filePath);
  const retry = validateRetry(value.retry, trigger, filePath);

  switch (trigger.type) {
    case 'manual':
      return {
        id: value.id,
        trigger,
        ...(retry ? { retry } : {}),
        run: value.run as FlowDefinition<unknown, Record<string, string>, typeof trigger>['run'],
      };
    case 'webhook':
      return {
        id: value.id,
        trigger,
        ...(retry ? { retry } : {}),
        run: value.run as FlowDefinition<unknown, Record<string, string>, typeof trigger>['run'],
      };
    case 'cron':
      return {
        id: value.id,
        trigger,
        ...(retry ? { retry } : {}),
        run: value.run as FlowDefinition<unknown, Record<string, string>, typeof trigger>['run'],
      };
    case 'queue':
      return {
        id: value.id,
        trigger,
        ...(retry ? { retry } : {}),
        run: value.run as FlowDefinition<unknown, Record<string, string>, typeof trigger>['run'],
      };
  }
}
