<p align="center">
  <a href="https://trigora.dev">
    <img src="https://trigora.dev/banner.png?v=2" alt="Trigora — durable execution without history replay." width="100%" />
  </a>
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/trigora"><img src="https://img.shields.io/npm/v/trigora.svg" alt="npm version" /></a>
  <a href="https://www.npmjs.com/package/trigora"><img src="https://img.shields.io/npm/dm/trigora.svg" alt="npm downloads" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://github.com/trigora-dev/trigora/stargazers"><img src="https://img.shields.io/github/stars/trigora-dev/trigora" alt="GitHub stars" /></a>
</p>

# Trigora

**Durable execution without history replay.**

Trigora is a durable execution substrate for long-running agents and dynamic software.

Its underlying execution architecture—**Transparent Continuation Checkpointing (TCC)**—preserves resumable program state at durable boundaries. Recovery from a committed continuation checkpoint does not require replaying the accumulated execution-history prefix.

```ts
import { effect, waitForEvent } from "@trigora/sdk";

export default async function approval() {
  const result = await effect("generate", () => 42);
  const review = await waitForEvent("approved");
  return { result, review };
}
```

Trigora is designed for programs that:

- call tools and external systems;
- wait for humans or events;
- invoke durable child executions;
- branch dynamically;
- survive for hours or days;
- recover after worker failure.

Cron, webhooks, queues, and API calls act as **triggers** that start durable executions rather than separate programming models.

## What is ready

- **Trigora Cloud** — managed production platform at [cloud.trigora.dev](https://cloud.trigora.dev).
- **TCC Engine** — portable, source-available engine and language frontends in this repository.
- **TCC Recovery Lab** — public proof of continuation restore at [demo.trigora.dev](https://demo.trigora.dev).

See the [docs quickstart](https://trigora.dev/docs/quickstart) for the write → start → suspend → restore → continue loop.

## Research

Portable TCC Engine benches (Node / WASM reference host) measure recovery vs history depth, live continuation size, and healthy-path overhead vs a matched history-replay baseline. See [Research](https://trigora.dev/research).

An earlier Temporal comparison used a **research prototype** (not the portable engine). Those labeled prototype results remain in the [technical report](https://trigora.dev/research/whitepaper) and [recovery vs history](https://trigora.dev/research/recovery-vs-history) page.

Crash an executor and restore from a committed continuation in the [TCC Recovery Lab](https://demo.trigora.dev). The lab is a controlled demonstration, not the hosted Cloud product.

[Read the research](https://trigora.dev/research) · [Technical report](https://trigora.dev/research/whitepaper) · [Limitations](https://trigora.dev/research/limitations) · [Recovery Lab](https://demo.trigora.dev)

## Learn more

[Website](https://trigora.dev) · [Product](https://trigora.dev/product) · [Documentation](https://trigora.dev/docs) · [Start building](https://cloud.trigora.dev) · [Recovery Lab](https://demo.trigora.dev)

Building a workload that needs durable execution? Start on [Cloud](https://cloud.trigora.dev) or contact [omar@trigora.dev](mailto:omar@trigora.dev).
