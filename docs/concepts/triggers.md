# Triggers

Triggers describe how a flow is intended to run.

They are declared on the flow itself:

```ts
trigger: { type: 'manual' }
```

## Manual

```ts
{ type: 'manual' }
```

Use manual triggers for:

- local development
- ad hoc testing
- payload-driven iteration

Manual flows are a great default when you are still designing logic locally.

## Webhook

```ts
{ type: 'webhook' }
```

Webhook flows are meant for hosted request-response behavior.

You can also include an optional event label:

```ts
{ type: 'webhook', event: 'orders.created' }
```

Webhook flows are the current hosted deployment path in alpha.

## Cron

```ts
{ type: 'cron', cron: '0 2 * * *' }
```

Use cron triggers when the flow represents scheduled work.

You can author cron-triggered flows today, but hosted deploy currently focuses on webhook flows for the alpha release.

## Choosing A Trigger

Choose:

- `manual` when you are testing or iterating locally
- `webhook` when the flow should respond to hosted HTTP requests
- `cron` when the flow represents recurring scheduled work

## Trigger Typing

Trigger types are strict.

That means:

- required fields must be present
- invalid extra fields should fail at compile time
- properties from one trigger kind should not be mixed into another
