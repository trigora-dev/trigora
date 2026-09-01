import type { EventDefinition } from '@trigora/contracts';

export type { EventDefinition };

export function event<TPayload = unknown>(name: string): EventDefinition<TPayload> {
  const trimmed = name.trim();

  if (!trimmed) {
    throw new Error('Event name must be a non-empty string.');
  }

  return { name: trimmed };
}

export function resolveEventName(eventOrName: EventDefinition | string): string {
  if (typeof eventOrName === 'string') {
    const name = eventOrName.trim();

    if (!name) {
      throw new Error('Event name must be a non-empty string.');
    }

    return name;
  }

  if (!eventOrName || typeof eventOrName.name !== 'string' || !eventOrName.name.trim()) {
    throw new Error('Expected an event definition or event name string.');
  }

  return eventOrName.name;
}
