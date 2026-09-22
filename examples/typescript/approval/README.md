# Approval (TypeScript)

Canonical local preview: one default-export program, a named `effect`, then `waitForEvent`. Kill `trigora dev` while the execution is waiting, restart it, and send the event — the run resumes from SQLite, not from parked in-process promises.

```ts
import { effect, waitForEvent } from '@trigora/sdk';

export default async function approval() {
  const result = await effect('generate', () => 42);
  const review = await waitForEvent('approved');
  return { result, review };
}
```

This example stays inside `ts.subset.v1` and does not take a parameter. The subset also allows one plain parameter. It does not use `Promise.all` or an unnamed `effect(fn)`.

## Run locally

From the repo root:

```bash
pnpm install
pnpm build
```

Terminal 1, from this directory:

```bash
pnpm dev
```

Terminal 2:

```bash
pnpm start
```

To see recovery: start the program, kill terminal 1, run `pnpm dev` again (it should report a restored suspended execution), then send `approved`.

You can also drive the same runtime from the CLI:

```bash
trigora start approval
trigora executions inspect <id>
trigora send <id> approved --payload '"ok"'
```

Default runtime URL: `http://127.0.0.1:3477`.
