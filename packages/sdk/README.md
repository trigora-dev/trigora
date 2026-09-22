# @trigora/sdk

Authoring primitives for Trigora durable programs.

Write an ordinary default-export async TypeScript function. Make durable operations explicit. `trigora dev` compiles the supported subset and runs it on the local TCC engine.

```ts
import { effect, waitForEvent } from '@trigora/sdk';

export default async function approval() {
  const result = await effect('generate', () => 42);
  const review = await waitForEvent('approved');
  return { result, review };
}
```

There is no `defineFlow()` and no mandatory `ctx`. The local compiler supports `ts.subset.v1`: a default-export async function with no parameters, or one plain parameter (`approval(input)`). Not a second parameter, a default, a rest parameter, or a binding pattern. Effect keys and event names are string literals.

## Install

```bash
npm install @trigora/sdk trigora @trigora/client
```

## Entrypoints

Discover programs from `trigora.config.ts`:

```ts
import { defineConfig } from '@trigora/sdk';

export default defineConfig({
  programs: './src/programs/**/*.ts',
});
```

Each matching file should default-export one async function. The function name is the program id.

## Primitives

- `effect(name, fn)` — durable side effect; `name` is required
- `sleep(ms)` — timer suspension
- `waitForEvent('approved')` — wait for an event
- `invoke('analyze', input)` — child execution. `input` is optional.
- `execution.id` / `execution.attempt` / `execution.signal` — current execution metadata

`event()` helpers exist for typed clients but are not required. Directly executing a program file will throw; start it with `@trigora/client` while `trigora dev` is running.

## Local preview

```bash
npx trigora init
npx trigora dev
trigora start approval
trigora send <id> approved --payload '"ok"'
```

See [`examples/typescript/approval`](../../examples/typescript/approval).

Webhook signature helpers remain available at `@trigora/sdk/stripe` and `@trigora/sdk/github`.

This package is MIT. The CLI may depend on separate `@tcc-engine/*` packages; those are not part of this SDK.

## License

MIT
