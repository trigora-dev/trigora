import type { EventDefinition } from './execution';

export type DurableProgram<TInput = unknown, TResult = unknown> = ((
  input: TInput,
) => Promise<TResult>) & {
  name: string;
};

export function resolveProgramId<TInput = unknown, TResult = unknown>(
  program: DurableProgram<TInput, TResult> | string,
): string {
  if (typeof program === 'string') {
    const id = program.trim();

    if (!id) {
      throw new Error('Program id must be a non-empty string.');
    }

    return id;
  }

  if (typeof program !== 'function') {
    throw new Error('Expected a named program function or a program id string.');
  }

  if (!program.name) {
    throw new Error('Cannot use an anonymous program. Export a named async function.');
  }

  return program.name;
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
