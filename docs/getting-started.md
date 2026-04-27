# Getting Started

This guide walks through the fastest path to a working Trigora project.

## 1. Install

Install the CLI and SDK:

```bash
npm install trigora @trigora/sdk
```

## 2. Initialize A Project

Create starter files in your current directory:

```bash
npx trigora init
```

This creates:

- `flows/hello.ts`
- `payload.json` - optional sample payload for `trigora trigger` or payload dev mode
- `.env.example`

`payload.json` is not needed for webhook dev mode. Webhook flows receive payloads from HTTP requests.

## 3. Start Local Webhook Development

The starter flow is a webhook flow, so `dev` starts a local server:

```bash
npx trigora dev hello
```

Send it a test event:

```bash
curl -X POST http://localhost:5252 \
  -H "Content-Type: application/json" \
  -d '{"message":"Hello from Trigora"}'
```

## 4. Run The Same Flow Once

You can still run the same flow once with the sample payload:

```bash
npx trigora trigger hello --payload payload.json
```

You should see flow logs and a local run summary in the terminal.

This runs the flow once with a local JSON payload file. It does not start an HTTP server.

## 5. Manual / Payload Dev Mode

For a manual or other non-webhook flow, `dev` can also watch both the flow and a payload file:

```bash
npx trigora dev my-manual-flow --payload payload.json
```

Use `payload.json` here when you want a sample local payload file to drive runs.

## 6. Deploy A Hosted Flow

The starter flow created by `trigora init` is already a webhook flow, so you can deploy it directly:

```bash
TRIGORA_DEPLOY_TOKEN=your-deploy-token
npx trigora deploy hello
```

For the current alpha, hosted deploys support webhook-triggered flows only.

## 7. Manage Hosted Flows

After deploying, you can manage hosted flows from the CLI:

```bash
npx trigora flows
npx trigora flows inspect <flowId>
npx trigora flows disable <flowId>
npx trigora flows enable <flowId>
```

## Next Steps

- read [Local Development](./local-development.md)
- learn the core concepts in [Flows](./concepts/flows.md) and [Triggers](./concepts/triggers.md)
- review [Hosted Deploys](./hosted-deploys.md)
