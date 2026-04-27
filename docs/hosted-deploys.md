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
npx trigora deploy stripe-checkout
npx trigora deploy ./flows/stripe-checkout.ts
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

## Try It

Once your webhook flow is deployed, send it a JSON `POST` request:

```bash
curl -X POST https://trigora.dev/f/7f3c2d91-4a9b-4e92-9f16-5d1c0d7c8c21 \
  -H "Content-Type: application/json" \
  -d '{"type":"checkout.session.completed","data":{"object":{"id":"cs_test_123","customer_email":"customer@example.com","amount_total":2000,"currency":"usd"}}}'
```

## Alpha Limitation

For the current alpha, hosted deploy supports webhook-triggered flows only.

That means:

- `manual` flows are useful for local development and testing
- `cron` flows can be authored, but are not part of the current hosted deploy path
- `webhook` flows are the supported hosted deployment target today
- webhook signature verification is not built in yet

## Recommended Flow

A typical alpha workflow looks like this:

1. build and test the flow locally
2. switch the hosted flow to a webhook trigger if needed
3. set `TRIGORA_DEPLOY_TOKEN`
4. run `npx trigora deploy stripe-checkout`
5. inspect and manage the deployed flow with `trigora flows`
