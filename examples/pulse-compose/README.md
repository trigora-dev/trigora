# Pulse Compose

Two local containers share one Unix socket. The Pulse container runs only `trigora-pulse`. The app container compiles the program, registers its handlers, and runs the effect callbacks. The program writes `a 1` to the effect log, waits for an event named `continue`, then writes `b 2`.

`examples/pulse-compose/recover.sh` is the automated proof:

1. Effect A commits, and effect B has not run.
2. Both containers stop with exit code 0.
3. A fresh stack resumes the same execution id and runs effect B once. Effect A is not repeated.
4. Resuming that id with `other.ts` reports `waiting-for-version` and does not run an effect.
5. A host with no durability configuration exits 2 with `Durability is not configured.` and does not create a socket.

## Run

Docker is required. On macOS the script cross-compiles the Linux binaries inside `rust:1-bookworm`. The local authority is the sibling `trigora-platform` checkout. `pnpm install` must already have placed `typescript` and `@tcc-engine/frontend-typescript` in `packages/cli/node_modules`.

```bash
./examples/pulse-compose/recover.sh
```

The script builds `trigora-pulse:local` and `pulse-compose-app:local`, then prints `recovery passed`.

To bring the stack up yourself after those images exist, create the external volume `pulse6d-sock` and give it to the same numeric user both services use (`PULSE_UID` and `PULSE_GID`, default `1000`). `PULSE_LOG_DIR` is required. Point the host at an authority with `TRIGORA_PULSE_AUTHORITY_URL`, `TRIGORA_PULSE_WORKSPACE_ID`, and `TRIGORA_PULSE_WORKSPACE_TOKEN`. Leave `TRIGORA_TOKEN` unset. Set `PULSE_EXECUTION_ID` to start, or `PULSE_RESUME_EXECUTION_ID` to resume an id you already have.

```bash
docker compose -f examples/pulse-compose/compose.yaml up
```

## Limitations

Images are built locally and are not published.

The socket directory is a Docker named volume. A Docker Desktop bind mount cannot `chmod` a Unix socket.

Both containers use one numeric uid. The host rejects a peer uid that is not its own. `1000` is this example's choice.

A new container does not search for an abandoned execution. Resume happens only when `PULSE_RESUME_EXECUTION_ID` names an id the caller already has. `PULSE_PROGRAM` selects the source file (`program.ts` when unset). A different artifact on a pinned execution waits.
