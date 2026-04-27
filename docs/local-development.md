# Local Development

Trigora is designed to make local iteration fast and predictable.

The usual workflow is:

1. define a flow in `flows/`
2. run it once with `trigger`
3. switch to `dev` while you iterate

## Run A Flow Once

Use `trigger` when you want a single local execution:

```bash
npx trigora trigger my-manual-flow --payload payload.json
```

This is useful for:

- quick validation
- trying different payloads
- reproducing behavior without watch mode

This runs the flow once with a local JSON payload file. It does not start an HTTP server.

If you want to simulate a webhook flow once locally, `trigger` can still pass JSON directly to the flow without starting the webhook server.

## Webhook Dev Server

For flows with `trigger: { type: 'webhook' }`, `dev` starts a local webhook server:

```bash
npx trigora dev hello
```

This mode:

- starts a local server at `http://localhost:5252`
- accepts `POST /`
- parses JSON request bodies
- runs the flow only when a request is received
- watches the flow file
- reloads the handler on save without running it

Webhook dev mode does not use `payload.json`; send payloads with HTTP requests instead.

If `5252` is already in use, Trigora prints the next available port.

Example request:

```bash
curl -X POST http://localhost:5252 \
  -H "Content-Type: application/json" \
  -d '{"message":"Hello from Trigora"}'
```

## Manual / Payload Dev Mode

Use `dev` with a payload file when you want a tighter edit-and-run loop for manual or non-webhook flows:

```bash
npx trigora dev my-manual-flow --payload payload.json
```

This mode:

- runs the flow immediately
- watches the selected flow file
- watches the payload file when one is provided
- reruns when either file changes

Use `payload.json` here when you want sample local input for payload-driven runs.

## Flow Resolution

Commands that take a `<flow>` argument accept either a flow name or a path.

Examples:

```bash
npx trigora trigger my-manual-flow
npx trigora trigger ./flows/my-manual-flow.ts
npx trigora dev hello
npx trigora dev my-manual-flow --payload payload.json
```

The CLI resolves flows in this order:

1. the exact path you passed
2. `flows/<name>.ts`
3. `flows/<name>.js`
4. `<name>.ts`
5. `<name>.js`

## Payload Files

Payload files should be valid JSON.

Example:

```json
{
  "message": "Hello from Trigora"
}
```

If you do not pass `--payload`, the payload defaults to an empty object.

Payload files are for `trigora trigger` and manual / payload dev mode. Webhook dev mode uses HTTP request bodies instead.

## Environment Files

The CLI automatically reads:

- `.env`
- `.env.local`

Shell environment variables take precedence over values loaded from these files.

This makes it easy to keep local configuration and deploy tokens out of source control.

## What Local Development Is Best For

Local development is especially useful for:

- iterating on flow logic
- testing payload handling
- testing webhook handlers locally with real HTTP requests
- refining logs
- validating trigger configuration

When the flow is ready for a hosted webhook environment, move on to [Hosted Deploys](./hosted-deploys.md).
