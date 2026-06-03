# Trigora

Run code when things happen.

Trigora is a code-first runtime and hosted control plane for event-driven backend workflows.

Define flows in TypeScript, run them locally with real events, deploy them to hosted webhook and cron endpoints, and inspect invocations, logs, secrets, usage, billing, and custom domains from the dashboard.

Website: https://trigora.dev  
Dashboard: https://app.trigora.dev  
Docs: https://trigora.dev/docs

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

    return {
      ok: true,
      received: event.payload,
    };
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
npx trigora dev hello
```

Send a local request:

```bash
curl -X POST http://localhost:5252 \
  -H "Content-Type: application/json" \
  -d '{"message":"Hello from Trigora"}'
```

Deploy:

```bash
npx trigora deploy hello
```

A deployed webhook flow receives traffic at:

```text
https://<workspace>.trigora.dev/hello
```

Webhook flows can also define custom routes:

```ts
trigger: { type: 'webhook', route: '/hooks/hello' }
```

Pro and Scale workspaces can connect custom domains:

```text
https://events.acme.com/hooks/hello
```

## What Trigora includes

- TypeScript flow definitions with `defineFlow()`
- Local development with real webhook events
- Hosted webhook endpoints
- Cron-triggered flows
- Custom webhook routes
- Workspace-scoped hosted URLs
- Custom domains for Pro and Scale workspaces
- Secrets management
- Invocation history and logs
- Usage and billing dashboard
- CLI flow management

## Packages

### `trigora`

CLI for:

- local development
- hosted deploys
- flow management
- invocation inspection
- secrets

### `@trigora/sdk`

SDK for defining flows with `defineFlow()`.

### `@trigora/contracts`

Shared public contracts and API types.

## Documentation

- Docs: https://trigora.dev/docs
- Getting Started: https://trigora.dev/docs/getting-started
- Deploy: https://trigora.dev/docs/guides/deploy
- Webhook Endpoints: https://trigora.dev/docs/guides/webhook-endpoints
- Custom Domains: https://trigora.dev/docs/guides/custom-domains
- CLI Reference: https://trigora.dev/docs/reference/cli
- API Reference: https://trigora.dev/docs/reference/api

## Repository

This repository contains the public Trigora packages:

- CLI
- SDK
- Contracts
- Examples

The hosted control plane and runtime are deployed separately.

## License

MIT
