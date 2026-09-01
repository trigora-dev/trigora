# @trigora/sdk

Authoring primitives for Trigora durable programs.

Write ordinary exported async TypeScript functions. Make durable operations explicit. The compiler and local runtime handle resumability.

```ts
import { effect, event, invoke, waitForEvent } from '@trigora/sdk';

export const approved = event<{ reviewer: string }>('approved');

export async function researchAgent(input: { query: string }) {
  const sources = await effect('search', () => searchWeb(input.query));
  const reports = await Promise.all(
    sources.map((source) => invoke(analyzeSource, { source })),
  );
  const approval = await waitForEvent(approved);
  return effect('publish', () => publish({ reports, reviewer: approval.reviewer }));
}
```

There is no `defineFlow()` and no mandatory `ctx`. Native `if` / `for` / `try` / `Promise.all` stay as TypeScript.

## Install

```bash
npm install @trigora/sdk trigora @trigora/client
```

## Entrypoints

Discover programs from `trigora.config.ts` rather than wrapping functions:

```ts
import { defineConfig } from '@trigora/sdk';

export default defineConfig({
  programs: './src/programs/**/*.ts',
});
```

Exported async functions in matching files are durable programs. Export names must be unique.

## Primitives

- `effect(fn)` / `effect(name, fn)` — durable side effect
- `sleep('30m')` — timer suspension
- `event<T>('approved')` — typed event channel
- `waitForEvent(approved)` — wait for an event
- `invoke(otherProgram, input)` — child execution
- `execution.id` / `execution.attempt` / `execution.signal` — current execution metadata

## Local preview

```bash
npx trigora init
npx trigora dev
```

Start and resume executions with [`@trigora/client`](../client) while the local runtime is running. See [`examples/research-agent`](../../examples/research-agent).

Webhook signature helpers remain available at `@trigora/sdk/stripe` and `@trigora/sdk/github`.

## License

MIT
