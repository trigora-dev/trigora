# @trigora/client

Client for starting and controlling Trigora executions against a local runtime.

```ts
import { createClient } from '@trigora/client';

async function approval() {
  return { result: 42, review: 'ok' };
}

const trigora = createClient();
const run = await trigora.start(approval, {});
await run.send('approved', 'ok');
const report = await run.result();
```

The client talks to `trigora dev` at `TRIGORA_RUNTIME_URL` or `http://127.0.0.1:3477`. After you kill and restart the CLI, `run.send` / `run.result` still work against the same `run.id`.

## API

- `trigora.start(program, input)` / `createClient({ url }).start(...)`
- `trigora.programs()` / `trigora.executions()` / `trigora.getExecution(id)`
- `run.id`
- `run.result()`
- `run.send(event, payload)`
- `run.cancel()`

`program` may be a named async function or a program id string. `event` may be an `event()` definition from `@trigora/sdk` or an event name string.

This package is MIT and talks to the local HTTP runtime. It does not import the TCC engine.

## License

MIT
