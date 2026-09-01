# trigora

The Trigora CLI.

Local preview:

- `trigora init` scaffolds `trigora.config.ts` and a starter program
- `trigora dev` discovers entrypoints, compiles them through the TCC pipeline contract, and starts a local execution runtime

Hosted workspace management commands (`deploy`, `flows`, `queues`, `secrets`, `invocations`, `logs`, `whoami`) still talk to Trigora Cloud. They are not the new durable-program authoring surface.

## Install

```bash
npm install trigora @trigora/sdk @trigora/client
```

## Quick Start

```bash
trigora init
trigora dev
```

`trigora init` creates:

- `trigora.config.ts`
- `src/programs/hello.ts`
- `.env.example`

While `trigora dev` is running, start executions with `@trigora/client`:

```ts
import { createClient } from '@trigora/client';
import { greeted, hello } from './src/programs/hello';

const trigora = createClient();
const run = await trigora.start(hello, { query: 'world' });
await run.send(greeted, { name: 'Omar' });
const result = await run.result();
```

Default runtime URL: `http://127.0.0.1:3477` (`TRIGORA_RUNTIME_URL` or `--port` / `--host`).

See [`examples/research-agent`](../../examples/research-agent) for the canonical local slice: effect, child `invoke`, typed `waitForEvent`, then `run.send()`.

## Hosted commands

Cloud management commands still exist (`trigora whoami`, `trigora flows`, `trigora queues`, `trigora secrets`, `trigora invocations`, `trigora logs`, `trigora deploy`). They operate on the hosted workspace API and are separate from the local durable runtime.

## Related Packages

- `@trigora/sdk` — program authoring primitives
- `@trigora/client` — start / send / result / cancel
- `@trigora/contracts` — shared types, including the compiler and runtime contracts

## License

MIT
