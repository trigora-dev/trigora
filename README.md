# Trigora

Run code when things happen.

Trigora is a code-first runtime for event-driven backend work.

Define flows in TypeScript, run them locally with real events, and deploy the same handler globally.

Hosted deploy is currently in private alpha.  
Request access: https://trigora.dev

## Install

```bash
npm install trigora @trigora/sdk
```

If installed locally, run commands with `npx trigora`.

## Quick example

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'webhook' },

  async run(event, ctx) {
    await ctx.log.info('Received event', event.payload);

    return { ok: true };
  },
});
```

## Quick start

Initialize a project:

```bash
npx trigora init
```

Run locally:

```bash
npx trigora dev
```

If your project contains multiple flows, pass the flow name explicitly:

```bash
npx trigora dev hello
```

Send a request:

```bash
curl -X POST http://localhost:5252 \
  -H "Content-Type: application/json" \
  -d '{"message":"Hello from Trigora"}'
```

Deploy:

```bash
npx trigora deploy hello
```

## Documentation

Full documentation, guides, examples, and API reference:

- Docs: https://trigora.dev/docs
- Getting Started: https://trigora.dev/docs/getting-started
- Production Workflow: https://trigora.dev/docs/examples/production-workflow
- CLI Reference: https://trigora.dev/docs/reference/cli
- API Reference: https://trigora.dev/docs/reference/api

## Packages

### `trigora`

CLI for:

- local development
- hosted deploys
- flow management
- logs and secrets

### `@trigora/sdk`

SDK for defining flows with `defineFlow()`.

### `@trigora/contracts`

Shared public contracts and types.

## Repository

This repository contains:

- CLI
- SDK
- Contracts
- examples

## License

MIT
