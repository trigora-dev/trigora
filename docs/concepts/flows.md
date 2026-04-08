# Flows

A flow is the core unit of logic in Trigora.

---

## Structure

```ts
defineFlow({
  id: string,
  trigger: Trigger,
  run: async (event, ctx) => {}
})
```

---

## Example

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'payment',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('Processing payment', event.payload);
  },
});
```

---

## Key Concepts

- **id**: unique identifier
- **trigger**: defines how the flow starts
- **run**: executed when triggered

---

## Philosophy

Flows are:

- deterministic
- testable
- composable
- local-first

---

## Best Practices

- keep flows small and focused
- validate payloads early
- use structured logging
