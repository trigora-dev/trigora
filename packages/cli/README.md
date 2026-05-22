# trigora

The Trigora CLI for local flow development, hosted deploys, and alpha flow management.

`trigora` helps you:

- scaffold a project
- run flows locally with real payloads
- watch flows during development
- deploy webhook flows to Trigora Cloud
- show which workspace and deploy token you're using
- list and manage hosted flows
- manage hosted flow secrets
- inspect hosted invocations and logs

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
Hosted webhook `route` settings do not change local dev routing today. The local dev server still accepts `POST /`.

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
- prints a deployment summary with endpoint details

When no flow is passed, `trigora deploy` discovers all `.ts` and `.js` flow files under `flows/`.

Current alpha limitation:

- `trigora deploy` currently supports webhook- and cron-triggered flows

`<flow>` always means the internal flow identifier from `defineFlow({ id: '...' })`.
Webhook `route` is a separate public hosted path.

For webhook deployments:

- if `route` is omitted, the hosted endpoint defaults to `https://<workspace>.trigora.dev/<flow>`
- if `route` is set, the hosted endpoint uses that public path instead

Example:

```ts
export default defineFlow({
  id: 'stripe-webhook',
  trigger: { type: 'webhook', route: '/hooks/stripe' },
  async run() {
    return { ok: true };
  },
});
```

### `trigora flows`

List hosted flows available to the authenticated deploy token.

```bash
trigora flows
```

The list output includes:

- flow id
- trigger type
- status
- route for webhook flows
- endpoint for webhook flows
- schedule for cron flows
- queue name for queue flows

### `trigora whoami`

Show the authenticated workspace and deploy token.

```bash
trigora whoami
```

Example output:

```text
Workspace  acme
Token      local-dev
Status     active
```

### `trigora flows inspect <flow>`

Inspect a single hosted flow.

```bash
trigora flows inspect stripe-checkout
```

The detail view shows available metadata such as:

- flow id
- trigger
- route for webhook flows
- status
- creation time
- endpoint for webhook flows
- schedule for cron flows
- queue name for queue flows

### `trigora flows disable <flow>`

Disable a hosted flow.

```bash
trigora flows disable stripe-checkout
```

This is idempotent from the CLI point of view. A successful response prints the flow and current status.

### `trigora flows enable <flow>`

Enable a disabled hosted flow.

```bash
trigora flows enable stripe-checkout
```

Like `disable`, this prints a concise success summary with the flow and resulting status.

### `trigora flows delete <flow>`

Delete a hosted flow and its hosted resources.

```bash
trigora flows delete stripe-checkout
trigora flows delete stripe-checkout --yes
trigora flows delete stripe-checkout -y
```

By default, the CLI requires typed confirmation before deleting a flow:

```text
This will delete flow "stripe-checkout", including deployments, invocations, logs, schedules, secrets, and hosted workers.

Type "stripe-checkout" to confirm:
```

Use `--yes` or `-y` to skip the prompt in automation or non-interactive environments.

### `trigora secrets`

List hosted secret metadata (names and timestamps, never values).

```bash
trigora secrets
trigora secrets --flow stripe-checkout
```

With no options, lists secrets across all flows in the workspace. With `--flow`, lists secrets for one flow only.

There is no `secrets list` subcommand. The root command is the list action.

### `trigora secrets set <name> --flow <flow>`

Set a hosted flow secret.

```bash
trigora secrets set STRIPE_WEBHOOK_SECRET --flow stripe-checkout
trigora secrets set STRIPE_WEBHOOK_SECRET --flow stripe-checkout --value super-secret
```

`--flow` is required. The CLI prompts for the value securely by default. Pass `--value` for automation when needed, but interactive entry is safer because shell history can leak secrets.

### `trigora secrets delete <name> --flow <flow>`

Delete a hosted flow secret.

```bash
trigora secrets delete STRIPE_WEBHOOK_SECRET --flow stripe-checkout
trigora secrets delete STRIPE_WEBHOOK_SECRET --flow stripe-checkout --yes
```

`--flow` is required. The CLI asks for confirmation unless you pass `--yes` or `-y`.

Hosted flows can access secrets through `ctx.env`:

```ts
const secret = ctx.env.STRIPE_WEBHOOK_SECRET;
```

Use `trigora flows` or `trigora secrets` to look up flow ids first. Secrets are managed separately from deploys. `trigora deploy` uploads code only.

### `trigora invocations`

List and inspect hosted flow invocations.

```bash
trigora invocations
trigora invocations --flow stripe-checkout
trigora invocations --status failed
trigora invocations --range 7d
trigora invocations inspect inv_123
```

`trigora invocations` lists recent invocations, newest first. Use `inspect` to open one invocation with its metadata and status details.

`--status` accepts `running`, `succeeded`, or `failed`. `--range` accepts values like `7d` or `24h`.

### `trigora logs <invocation>`

Show stored logs for a single invocation.

```bash
trigora logs inv_123
```

`trigora logs <invocation>` prints the log output for one invocation. It does not take `--flow`. Use `trigora invocations` to find invocation ids first.

## Authentication

Hosted commands require a deploy token:

```bash
TRIGORA_DEPLOY_TOKEN=your-deploy-token
```

Optional API override (defaults to `https://api.trigora.dev`):

```bash
TRIGORA_API_BASE_URL=https://api.trigora.dev
```

Commands that require `TRIGORA_DEPLOY_TOKEN`:

- `trigora deploy`
- `trigora flows`
- `trigora whoami`
- `trigora flows inspect <flow>`
- `trigora flows disable <flow>`
- `trigora flows enable <flow>`
- `trigora flows delete <flow>`
- `trigora secrets`
- `trigora secrets --flow <flow>`
- `trigora secrets set <name> --flow <flow>`
- `trigora secrets delete <name> --flow <flow>`
- `trigora invocations`
- `trigora invocations --flow <flow>`
- `trigora invocations --status <status>`
- `trigora invocations --range <range>`
- `trigora invocations inspect <invocation>`
- `trigora logs <invocation>`

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

## Flow Arguments

For hosted commands, `<flow>` means the internal id from `defineFlow({ id: '...' })`, not the public webhook `route`.

For local commands that accept a flow name or path, the CLI resolves flows in this order:

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
✔ Found 3 invocations
```

```text
✔ Found 2 secrets:
```

## Alpha Notes

Current alpha scope:

- local development with `trigger` and `dev`
- hosted deploys with `deploy`
- hosted flow listing, inspection, enable, disable, and delete actions
- hosted secrets management
- hosted invocation listing and inspection
- hosted invocation log output

Not in the CLI yet:

- webhook signature verification
- full hosted trigger management from the CLI
- advanced environment management

Deployed webhook flows do expose `event.request.headers` and `event.request.rawBody` if you want to verify signatures yourself.

## Related Packages

- `trigora` - CLI
- `@trigora/sdk` - flow definition API
- `@trigora/contracts` - shared public types

## License

MIT
