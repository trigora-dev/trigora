<p align="center">
  <a href="https://trigora.dev">
    <img src="https://trigora.dev/banner.png?v=2" alt="Trigora — durable execution, without replay." width="100%" />
  </a>
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/trigora"><img src="https://img.shields.io/npm/v/trigora.svg" alt="npm version" /></a>
  <a href="https://www.npmjs.com/package/trigora"><img src="https://img.shields.io/npm/dm/trigora.svg" alt="npm downloads" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://github.com/trigora-dev/trigora/stargazers"><img src="https://img.shields.io/github/stars/trigora-dev/trigora" alt="GitHub stars" /></a>
</p>

# Trigora

**Durable execution, without replay.**

Trigora is a durable execution substrate for long-running agents and dynamic software.

Its underlying execution architecture—**Transparent Continuation Checkpointing (TCC)**—preserves resumable program state at durable boundaries. Recovery from a committed continuation checkpoint does not require replaying the accumulated execution-history prefix.

```ts
import {
  effect,
  invoke,
  waitForEvent,
} from "@trigora/sdk";

export async function researchAgent(input: ResearchInput) {
  const sources = await effect(() =>
    searchWeb(input.query)
  );

  const analysis = await invoke(analyzeSources, {
    sources,
  });

  await waitForEvent("human.approved");

  return effect(() => publishReport(analysis));
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

## Status

Trigora is under active development.

The TCC research engine and evaluation prototype exist today, including a public [demo](https://demo.trigora.dev). The public SDK, CLI, and managed Trigora Cloud platform are being built. Some code in this repository still reflects Trigora’s earlier event-execution product and should not be considered the final durable-execution API.

## Research

In a controlled evaluation at fixed live state:

- TCC recovery remained approximately **0.6–0.9 ms** across history depths from 10 to 1,000.
- No semantic failures were observed across **50,000 generated cases** within the tested TypeScript subset.

These are research-prototype measurements, not production performance guarantees.

Crash an executor and restore from a committed continuation in the [TCC demo](https://demo.trigora.dev). The demo is a research demonstration, not the hosted product.

[Read the research](https://trigora.dev/research) · [Technical report](https://trigora.dev/research/whitepaper) · [Limitations](https://trigora.dev/research/limitations) · [Demo](https://demo.trigora.dev)

## Learn more

[Website](https://trigora.dev) · [Technology](https://trigora.dev/technology) · [Documentation](https://trigora.dev/docs) · [Demo](https://demo.trigora.dev) · [Design partners](https://trigora.dev/early-access)

Building a workload that needs durable execution? Contact [omar@trigora.dev](mailto:omar@trigora.dev).
