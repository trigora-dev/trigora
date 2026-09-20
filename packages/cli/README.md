# trigora

The Trigora CLI.

Local preview:

- `trigora init` scaffolds `trigora.config.ts` and a starter program
- `trigora dev` compiles discovered programs with the TCC TypeScript (or Python) frontend and runs them on a SQLite-backed local host
- `trigora programs` / `trigora executions` / `trigora start` / `trigora send` / `trigora cancel` talk to that local runtime

Cloud (set `TRIGORA_TOKEN`):

- `trigora deploy` compiles locally and uploads dual artifacts
- `trigora programs` / `trigora executions` / `trigora start` / `trigora send` / `trigora cancel` / `trigora whoami` hit Trigora Cloud

Requires **Node 22** and `--experimental-sqlite` (the published `trigora` binary sets this flag). Local and Cloud differ only by base URL and auth — both go through `@trigora/client`.

## Install

```bash
npm install trigora @trigora/sdk @trigora/client
```

The CLI depends on `@tcc-engine/frontend-typescript`, `@tcc-engine/bindings-javascript`, and `@tcc-engine/host-node`. Those engine packages are a separate distribution from this MIT CLI; do not import them from application code.

## Quick Start

```bash
trigora init
trigora dev
```

`trigora init` creates:

- `trigora.config.ts`
- `src/programs/hello.ts`
- `.env.example`

While `trigora dev` is running:

```bash
trigora programs
trigora start hello
trigora executions
trigora executions inspect <id>
trigora send <id> greeted --payload '"Omar"'
trigora cancel <id>
```

You can also start executions with `@trigora/client`:

```ts
import { createClient } from '@trigora/client';

async function hello() {
  return { greeting: 'hello' };
}

const trigora = createClient();
const run = await trigora.start(hello, {});
await run.send('greeted', 'Omar');
const result = await run.result();
```

Kill `trigora dev` while an execution is waiting, start it again, then `send` — waiting executions restore from `.trigora/state.db`.

Default runtime URL: `http://127.0.0.1:3477` (`TRIGORA_RUNTIME_URL` or `--port` / `--host`).

See [`examples/typescript/approval`](../../examples/typescript/approval).

The current compiler subset (`ts.subset.v1` / `py.subset.v1`) accepts a default-export async function with **no parameters**. `export default async function run(input)` is not supported yet — that is a current subset limitation, not the permanent product model. Directly executing a program file still throws and tells you to use `trigora dev`.

## Related Packages

- `@trigora/sdk` — program authoring primitives
- `@trigora/client` — start / send / result / cancel (local or Cloud)
- `@trigora/contracts` — shared public types

## License

MIT
