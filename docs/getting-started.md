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
- `payload.json`
- `.env.example`

## 3. Run The Starter Flow

Run the flow once with the sample payload:

```bash
npx trigora trigger hello --payload payload.json
```

You should see flow logs and a local run summary in the terminal.

## 4. Start Local Watch Mode

Run the same flow in watch mode:

```bash
npx trigora dev hello --payload payload.json
```

Now Trigora will:

- run the flow immediately
- watch the flow file for changes
- watch the payload file for changes
- re-run automatically when you save

## 5. Deploy A Hosted Flow

The starter flow created by `trigora init` uses a manual trigger for local development.

Before deploying, create or update a flow to use a webhook trigger, then set your deploy token and run:

```bash
TRIGORA_DEPLOY_TOKEN=your-deploy-token
npx trigora deploy
```

For the current alpha, hosted deploys support webhook-triggered flows only.

## 6. Manage Hosted Flows

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
