import type { FlowDefinition, Trigger } from '@trigora/contracts';

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
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

      return {
        type: 'webhook',
        event: typeof trigger.event === 'string' ? trigger.event : undefined,
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

    default:
      throw new Error(
        `Invalid flow in "${filePath}": unsupported trigger type "${trigger.type}". Expected "manual", "webhook", or "cron".`,
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

  return {
    id: value.id,
    trigger,
    run: value.run as FlowDefinition['run'],
  };
}
