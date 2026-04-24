# Events

Each flow receives an event object as the first argument to `run`.

## Shape

At minimum, every flow receives a payload:

```ts
type FlowEvent<TPayload = unknown> = {
  payload: TPayload;
  id?: string;
  type?: string;
  timestamp?: string;
};
```

## Payload

`payload` is the main data your flow works with.

Example:

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('Received payload', event.payload);
  },
});
```

## Metadata

The event may also include:

- `id`
- `type`
- `timestamp`

These fields are useful when present, but your flow should primarily rely on the payload it receives.

## Working With Typed Payloads

You can use TypeScript to model the payload shape your flow expects:

```ts
import { defineFlow } from '@trigora/sdk';

type GreetingPayload = {
  message: string;
};

export default defineFlow<GreetingPayload>({
  id: 'greet',
  trigger: { type: 'webhook' },
  async run(event) {
    return event.payload.message;
  },
});
```

For manual and cron flows, the return value itself is not used. For webhook flows, return values can shape the response.
