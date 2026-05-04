# Context

The `ctx` object gives a flow access to runtime utilities.

It is the second argument passed to `run`.

```ts
async run(event, ctx) {
  await ctx.log.info('Hello from Trigora');
}
```

## Logging

Use `ctx.log` for runtime logs:

```ts
await ctx.log.info('message');
await ctx.log.warn('warning');
await ctx.log.error('error');
```

Use logs to capture:

- important checkpoints
- payload or input details when helpful
- warnings for unexpected but recoverable states
- failures and debugging context

## Environment Variables

`ctx.env` provides access to environment variables available to the flow.

```ts
const apiKey = ctx.env.MY_API_KEY;
```

In local CLI runs, `ctx.env` is populated from the process environment after Trigora loads:

- `.env`
- `.env.local`

Shell environment variables take precedence over values from those files.

## Keep Context Usage Simple

A good rule of thumb is:

- use `event` for the incoming data
- use `ctx` for logging and environment access

This keeps flows easy to read and predictable to test.
