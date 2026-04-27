# Events

Each flow receives an event object as the first argument to `run`.

## Shape

At minimum, every flow receives a payload:

```ts
type FlowEvent<TPayload = JsonValue> = {
  payload: TPayload;
  id?: string;
  type?: string;
  timestamp?: string;
  request?: {
    headers: Record<string, string>;
    method: string;
    url: string;
    rawBody: string;
  };
};
```

## Payload

`payload` is the main data your flow works with.

If you do not provide your own payload type, `event.payload` defaults to `JsonValue`.

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
- `request` for webhook flows

These fields are useful when present, but your flow should primarily rely on the payload it receives.

For webhook flows, `event.request` gives you request metadata such as headers, method, URL, and the raw request body. `event.request.rawBody` is especially useful when you want to verify webhook signatures yourself.

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
