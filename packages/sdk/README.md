# @trigora/sdk

Define flows for Trigora.

---

## Install

```bash
npm install @trigora/sdk
```

---

## Basic Usage

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('Hello');
  },
});
```

---

## Webhook Responses

Webhook flows can return HTTP-friendly values directly from `run`.

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

Supported return values:

- `Response` for full HTTP control
- plain objects / arrays / numbers / booleans for JSON responses
- `string` for text responses
- `null` or `undefined` for no-content responses

---

## Flow Structure

```ts
defineFlow({
  id: string,
  trigger: { type: 'manual' },
  run: async (event, ctx) => {}
})
```

---

## Event

```ts
{
  id: string;
  type: string;
  timestamp: string;
  payload: unknown;
}
```

---

## Context

```ts
ctx.log.info()
ctx.log.warn()
ctx.log.error()

ctx.env
```

---

## Notes

- flows are plain TypeScript modules
- no framework required
