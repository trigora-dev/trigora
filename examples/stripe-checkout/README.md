# Stripe Checkout Webhook

This example handles a Stripe `checkout.session.completed` event with a Trigora webhook flow.

## What the flow does

- Accepts a Stripe-style webhook payload
- Checks for `checkout.session.completed`
- Extracts the customer email, amount total, currency, and session id
- Logs a clear `New purchase` message
- Calls a placeholder `fulfillOrder(session)` function to show where your real business logic would go

## Run locally

Copy `flow.ts` into your own Trigora project as `flows/stripe-checkout.ts`.

Start the dev server:

```bash
trigora dev stripe-checkout
```

It watches your flow, reloads the handler on edits, exposes a local webhook endpoint, and streams logs in real time.

Your local webhook endpoint:
http://localhost:5252

If `5252` is already in use, Trigora will print the next available port.

## Send a test event

With the dev server running, you can send a test event into the local webhook endpoint with `curl`:

```bash
curl -X POST http://localhost:5252 \
  -H "Content-Type: application/json" \
  -d '{
    "type": "checkout.session.completed",
    "data": {
      "object": {
        "id": "cs_test_123",
        "customer_email": "customer@example.com",
        "amount_total": 2000,
        "currency": "usd"
      }
    }
  }'
```

You should see logs from your flow immediately in the terminal.

Edit the flow and Trigora reloads the handler without running it. Send another request and the updated flow runs immediately.

## How to deploy it

Once the flow is in your project's `flows/` directory, deploy it with:

```bash
trigora deploy stripe-checkout
```

The returned endpoint can be used as the Stripe webhook URL.

## How to use the deployed endpoint in Stripe

1. Open your Stripe webhook settings.
2. Create or update a webhook endpoint.
3. Paste the endpoint returned by `trigora deploy stripe-checkout`.
4. Subscribe the endpoint to `checkout.session.completed`.

## Production note

This example does not verify Stripe signatures. Production Stripe webhooks should verify signatures before trusting payloads. Deployed webhook flows expose `event.request.headers` and `event.request.rawBody` if you want to handle that verification yourself.
