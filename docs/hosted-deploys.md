# Hosted Deploys

Trigora can deploy hosted webhook flows to Trigora Cloud.

## Authentication

Hosted deploy commands require a deploy token:

```bash
TRIGORA_DEPLOY_TOKEN=your-deploy-token
```

You can place this in `.env` or `.env.local` for local use.

## Deploy All Flows

Deploy all discovered flow files under `flows/`:

```bash
npx trigora deploy
```

## Deploy One Flow

Deploy a specific flow by name or path:

```bash
npx trigora deploy hello
npx trigora deploy ./flows/hello.ts
```

## What Deploy Does

The CLI will:

- validate the selected flow modules
- build a deployment artifact
- upload the deployment package
- activate the deployment
- print a deployment summary

Example result:

```text
✔ Deployment complete

Endpoint:
https://trigora.dev/f/7f3c2d91-4a9b-4e92-9f16-5d1c0d7c8c21
```

## Alpha Limitation

For the current alpha, hosted deploy supports webhook-triggered flows only.

That means:

- `manual` flows are useful for local development and testing
- `cron` flows can be authored, but are not part of the current hosted deploy path
- `webhook` flows are the supported hosted deployment target today

## Recommended Flow

A typical alpha workflow looks like this:

1. build and test the flow locally
2. switch the hosted flow to a webhook trigger if needed
3. set `TRIGORA_DEPLOY_TOKEN`
4. run `npx trigora deploy`
5. inspect and manage the deployed flow with `trigora flows`
