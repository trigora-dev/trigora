# Stripe Checkout Webhook

This example verifies Stripe webhook signatures before it trusts the payload.

## Required Secret

Set a Stripe webhook secret:

```bash
STRIPE_WEBHOOK_SECRET=whsec_...
```

The flow reads it from `ctx.env.STRIPE_WEBHOOK_SECRET`.

## Local Dev

Put the secret in `.env.local` in your Trigora project, then start the local webhook server:

```bash
trigora dev stripe-checkout
```

Unsigned `curl` requests will fail verification. Use Stripe's signed tooling instead:

```bash
stripe listen --forward-to http://localhost:5252
stripe trigger checkout.session.completed
```

If port `5252` is busy, Trigora prints the next available port.

## Hosted Deploy

Deploy the flow, then set the hosted secret for that flow:

```bash
trigora deploy stripe-checkout
trigora secrets set STRIPE_WEBHOOK_SECRET --flow <hosted-flow-id>
```

Hosted secrets are exposed to the flow through `ctx.env`.

## Stripe Setup

Use the endpoint returned by `trigora deploy stripe-checkout` as your Stripe webhook URL.

For local signed testing, use the Stripe CLI. For production webhooks, use the Stripe Dashboard or Stripe CLI to deliver signed events. Plain unsigned requests should not be used with this example.
