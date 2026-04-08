# Triggers

Triggers define how a flow is invoked.

---

## Manual Trigger

```ts
{ type: 'manual' }
```

Used for local development and testing.

---

## Future Trigger Types

Planned triggers include:

- **webhook** — HTTP-based triggers
- **schedule** — cron-like execution
- **queue** — message-driven systems

---

## Design

Triggers are declarative.

They describe *when* a flow runs, not *how* it runs.

---

## Example

```ts
defineFlow({
  id: 'example',
  trigger: { type: 'manual' },
  async run(event, ctx) {}
});
```
