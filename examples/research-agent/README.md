# Research agent

Canonical local preview of the Trigora programming model: ordinary exported async functions, explicit durable primitives, and a client that can start / wait / resume an execution.

This example does not deploy to Trigora Cloud. It runs against `trigora dev`.

## Programs

`researchAgent` searches, fan-outs child `analyzeSource` executions with `Promise.all`, suspends for an `approved` event, then publishes.

```ts
import { effect, event, invoke, waitForEvent } from '@trigora/sdk';

const approved = event<{ reviewer: string }>('approved');

export async function researchAgent(input: { query: string }) {
  const sources = await effect('search', () => searchWeb(input.query));
  const reports = await Promise.all(
    sources.map((source) => invoke(analyzeSource, { source })),
  );
  const approval = await waitForEvent(approved);
  return effect('publish', () => publish({ reports, reviewer: approval.reviewer }));
}
```

## Run locally

From the repo root:

```bash
pnpm install
pnpm build
```

Terminal 1, from this directory:

```bash
pnpm dev
```

Terminal 2:

```bash
pnpm start
```

`trigora dev` prints the runtime URL (default `http://127.0.0.1:3477`). The client starts `researchAgent`, sends the typed `approved` event, and prints the result.
