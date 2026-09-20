# @trigora/contracts

Public product contracts for Trigora.

Most users should start with:

- `trigora` for the local runtime and CLI
- `@trigora/sdk` for durable program authoring
- `@trigora/client` to talk to local or Cloud HTTP APIs

This package is for typed clients and tooling that consume Trigora responses.

## Install

```bash
npm install @trigora/contracts
```

## What this package covers

Public developer-facing types only:

- Workspace, Project
- Program, ProgramVersion, Execution, Event
- Deploy / start / send / cancel request and response shapes
- ApiError, Pagination

It does **not** export TCC IR, host protocol, effect journals, Durable Object storage, or Cloudflare internals.

```ts
import type {
  Execution,
  ExecutionWait,
  ProgramSummary,
  ListProgramsResponse,
  DeployProgramRequest,
} from '@trigora/contracts';
```

## Related packages

- `@trigora/sdk` — durable program authoring
- `@trigora/client` — start and control executions
- `trigora` — local runtime and Cloud CLI

## License

MIT
