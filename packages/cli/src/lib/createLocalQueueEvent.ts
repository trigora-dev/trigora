import { randomUUID } from 'node:crypto';

import type { JsonValue, QueueFlowEvent } from '@trigora/contracts';

export function createLocalQueueEvent<TPayload extends JsonValue = JsonValue>(options: {
  payload: TPayload;
  queue: string;
}): QueueFlowEvent<TPayload> {
  return {
    id: `evt_local_${Date.now()}`,
    type: 'queue',
    timestamp: new Date().toISOString(),
    payload: options.payload,
    queue: options.queue,
    messageId: `local_${randomUUID()}`,
  };
}
