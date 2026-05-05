# Trigora

Run code when things happen.

Define flows in code, run them locally, and deploy them globally.

Trigora is designed for event-driven code:

- webhooks
- scheduled jobs
- background tasks

It lets you build and test these flows locally before deploying them.

Hosted deploy is currently in private alpha.  
Request access: https://trigora.dev or https://discord.gg/EJnjjSf4WR

For the current alpha, Trigora gives you:

- a CLI for local development and hosted deploys
- a small SDK for defining flows
- shared public contracts for the CLI, API, and consumers
- hosted flow management commands for listing, inspecting, disabling, and enabling flows

## Install

Install the CLI and SDK in your project:

```bash
npm install trigora @trigora/sdk
```

If installed locally, run commands with `npx trigora`.

## Quick Start

Create a starter project:

```bash
npx trigora init
```

This creates a starter project with:

- `flows/hello.ts`
- `payload.json` — optional sample payload for `trigora trigger` or payload dev mode
- `.env.example`

`payload.json` is not needed for webhook dev mode. Webhook flows receive payloads from HTTP requests.

Run the webhook dev server:

```bash
npx trigora dev hello
```

Send a test event:

```bash
curl -X POST http://localhost:5252 \
  -H "Content-Type: application/json" \
  -d '{"message":"Hello from Trigora"}'
```

Deploy the webhook flow:

```bash
npx trigora deploy hello
```

## Local Webhook Development

For webhook flows, `trigora dev` starts a local webhook server:

```bash
# run locally
npx trigora dev hello
```

Example output:

```text
[dev] running hello

Local webhook endpoint:
http://localhost:5252

Watching flow:
flows/hello.ts

Ready to receive events.
```

Edit the flow and Trigora reloads the handler without running it. The next request uses the updated code.

For a real-world webhook example, see [examples/stripe-checkout](./examples/stripe-checkout).

## Define A Flow

Flows are plain TypeScript or JavaScript modules.

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'webhook' },
  async run(event, ctx) {
    await ctx.log.info('Received event', event.payload);
  },
});
```

Supported trigger types today:

- `manual`
- `webhook`
- `cron`

Webhook flows can return HTTP-friendly values from `run`. Manual and cron flows are for local or background execution and do not use return values.

## Local Development

Trigora is designed to make local development the default workflow.

### Webhook Dev Server

Use webhook dev mode for flows with `trigger: { type: 'webhook' }`:

```bash
npx trigora dev hello
```

This mode:

- starts a local webhook server on `http://localhost:5252`
- accepts `POST` requests
- runs the flow only when a request is received
- watches the flow file
- reloads the handler on flow edits without running it

Webhook dev mode does not use `payload.json`; send payloads with HTTP requests instead.

The next webhook request uses the updated flow code.

### Manual / Payload Dev Mode

Use payload dev mode for a non-webhook flow:

```bash
npx trigora dev my-manual-flow --payload payload.json
```

This mode:

- runs the flow once immediately
- watches the selected flow file
- watches the payload file when provided
- reruns when either file changes

Use `payload.json` here when you want a sample local payload file to drive runs.

### Run Once

Use `trigger` when you want a single local execution with a payload file:

```bash
npx trigora trigger hello --payload payload.json
```

This runs the flow once with a local JSON payload file. It does not start an HTTP server.

The CLI can resolve either a flow name or a direct file path. For example:

```bash
npx trigora trigger hello
npx trigora trigger ./flows/hello.ts
npx trigora dev hello
npx trigora dev ./flows/hello.ts
```

## Hosted Deploys

Deploy hosted flows to Trigora Cloud:

```bash
# deploy
npx trigora deploy hello
npx trigora deploy ./flows/hello.ts
```

For alpha, hosted deploys currently support webhook-triggered flows only.

Example result:

```text
✔ Deployment complete

Endpoint:
https://trigora.dev/f/7f3c2d91-4a9b-4e92-9f16-5d1c0d7c8c21
```

## Try It

Deploy your webhook flow:

```bash
# deploy
npx trigora deploy hello
```

Then send it a request:

```bash
# send a request
curl -X POST https://trigora.dev/f/7f3c2d91-4a9b-4e92-9f16-5d1c0d7c8c21 \
  -H "Content-Type: application/json" \
  -d '{"message":"Hello from Trigora"}'
```

Hosted commands require a deploy token:

```bash
TRIGORA_DEPLOY_TOKEN=your-deploy-token
```

The CLI automatically reads `.env` and `.env.local` when present.

In local runs, those values are exposed to flows through `ctx.env`. Existing shell environment variables still take precedence.

## Hosted Flow Management

The CLI can manage deployed hosted flows for your token:

```bash
npx trigora flows
npx trigora flows inspect <flowId>
npx trigora flows disable <flowId>
npx trigora flows enable <flowId>
```

This lets alpha users:

- list hosted flows
- inspect hosted flow details
- disable a flow without deleting it
- re-enable a previously disabled flow

## Secrets

Hosted flows can access secrets through `ctx.env`.

```bash
npx trigora flows
npx trigora secrets set STRIPE_WEBHOOK_SECRET --flow 402c04b0-62c8-4d0b-942f-0ee2329436a8
npx trigora secrets list --flow 402c04b0-62c8-4d0b-942f-0ee2329436a8
npx trigora secrets delete STRIPE_WEBHOOK_SECRET --flow 402c04b0-62c8-4d0b-942f-0ee2329436a8
```

Then in code:

```ts
const secret = ctx.env.STRIPE_WEBHOOK_SECRET;
```

Use `trigora flows` to find the hosted flow ID first. `trigora secrets set` prompts for the value securely by default, and `--value <value>` is available as an optional non-interactive alternative when needed. Secrets are managed separately from deploys. `trigora deploy` uploads code only.

## Logs

Hosted flows retain recent invocation metadata and logs for debugging.

```bash
npx trigora logs list --flow 402c04b0-62c8-4d0b-942f-0ee2329436a8
npx trigora logs get inv_123 --flow 402c04b0-62c8-4d0b-942f-0ee2329436a8
```

Use `trigora logs list` to find a recent failed invocation, then `trigora logs get` to inspect its stored log lines. Both commands require the hosted flow ID.

## Packages

### `trigora`

The CLI package.

Use it to:

- scaffold projects
- trigger flows locally
- run local payload dev mode
- run a local webhook dev server
- deploy hosted webhook flows
- manage hosted flows in alpha

Package README:

- [packages/cli/README.md](./packages/cli/README.md)

### `@trigora/sdk`

The flow authoring SDK.

Use it to:

- define flows with `defineFlow`
- get typed triggers, events, and context
- get trigger-aware return typing for webhook flows

Package README:

- [packages/sdk/README.md](./packages/sdk/README.md)

### `@trigora/contracts`

Shared public contracts.

Use it when you need:

- shared flow and trigger types
- deployment request and response contracts
- hosted API error contracts
- hosted flow management, secret, and invocation response types

Package README:

- [packages/contracts/README.md](./packages/contracts/README.md)

## Alpha Scope

Current alpha scope:

- local flow development
- local payload-driven testing
- local webhook dev server
- hosted webhook deploys
- hosted flow listing, inspection, disable, and enable
- hosted invocation inspection

Current limitations:

- hosted deploy currently supports webhook-triggered flows only
- delete is not available yet from the CLI
- advanced hosted environment management is not available yet

Deployed webhook flows do expose `event.request.headers` and `event.request.rawBody` if you want to verify signatures yourself.

## Repository

This repository contains the open-source Trigora developer toolkit:

- CLI
- SDK
- Contracts
- documentation and examples

## License

MIT
