# Pulse Node

A local HTTP server that starts one Pulse execution per request. The sample program runs a single effect and returns `42`. The process opens `trigora-pulse` with `--ephemeral`, so the authority stays in memory for this process.

## Run

From the repository root, with dependencies installed:

```bash
cargo build -p trigora-pulse
pnpm exec tsx examples/pulse-node/server.ts
curl -X POST http://127.0.0.1:3478/executions
```

The binary defaults to `target/debug/trigora-pulse`. Set `TRIGORA_PULSE_BIN` to use another build. A successful response is JSON `{ "executionId": "..." }`.

## Limitations

The `200` means the host accepted the id. It is not a checkpoint, and the execution does not survive the `trigora-pulse` process.

Effect callbacks are synchronous. A slow callback blocks this Node process. The Rust host keeps driving other work.

This example does not resume an execution.
