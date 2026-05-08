# Production Workflow

This example demonstrates a production-style Stripe webhook flow with:

- verified Stripe webhook handling
- idempotent state updates
- downstream fanout
- partial failure handling
- structured logging
- local testing and hosted deployment

The architecture is simple: verify the incoming Stripe event, apply one critical state transition, then fan out non-critical follow-up work.

## Flow

See [flow.ts](./flow.ts) for the full implementation.

## Required secrets

Local development reads `.env.local`:

```bash
STRIPE_WEBHOOK_SECRET=whsec_...
APP_API_URL=http://localhost:3000
APP_API_TOKEN=dev_app_token
SLACK_WEBHOOK_URL=https://hooks.slack.com/services/...
ANALYTICS_ENDPOINT=https://analytics.example.com/events
ANALYTICS_TOKEN=analytics_token
EMAIL_API_URL=https://email.example.com/send
EMAIL_API_KEY=email_api_key
```

Hosted deploys read the same values from `ctx.env` after you set them as managed secrets.

## Run locally

```bash
npx trigora dev stripe-checkout
```

The webhook endpoint is:

```text
http://localhost:5252
```

If port `5252` is busy, Trigora prints the next available port.

## Send real webhook traffic

```bash
stripe listen --forward-to http://localhost:5252
stripe trigger checkout.session.completed
```

Use signed Stripe traffic for local verification. Plain unsigned requests will fail signature validation.

## What this flow is doing

The flow is split into one critical write and several non-critical follow-up tasks.

Critical path:

- verify Stripe signature
- validate required metadata
- mark the order as paid

Fan-out work:

- notify Slack
- send analytics
- send the receipt email

That shape is useful when one state transition matters more than the optional notifications around it.

## Deployment

```bash
npx trigora deploy stripe-checkout
```

Set hosted secrets on the deployed flow:

```bash
npx trigora secrets set STRIPE_WEBHOOK_SECRET --flow <flow-id>
npx trigora secrets set APP_API_URL --flow <flow-id>
npx trigora secrets set APP_API_TOKEN --flow <flow-id>
npx trigora secrets set SLACK_WEBHOOK_URL --flow <flow-id>
npx trigora secrets set ANALYTICS_ENDPOINT --flow <flow-id>
npx trigora secrets set ANALYTICS_TOKEN --flow <flow-id>
npx trigora secrets set EMAIL_API_URL --flow <flow-id>
npx trigora secrets set EMAIL_API_KEY --flow <flow-id>
```

## Production notes

For production handlers, plan for:

- idempotent downstream writes
- explicit validation
- partial downstream failures
- useful structured logs
- safe external API timeouts

Trigora gives you the invocation boundary. Your flow still owns the business logic and side-effect policy.
