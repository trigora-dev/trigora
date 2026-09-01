# @trigora/client

Client for starting and controlling Trigora executions against a local runtime.

```ts
import { createClient } from '@trigora/client';
import { approved, researchAgent } from './src/researchAgent';

const trigora = createClient();

const run = await trigora.start(researchAgent, {
  query: 'durable agent infrastructure',
});

await run.send(approved, { reviewer: 'Omar' });
const report = await run.result();
```

The client talks to `trigora dev` at `TRIGORA_RUNTIME_URL` or `http://127.0.0.1:3477`.

## API

- `trigora.start(program, input)` / `createClient({ url }).start(...)`
- `run.id`
- `run.result()`
- `run.send(event, payload)`
- `run.cancel()`

`program` may be a named exported async function or a program id string. `event` may be an `event()` definition from `@trigora/sdk` or an event name string.

This package is the local preview client. It does not yet implement hosted production execution.

## License

MIT
