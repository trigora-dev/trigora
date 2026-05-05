# trigora

The Trigora CLI for local flow development, hosted deploys, and alpha flow management.

`trigora` helps you:

- scaffold a project
- run flows locally with real payloads
- watch flows during development
- deploy webhook flows to Trigora Cloud
- inspect, disable, and enable hosted flows

## Install

Install the CLI and SDK in your project:

```bash
npm install trigora @trigora/sdk
```

If you install locally, run commands with `npx trigora`. The examples below use `trigora` for brevity.

## Quick Start

Create a project, start the local webhook dev server, then deploy:

```bash
trigora init
trigora dev hello
trigora deploy hello
```

`trigora init` creates:

- `flows/hello.ts`
- `payload.json` - optional sample payload for `trigora trigger` or payload dev mode
- `.env.example`

`payload.json` is not needed for webhook dev mode. Webhook flows receive payloads from HTTP requests.

## Project Structure

A typical Trigora project looks like this:

```text
.
├── flows/
│   └── hello.ts
├── payload.json
├── .env
└── .env.local
```

Flows are plain TypeScript or JavaScript modules. The generated starter flow looks like this:

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

## Commands

### `trigora init`

Scaffold a new project in the current directory.

```bash
trigora init
trigora init --force
```

What it does:

- creates a starter flow at `flows/hello.ts`
- creates an optional sample `payload.json` for local payload-driven runs
- creates `.env.example` with `TRIGORA_DEPLOY_TOKEN`
- prints suggested next steps

Use `--force` to overwrite existing generated files.

### `trigora trigger <flow>`

Run a flow once locally.

```bash
trigora trigger hello
trigora trigger hello --payload payload.json
trigora trigger ./flows/hello.ts --payload payload.json
```

Behavior:

- loads the selected flow module
- loads and parses the payload file when provided
- invokes the flow locally
- prints structured logs and a success or failure summary

If no payload file is passed, the payload defaults to `{}`.

`trigger` runs the flow once with a local JSON payload file. It does not start an HTTP server.

### `trigora dev [flow]`

Run a flow locally in development mode.

```bash
trigora dev hello
trigora dev ./flows/hello.ts
trigora dev my-manual-flow --payload payload.json
```

Webhook dev mode:

- starts a local webhook server at `http://localhost:5252`
- falls forward to the next available port if `5252` is in use
- accepts `POST /`
- parses JSON request bodies
- runs the flow only when a request is received
- watches the flow file and reloads it on save without executing it

Webhook dev mode does not use `payload.json`; send payloads with HTTP requests instead.

Manual / payload dev mode:

- runs the flow immediately
- watches the flow file for changes
- watches the payload file when provided
- re-runs automatically on save

Use `payload.json` here when you want sample local input for payload-driven runs.

If exactly one flow file exists under `flows/`, you can omit the flow argument:

```bash
trigora dev
```

The CLI resolves either a flow name or a direct file path.

### `trigora deploy [flow]`

Deploy flows to Trigora Cloud.

```bash
trigora deploy
trigora deploy hello
trigora deploy ./flows/hello.ts
```

Behavior:

- validates selected flow modules
- builds a deployment artifact
- uploads the deployment package
- activates the deployment in Trigora Cloud
- prints a deployment summary with route and endpoint details

When no flow is passed, `trigora deploy` discovers all `.ts` and `.js` flow files under `flows/`.

Current alpha limitation:

- `trigora deploy` currently supports webhook-triggered flows only

For webhook deployments, the default hosted route is `/<flowId>`.

### `trigora flows`

List hosted flows available to the authenticated deploy token.

```bash
trigora flows
```

The list output includes:

- flow name
- hosted flow ID
- trigger type
- status
- endpoint for webhook flows
- schedule for cron flows
- queue name for queue flows

### `trigora flows inspect <flowId>`

Inspect a single hosted flow.

```bash
trigora flows inspect 402c04b0-62c8-4d0b-942f-0ee2329436a8
```

The detail view shows available metadata such as:

- name
- ID
- trigger
- status
- creation time
- endpoint and route for webhook flows
- schedule for cron flows
- queue name for queue flows

### `trigora flows disable <flowId>`

Disable a hosted flow.

```bash
trigora flows disable 402c04b0-62c8-4d0b-942f-0ee2329436a8
```

This is idempotent from the CLI point of view. A successful response prints the flow ID and current status.

### `trigora flows enable <flowId>`

Enable a disabled hosted flow.

```bash
trigora flows enable 402c04b0-62c8-4d0b-942f-0ee2329436a8
```

Like `disable`, this prints a concise success summary with the flow ID and resulting status.

### `trigora secrets`

Manage hosted flow secrets separately from deploys.

```bash
trigora flows
trigora secrets set STRIPE_WEBHOOK_SECRET --flow 402c04b0-62c8-4d0b-942f-0ee2329436a8
trigora secrets list --flow 402c04b0-62c8-4d0b-942f-0ee2329436a8
trigora secrets delete STRIPE_WEBHOOK_SECRET --flow 402c04b0-62c8-4d0b-942f-0ee2329436a8
```

Hosted flows can access these secrets through `ctx.env`:

```ts
const secret = ctx.env.STRIPE_WEBHOOK_SECRET;
```

Use `trigora flows` to look up the hosted flow ID first. `trigora secrets set` prompts for the value securely by default. You can pass `--value` for automation when needed, but interactive entry is safer because shell history can leak secrets.

Secret metadata is listed without values, and `trigora secrets delete` asks for confirmation unless you pass `--yes`.

Secrets are managed separately from deploys. `trigora deploy` uploads code only.

### `trigora logs`

Inspect recent hosted flow invocations and stored log lines for a hosted flow ID.

```bash
trigora logs list --flow 402c04b0-62c8-4d0b-942f-0ee2329436a8
trigora logs get inv_123 --flow 402c04b0-62c8-4d0b-942f-0ee2329436a8
```

`trigora logs list` shows recent invocations for a hosted flow, newest first. `trigora logs get` opens one invocation with its stored log lines and metadata.

## Authentication

Hosted commands require a deploy token:

```bash
TRIGORA_DEPLOY_TOKEN=your-deploy-token
```

Commands that require `TRIGORA_DEPLOY_TOKEN`:

- `trigora deploy`
- `trigora flows`
- `trigora flows inspect <flowId>`
- `trigora flows disable <flowId>`
- `trigora flows enable <flowId>`
- `trigora secrets set <name> --flow <flowId>`
- `trigora secrets list --flow <flowId>`
- `trigora secrets delete <name> --flow <flowId>`
- `trigora logs list --flow <flowId>`
- `trigora logs get <invocationId> --flow <flowId>`

If the token is missing, invalid, or revoked, the CLI returns a clear error message.

## Environment Loading

The CLI automatically loads environment variables from:

- `.env`
- `.env.local`

Behavior:

- `.env` is loaded first
- `.env.local` is loaded after that
- existing shell environment variables are not overridden

In local `trigora trigger` and `trigora dev` runs, those values are available in `ctx.env`.

This makes it easy to keep `TRIGORA_DEPLOY_TOKEN` and local secrets out of source control.

## Flow Resolution

For commands that accept `<flow>`, the CLI resolves flows in this order:

1. the exact path you passed
2. `flows/<name>.ts`
3. `flows/<name>.js`
4. `<name>.ts`
5. `<name>.js`

Examples:

```bash
trigora trigger hello
trigora trigger flows/hello.ts
trigora dev ./flows/hello.ts
trigora deploy hello
```

## Output Style

The CLI is designed to be easy to scan in terminals and CI logs.

Examples:

```text
Deploying flow "hello"...

✔ Deployment complete
```

```text
✔ Found 2 flows:
```

```text
✔ Found 3 invocations for flow "stripe-checkout" (402c04b0-...):
```

## Alpha Notes

Current alpha scope:

- local development with `trigger` and `dev`
- hosted deploys with `deploy`
- hosted flow listing and inspection
- hosted flow enable and disable actions

Not in the CLI yet:

- webhook signature verification
- flow deletion
- full hosted trigger management from the CLI
- advanced environment management

Deployed webhook flows do expose `event.request.headers` and `event.request.rawBody` if you want to verify signatures yourself.

## Related Packages

- `trigora` - CLI
- `@trigora/sdk` - flow definition API
- `@trigora/contracts` - shared public types

## License

MIT
