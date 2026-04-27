# @trigora/sdk

Flow authoring for Trigora.

`@trigora/sdk` is the package most Trigora users write against directly. It provides `defineFlow` plus the shared runtime types needed to author flows with good TypeScript ergonomics.

## Install

```bash
npm install @trigora/sdk
```

Most projects will install the CLI alongside it:

```bash
npm install trigora @trigora/sdk
```

## Basic Usage

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('Hello from Trigora', event.payload);
  },
});
```

## What `defineFlow` Gives You

`defineFlow` helps with:

- typed flow definitions
- typed trigger variants
- typed event payload access
- typed runtime context
- trigger-aware return types
- compile-time validation of trigger shape

The trigger typing is strict. If you mix properties from different trigger kinds, TypeScript will flag it.

For example, this is invalid:

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'broken',
  trigger: {
    type: 'webhook',
    cron: '* * * * *',
  },
  async run() {
    return 'nope';
  },
});
```

## Trigger Types

### Manual

Use manual triggers for local invocation and testing.

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('Triggered manually', event.payload);
  },
});
```

### Webhook

Use webhook triggers for hosted HTTP entrypoints.

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'ping',
  trigger: { type: 'webhook' },
  async run() {
    return 'pong';
  },
});
```

You can also include an optional event name:

```ts
trigger: { type: 'webhook', event: 'orders.created' }
```

### Cron

Use cron triggers for scheduled work.

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'nightly-sync',
  trigger: { type: 'cron', cron: '0 2 * * *' },
  async run(event, ctx) {
    await ctx.log.info('Nightly sync started', event.payload);
  },
});
```

## Return Types

Return values are trigger-aware.

### Webhook flows

Webhook flows can return HTTP-friendly values directly from `run`.

Supported return values:

- `Response`
- plain objects, arrays, numbers, and booleans
- `string`
- `null`
- `undefined`

Example:

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'status',
  trigger: { type: 'webhook' },
  async run() {
    return {
      ok: true,
      service: 'trigora',
    };
  },
});
```

### Manual and cron flows

Manual and cron flows do not use return values. Their `run` functions are typed as `void`.

This matches how Trigora uses them:

- manual flows are invoked locally for testing and development
- cron flows are background-style scheduled jobs
- webhook flows are request-response flows

## Event Shape

Flows receive an event object:

```ts
type FlowEvent<TPayload = JsonValue> = {
  id: string;
  type: string;
  timestamp: string;
  payload: TPayload;
  request?: {
    headers: Record<string, string>;
    method: string;
    url: string;
    rawBody: string;
  };
};
```

In local manual runs, the payload comes from your JSON payload file when one is provided. In local webhook dev and deployed webhook flows, the payload is the parsed JSON request body.

If you do not provide your own payload type, `event.payload` defaults to `JsonValue`.

For webhook flows, request metadata may also be available on `event.request`, including headers, method, URL, and the raw request body. `event.request.rawBody` is useful when you want to verify webhook signatures yourself.

## Context

Flows receive a context object with logging and environment access:

```ts
ctx.log.info('message')
ctx.log.warn('message')
ctx.log.error('message')

ctx.env
```

The exact exported type is `FlowContext<TEnv>`.

## Exported Types

`@trigora/sdk` re-exports the main flow authoring types from `@trigora/contracts`:

- `FlowContext`
- `FlowDefinition`
- `FlowEvent`
- `FlowRunFn`
- `JsonValue`
- `Trigger`
- `WebhookFlowResult`

That means most flow authors can stay entirely within `@trigora/sdk`.

## Typical Workflow

1. Define a flow with `defineFlow`
2. Run it locally with:
   - `trigora trigger hello --payload payload.json` for one-off runs
   - `trigora dev hello --payload payload.json` for manual or payload watch mode
   - `trigora dev stripe-checkout` for a local webhook server
3. Deploy webhook flows with `trigora deploy`
4. Manage hosted flows with `trigora flows`

## Related Packages

- `trigora` - CLI for local development and hosted deploys
- `@trigora/contracts` - shared public types used by the SDK, CLI, and API consumers

## License

MIT
