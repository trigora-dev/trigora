# Context

The `ctx` object provides runtime utilities available inside a flow.

---

## Logging

```ts
await ctx.log.info('message');
await ctx.log.warn('warning');
await ctx.log.error('error');
```

Logs are formatted and printed by the CLI.

---

## Environment Variables

```ts
ctx.env
```

Access environment variables passed into the runtime.

---

## Design

The context is intentionally minimal and explicit.  
All side effects (logging, environment access, integrations) flow through `ctx`.

---

## Future

Planned additions:

- external service integrations
- secrets management
- tracing / observability
