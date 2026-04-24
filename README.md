# Trigora

Run code when things happen.

Define flows in code, run them locally, and deploy them globally.

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

This creates:

- `flows/hello.ts`
- `payload.json`
- `.env.example`

Run the starter flow once:

```bash
npx trigora trigger hello --payload payload.json
```

Start local watch mode:

```bash
npx trigora dev hello --payload payload.json
```

## Define A Flow

Flows are plain TypeScript or JavaScript modules.

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('Hello from Trigora', {
      payload: event.payload,
    });
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

Run a flow once:

```bash
npx trigora trigger hello --payload payload.json
```

Run a flow in watch mode:

```bash
npx trigora dev hello --payload payload.json
```

The CLI can resolve either a flow name or a direct file path. For example:

```bash
npx trigora trigger hello
npx trigora trigger ./flows/hello.ts
```

## Hosted Deploys

Deploy hosted flows to Trigora Cloud:

```bash
npx trigora deploy
```

Or deploy a specific flow:

```bash
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

Once your webhook flow is deployed, you can send it an HTTP request directly:

```bash
curl https://trigora.dev/f/7f3c2d91-4a9b-4e92-9f16-5d1c0d7c8c21
```

If your flow expects JSON, send a POST request:

```bash
curl -X POST https://trigora.dev/f/7f3c2d91-4a9b-4e92-9f16-5d1c0d7c8c21 \
  -H "Content-Type: application/json" \
  -d '{"message":"Hello from Trigora"}'
```

Hosted commands require a deploy token:

```bash
TRIGORA_DEPLOY_TOKEN=your-deploy-token
```

The CLI automatically reads `.env` and `.env.local` when present.

Hosted deploy is currently in private alpha.  
Request access: https://trigora.dev or https://discord.gg/EJnjjSf4WR

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

## Packages

### `trigora`

The CLI package.

Use it to:

- scaffold projects
- trigger flows locally
- run flows in watch mode
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
- hosted flow management response types

Package README:

- [packages/contracts/README.md](./packages/contracts/README.md)

## Alpha Scope

Current alpha scope:

- local flow development
- local payload-driven testing
- hosted webhook deploys
- hosted flow listing, inspection, disable, and enable

Current limitations:

- hosted deploy currently supports webhook-triggered flows only
- delete is not available yet from the CLI
- advanced hosted environment management is not available yet

## Repository

This repository contains the open-source Trigora developer toolkit:

- CLI
- SDK
- Contracts
- documentation and examples

## License

MIT
