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
  A durable execution platform for dynamic TypeScript applications and agents.
</p>

<p align="center">
  <a href="https://trigora.dev/docs/getting-started"><strong>Get Started</strong></a> ·
  <a href="https://trigora.dev/docs"><strong>Docs</strong></a> ·
  <a href="https://app.trigora.dev"><strong>Dashboard</strong></a>
</p>

---

## Write a program. Run it. It stays alive.

```ts
import { effect, event, invoke, waitForEvent } from '@trigora/sdk';

const approved = event<{ reviewer: string }>('approved');

export async function researchAgent(input: { query: string }) {
  const sources = await effect('search', () => searchWeb(input.query));
  const reports = await Promise.all(
    sources.map((source) => invoke(analyzeSource, { source })),
  );
  const approval = await waitForEvent(approved);
  return effect('publish', () => publish({ reports, reviewer: approval.reviewer }));
}
```

```bash
npx trigora init
npx trigora dev
```

Ordinary TypeScript control flow. Explicit durable primitives. No `defineFlow()`, no mandatory `ctx`.

See the local preview example: [Research agent](./examples/research-agent).

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
npx trigora dev
```

In another terminal, start the program with `@trigora/client`, send any events it waits on, and read `run.result()`.

---

## Packages

| Package | Description |
| --- | --- |
| [`trigora`](./packages/cli) | CLI for local runtime, plus hosted workspace management |
| [`@trigora/sdk`](./packages/sdk) | Durable primitives: `effect`, `sleep`, `waitForEvent`, `invoke`, `event`, `execution` |
| [`@trigora/client`](./packages/client) | `start` / `send` / `result` / `cancel` against the local runtime |
| [`@trigora/contracts`](./packages/contracts) | Shared public contracts, including the compiler/runtime pipeline |

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
