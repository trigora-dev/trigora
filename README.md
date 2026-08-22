<p align="center">
  <img src="https://trigora.dev/banner.png" alt="Trigora — run code when things happen" width="100%" />
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/trigora"><img src="https://img.shields.io/npm/v/trigora.svg?label=npm" alt="npm version" /></a>
  <a href="https://www.npmjs.com/package/trigora"><img src="https://img.shields.io/npm/dm/trigora.svg" alt="npm downloads" /></a>
  <a href="https://github.com/trigora-dev/trigora/blob/main/LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://github.com/trigora-dev/trigora/stargazers"><img src="https://img.shields.io/github/stars/trigora-dev/trigora?style=social" alt="GitHub stars" /></a>
</p>

<p align="center">
  <a href="https://trigora.dev/docs">Docs</a> ·
  <a href="https://trigora.dev/docs/getting-started">Getting Started</a> ·
  <a href="https://github.com/trigora-dev/trigora/tree/main/examples">Examples</a> ·
  <a href="https://github.com/trigora-dev/trigora/releases">Changelog</a> ·
  <a href="https://discord.gg/EJnjjSf4WR">Discord</a>
</p>

<p align="center">
  <strong>Run code when things happen.</strong>
</p>

<p align="center">
  A hosted runtime for event-driven TypeScript workflows.
</p>

<p align="center">
  <a href="https://trigora.dev/docs/getting-started"><strong>Get Started</strong></a> ·
  <a href="https://trigora.dev/docs"><strong>Docs</strong></a> ·
  <a href="https://app.trigora.dev"><strong>Dashboard</strong></a>
</p>

---

## Write a flow. Deploy it. Done.

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'hello',
  trigger: { type: 'webhook' },
  async run(event, ctx) {
    await ctx.log.info('Received event', event.payload);
    return { ok: true, received: event.payload };
  },
});
```

```bash
npx trigora init
npx trigora dev hello
npx trigora deploy hello
```

Your flow is live at:

```text
https://<workspace>.trigora.dev/hello
```

Event happens → flow executes.  
Schedule fires → flow executes.  
Job enters queue → flow executes.

---

## Build your first flow

One model for webhooks, schedules, and background work.

**Webhook** — receive HTTP events

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'stripe-webhook',
  trigger: { type: 'webhook', route: '/hooks/stripe' },
  async run(event, ctx) {
    await ctx.log.info('Stripe event', event.payload);
  },
});
```

**Cron** — run on a schedule

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'nightly-sync',
  trigger: { type: 'cron', cron: '0 2 * * *' },
  async run(event, ctx) {
    await ctx.log.info('Nightly sync started');
  },
});
```

**Queue** — process background jobs

```ts
import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'process-image',
  trigger: { type: 'queue', queue: 'image-processing' },
  async run(event, ctx) {
    await ctx.log.info('Processing message', {
      queue: event.queue,
      messageId: event.messageId,
      payload: event.payload,
    });
  },
});
```

See a production-style example: [Stripe checkout](./examples/stripe-checkout).

---

## Why Trigora?

Writing a webhook handler, scheduled job, or queue consumer is usually the easy part. Running it in production means dealing with deployments, secrets, logs, infrastructure, scheduling, workers, and observability.

Trigora handles the operational layer so you can focus on the code that runs when something happens.

---

## Everything around your workflow, included

**Webhooks** — Receive HTTP events through hosted endpoints.

**Cron** — Run workflows on a schedule.

**Queues** — Run asynchronous background work without managing workers.

**Deployments** — Ship flows directly from the CLI.

**Secrets** — Manage environment secrets from the control plane.

**Observability** — Inspect invocations and structured logs.

**Custom domains** — Expose webhook flows through your own domain.

---

## Quick start

```bash
npm install trigora @trigora/sdk
```

If installed locally, run commands with `npx trigora`.

### Local loop

```bash
npx trigora init
npx trigora dev hello

curl -X POST http://localhost:5252 \
  -H "Content-Type: application/json" \
  -d '{"message":"Hello from Trigora"}'

npx trigora deploy hello
```

---

## Packages

| Package | Description |
| --- | --- |
| [`trigora`](./packages/cli) | CLI for local development, deploy, flows, queues, secrets, and invocations |
| [`@trigora/sdk`](./packages/sdk) | Flow authoring with `defineFlow()` |
| [`@trigora/contracts`](./packages/contracts) | Shared public contracts and API types |

This repository contains the public Trigora packages and examples. The hosted control plane and runtime are deployed separately.

---

## Documentation

- [Docs](https://trigora.dev/docs)
- [Getting Started](https://trigora.dev/docs/getting-started)
- [Deploy](https://trigora.dev/docs/guides/deploy)
- [Webhook Endpoints](https://trigora.dev/docs/guides/webhook-endpoints)
- [Cron](https://trigora.dev/docs/guides/cron)
- [Queues](https://trigora.dev/docs/guides/queues)
- [Custom Domains](https://trigora.dev/docs/guides/custom-domains)
- [CLI Reference](https://trigora.dev/docs/reference/cli)
- [API Reference](https://trigora.dev/docs/reference/api)

Website: [trigora.dev](https://trigora.dev) · Dashboard: [app.trigora.dev](https://app.trigora.dev)

---

## License

[MIT](./LICENSE)
