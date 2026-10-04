# trigora

The Trigora CLI.

CLI runtime commands target the local runtime by default. Add `--remote` to target Trigora Cloud. `trigora deploy` and `trigora whoami` are Cloud-only.

- `trigora init` scaffolds `trigora.toml` and a starter program entry
- `trigora dev` compiles discovered programs with the TCC TypeScript, Python, or Rust frontend and runs them on a SQLite-backed local host
- `trigora programs` / `trigora executions` / `trigora start` / `trigora send` / `trigora result` / `trigora cancel` talk to that local runtime, or to Cloud with `--remote`
- `trigora bench` measures one healthy local run. `trigora verify` checks recovery after checkpoints. Both stay on the local runtime.
- `trigora deploy` compiles locally, uploads programs, and replaces the project's triggers to match `trigora.toml`

Requires **Node 22** and `--experimental-sqlite` (the published `trigora` binary sets this flag). `TRIGORA_TOKEN` authenticates Cloud commands. It does not change which host a runtime command uses.

## Install

```bash
npm install trigora @trigora/sdk @trigora/client
```

TypeScript and Rust compilation ship with the CLI. The Python authoring package installs the compatible TCC compiler. The CLI uses that compiler when it builds Python programs. `trigora-client` does not depend on it.

```bash
npm install trigora
python3 -m pip install trigora trigora-client
```

## Quick Start

```bash
trigora init
trigora dev
```

`trigora init` prompts for the language, project name, and whether to create an example when a terminal is attached. `--language typescript|python|rust`, `--name`, and `--example` / `--no-example` skip those prompts. Without a terminal and without `--language`, it scaffolds TypeScript.

`trigora init` creates:

- `trigora.toml`
- `package.json`, `pyproject.toml`, or `Cargo.toml`
- `src/program.ts`, `src/program.py`, or `src/lib.rs` when the example is created
- `.env.example`

While `trigora dev` is running:

```bash
trigora programs
trigora start program
trigora executions
trigora executions inspect <id>
trigora send <id> greeted --payload '"Omar"'
trigora result <id>
trigora cancel <id>
```

You can also start executions with `@trigora/client`:

```ts
import { createClient } from '@trigora/client';

async function program() {
  return { greeting: 'hello' };
}

const trigora = createClient();
const run = await trigora.start(program, {});
await run.send('greeted', 'Omar');
const result = await run.result();
```

Kill `trigora dev` while an execution is waiting, start it again, then `send` — waiting executions restore from `.trigora/state.db`.

Default runtime URL: `http://127.0.0.1:3477` (`TRIGORA_RUNTIME_URL` or `--port` / `--host`).

The approval examples live in the language repos: `trigora-typescript/examples/approval`, `trigora-python/examples/approval`, and `trigora-rust/examples/approval`.

`trigora.toml` is the desired project deployment. Deploy discovers programs from `[project].programs` and replaces triggers to match `[[triggers]]`. No `[[triggers]]` means the desired list is empty, so existing triggers are removed. `trigora init` does not add a trigger.

```toml
[project]
name = "reports"
programs = ["src/**/*.py"]

# [[triggers]]
# name = "nightly-report"
# type = "cron"
# program = "report"
# schedule = "0 2 * * *"
# timezone = "UTC"
```

The compiler subset accepts a default-exported async TypeScript program entry, one Python `@program` async function, or one Rust `pub async fn main`. Start input is an ordered argument list of plain parameters. TypeScript defaults run at the call and may use earlier parameters; extra arguments are ignored. Python defaults are compile-time constants (`None`, bool, finite number, string); extra arguments are a start error. Rust has exact arity and no defaults. Not a rest parameter or a binding pattern. The TypeScript frontend accepts ordinary TypeScript within that subset, and JavaScript-style code without type annotations is valid. `.js` and `.mjs` programs deploy as `typescript`. Directly executing a program file still throws and tells you to use `trigora dev`.

## Related Packages

- `@trigora/sdk` — program authoring primitives
- `@trigora/client` — start / send / result / cancel (local or Cloud)
- `@trigora/contracts` — shared public types

## License

MIT
