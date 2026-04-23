# Flows

A flow is the core unit of logic in Trigora.

Flows are plain TypeScript or JavaScript modules that export a single flow definition.

## Basic Shape

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('Hello from Trigora', {
      payload: event.payload,
    });
  },
});
```

Every flow has three main parts:

- `id`
- `trigger`
- `run`

## `id`

`id` is the source identifier for the flow in your project.

Guidance:

- keep it stable once a flow is in use
- make it descriptive
- prefer short, readable IDs like `hello`, `nightly-sync`, or `orders-created`

For hosted webhook deployments, the flow ID is also used to derive the default route path.

## `trigger`

`trigger` defines how the flow is intended to run.

Current trigger types:

- `manual`
- `webhook`
- `cron`

The trigger typing is strict, so invalid extra trigger properties should fail at compile time.

## `run`

`run` contains the flow logic.

It receives:

- `event`
- `ctx`

Example:

```ts
async run(event, ctx) {
  await ctx.log.info('Running flow', {
    payload: event.payload,
  });
}
```

## Return Values

Return behavior depends on the trigger:

- `webhook` flows can return HTTP-friendly values
- `manual` flows do not use return values
- `cron` flows do not use return values

## File Organization

A common project layout is:

```text
flows/
  hello.ts
  nightly-sync.ts
  orders-created.ts
```

During deploy, the CLI can discover flow files automatically under `flows/`.

## Good Flow Habits

- keep flows focused on one job
- validate payloads early
- use clear IDs
- log meaningful checkpoints
- test locally before deploying
