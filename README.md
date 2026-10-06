<p align="center">
  <a href="https://trigora.dev">
    <img src="https://trigora.dev/trigora-banner.png" alt="Trigora — durable execution without history replay." width="100%" />
  </a>
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/trigora"><img src="https://img.shields.io/npm/v/trigora.svg" alt="npm version" /></a>
  <a href="https://www.npmjs.com/package/trigora"><img src="https://img.shields.io/npm/dm/trigora.svg" alt="npm downloads" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://github.com/trigora-dev/trigora/stargazers"><img src="https://img.shields.io/github/stars/trigora-dev/trigora" alt="GitHub stars" /></a>
</p>

<p align="center">
  <a href="https://github.com/trigora-dev/trigora-typescript"><b>TypeScript</b></a>
  ·
  <a href="https://github.com/trigora-dev/trigora-python"><b>Python</b></a>
  ·
  <a href="https://github.com/trigora-dev/trigora-rust"><b>Rust</b></a>
  ·
  <a href="https://github.com/trigora-dev/tcc-engine"><b>TCC Engine</b></a>
  ·
  <a href="https://cloud.trigora.dev"><b>Cloud</b></a>
</p>

# Trigora

**Durable execution without history replay.**

Trigora is a durable execution platform for long-lived AI agents and programs.

Write normal application logic in **TypeScript, Python, or Rust**. Trigora can suspend execution across events, timers, external effects, and child programs, then recover from committed continuation state after failure without replaying completed execution history.

```ts
import { effect, waitForEvent } from "@trigora/sdk";

export default async function approval() {
  const result = await effect("generate", () => 42);

  const review = await waitForEvent("approved");

  return { result, review };
}
```

Deploy it once. Start executions from the CLI, an API call, a webhook, or a cron trigger. Let them run for seconds, hours, or days.

## Why Trigora?

Traditional durable workflow systems often reconstruct execution state by replaying prior history.

Trigora uses **Transparent Continuation Checkpointing (TCC)** instead.

At durable boundaries, TCC commits the program position and the live durable state required to continue. Recovery resumes from that committed continuation rather than re-executing the completed prefix.

That makes Trigora a natural fit for workloads that:

- run long-lived AI agents;
- call models, tools, APIs, and external systems;
- wait for humans or external events;
- sleep for minutes, hours, or days;
- coordinate durable child executions;
- branch dynamically;
- need to survive worker or process failure.

[How TCC works →](https://trigora.dev/research)  
[Read the technical report →](https://trigora.dev/research/whitepaper)

## Start building

Install the SDK, client, and CLI for your language.

### TypeScript

```sh
npm install @trigora/sdk @trigora/client trigora
```

### Python

```sh
pip install trigora trigora-client trigora-cli
```

Requires CPython 3.10–3.12 until 3.13 wheels exist.

### Rust

```sh
cargo add trigora trigora-client
cargo install trigora-cli
```

Then initialize a project:

```sh
trigora init
```

Start the local runtime:

```sh
trigora dev
```

When you're ready to run in production:

```sh
trigora deploy
```

[Read the quickstart →](https://trigora.dev/docs/quickstart)

## Trigora Cloud

**Trigora Cloud** is the managed production runtime for TCC programs.

It provides:

- durable execution;
- deployment and program versioning;
- events and timers;
- cron and webhook triggers;
- project secrets;
- execution inspection and observability;
- workspace access and API tokens;
- managed persistence and recovery.

Start at **[cloud.trigora.dev](https://cloud.trigora.dev)**.

## Repository map

Trigora is split into focused repositories.

| Repository | What it contains |
| --- | --- |
| **[trigora](https://github.com/trigora-dev/trigora)** | CLI, local runtime, public contracts, and ecosystem entry point |
| **[trigora-typescript](https://github.com/trigora-dev/trigora-typescript)** | TypeScript authoring SDK and API client |
| **[trigora-python](https://github.com/trigora-dev/trigora-python)** | Python authoring SDK and API client |
| **[trigora-rust](https://github.com/trigora-dev/trigora-rust)** | Rust authoring SDK and API client |
| **[tcc-engine](https://github.com/trigora-dev/tcc-engine)** | Portable TCC execution engine, language frontends, host protocol, and specifications |

### This repository

`trigora-dev/trigora` contains the pieces shared across the Trigora ecosystem:

- the `trigora` CLI;
- the native local runtime used by `trigora dev`;
- shared public contracts;
- release coordination for the Trigora SDK ecosystem.

Language-specific authoring APIs live in their respective repositories above.

## TCC Engine

**TCC Engine** is the portable execution technology underneath Trigora.

It is implemented primarily in Rust, supports native and WebAssembly embedding, and has declared frontends for TypeScript, Python, and Rust.

The engine is available separately for teams that want to embed TCC into their own runtimes or infrastructure.

[TCC Engine on GitHub →](https://github.com/trigora-dev/tcc-engine)

TCC Engine is source-available under the Business Source License 1.1 and converts to Apache 2.0 under its license terms. The Trigora SDKs, clients, CLI, and public contracts are MIT licensed.

## Research

The public research covers:

- continuation-based recovery;
- matched recovery versus history replay;
- live-state scaling;
- crash recovery and fresh-process restore;
- frontend semantics;
- host responsibilities;
- limitations and failure windows.

**[Technical report →](https://trigora.dev/research/whitepaper)**  

## Links

- **Website:** [trigora.dev](https://trigora.dev)
- **Cloud:** [cloud.trigora.dev](https://cloud.trigora.dev)
- **Docs:** [trigora.dev/docs](https://trigora.dev/docs)
- **Research:** [trigora.dev/research](https://trigora.dev/research)
- **TCC Engine:** [github.com/trigora-dev/tcc-engine](https://github.com/trigora-dev/tcc-engine)
- **GitHub:** [github.com/trigora-dev/trigora](https://github.com/trigora-dev/trigora)

## Company

Trigora is built by **Trigora, Inc.**

- For general inquiries: [info@trigora.dev](mailto:info@trigora.dev)
- For product questions and support: [support@trigora.dev](mailto:support@trigora.dev)  
- For commercial terms: [sales@trigora.dev](mailto:sales@trigora.dev)  
- For security reports: [security@trigora.dev](mailto:security@trigora.dev)

## License

MIT © 2026 Trigora, Inc.
