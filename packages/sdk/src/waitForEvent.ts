import type { EventDefinition, WaitForEventOptions } from '@trigora/contracts';

import { resolveEventName } from './event';
import { getDurableRuntimeHost } from './runtimeHost';

export type { WaitForEventOptions };

export function waitForEvent<TPayload>(
  eventOrName: EventDefinition<TPayload> | string,
  options?: WaitForEventOptions,
): Promise<TPayload> {
  return getDurableRuntimeHost().waitForEvent(resolveEventName(eventOrName), options);
}
