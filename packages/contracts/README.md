# @trigora/contracts

Shared public contracts for Trigora.

`@trigora/contracts` is the shared contracts layer used across Trigora packages and advanced integrations.

Most users should start with:

- `trigora` for CLI usage
- `@trigora/sdk` for defining flows

This package is mainly intended for:

- typed API clients
- advanced integrations
- tooling that consumes Trigora responses

If you are authoring flows directly, you will usually want `@trigora/sdk` instead.

## Install

```bash
npm install @trigora/contracts
```

## What This Package Covers

This package exports public contracts for:

- triggers
- flow definitions
- flow events and runtime context
- deployment request and response payloads
- structured API errors
- hosted flow management responses
- hosted flow secret responses
- hosted flow invocation responses

Everything is exported from the package root:

```ts
import type {
  ApiErrorResponse,
  CreateDeploymentRequest,
  CreateDeploymentResponse,
  FlowSecretRecord,
  FlowDefinition,
  FlowInvocationRecord,
  FlowRecord,
  FlowStatusResponse,
  GetInvocationResponse,
  ListInvocationsResponse,
  ListSecretsResponse,
  Trigger,
} from '@trigora/contracts';
```

## Flow Authoring Contracts

These contracts define the core shape of Trigora flows:

- `ManualTrigger`
- `WebhookTrigger`
- `CronTrigger`
- `Trigger`
- `FlowDefinition`
- `FlowRunFn`
- `FlowEvent`
- `FlowContext`
- `JsonValue`
- `WebhookFlowResult`

For webhook flows, `FlowEvent` can also include `request` metadata with headers, method, URL, and `rawBody` in addition to the parsed `payload`.

Example:

```ts
import type { FlowDefinition } from '@trigora/contracts';

const flow: FlowDefinition = {
  id: 'hello',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('Hello from Trigora', {
      payload: event.payload,
    });
  },
};
```

Trigger contracts are strict, so invalid or mixed trigger fields should fail at compile time.

## Deployment Contracts (Advanced)

These contracts are primarily intended for advanced integrations and tooling. Most users will not need to interact with these directly.

They cover the public deployment request and response shapes:

- `DeploymentManifestFlow`
- `DeploymentManifest`
- `DeploymentArtifactFile`
- `DeploymentArtifact`
- `CreateDeploymentRequest`
- `DeploymentStatus`
- `DeploymentManifestSnapshot`
- `DeployedFlowResponse`
- `CreateDeploymentResponse`

Example:

```ts
import type { CreateDeploymentRequest, DeploymentManifest } from '@trigora/contracts';

const manifest: DeploymentManifest = {
  version: 1,
  flow: {
    id: 'hello',
    entrypoint: 'flows/hello.ts',
    trigger: { type: 'webhook' },
  },
};

const request: CreateDeploymentRequest = {
  manifest,
  artifact: {
    version: 1,
    format: 'esm',
    target: 'node20',
    files: [],
  },
};
```

## Structured API Error Contracts

Client-facing errors use a structured shape:

- `ApiErrorCode`
- `ApiErrorStep`
- `ApiErrorResponse`

Example:

```ts
import type { ApiErrorResponse } from '@trigora/contracts';

const error: ApiErrorResponse = {
  error: {
    code: 'internal_error',
    message: 'Failed to activate deployment.',
    step: 'activating',
  },
};
```

These contracts let typed clients branch on stable error codes and optional error steps without relying on ad hoc response parsing.

## Hosted Flow Management Contracts

These contracts support hosted flow management responses used by the CLI and other typed clients:

- `FlowTriggerType`
- `FlowStatus`
- `WebhookFlowRecord`
- `CronFlowRecord`
- `QueueFlowRecord`
- `FlowRecord`
- `ListFlowsResponse`
- `GetFlowResponse`
- `FlowStatusResponse`
- `FlowSecretRecord`
- `ListSecretsResponse`
- `SetFlowSecretRequest`
- `SetFlowSecretResponse`
- `DeleteFlowSecretResponse`
- `FlowInvocationStatus`
- `FlowInvocationLogLevel`
- `FlowInvocationRecord`
- `FlowInvocationLogRecord`
- `ListInvocationsResponse`
- `GetInvocationResponse`
- `ListFlowInvocationsQuery`

Example:

```ts
import type { ListFlowsResponse } from '@trigora/contracts';

const response: ListFlowsResponse = {
  flows: [
    {
      id: 'hello',
      slug: 'hello',
      trigger: 'webhook',
      status: 'ready',
      createdAt: '2026-04-21T10:00:00.000Z',
      endpoint: 'https://acme.trigora.dev/hello',
    },
  ],
};
```

`FlowStatusResponse` is the shared response shape for flow status changes such as disable and enable.

Hosted secret responses expose secret metadata only. Secret values are write-only and should not be returned by the API.

Hosted invocation responses expose invocation metadata and buffered log lines for debugging. They do not imply storage of request bodies, response bodies, or secret values.

## When To Use `@trigora/contracts`

Use this package when you are:

- building a typed API client for Trigora Cloud
- integrating with deployment or hosted flow management responses
- building tooling around Trigora manifests or responses

Most application authors should use `@trigora/sdk` for defining flows and `trigora` for running and deploying them.

## Related Packages

- `trigora` - CLI for local development, hosted deploys, and flow management
- `@trigora/sdk` - flow authoring SDK built on top of these contracts

## License

MIT
