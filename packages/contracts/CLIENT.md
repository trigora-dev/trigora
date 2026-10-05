# Trigora clients

`@trigora/client`, `trigora-client` (import `trigora_client`), and the `trigora-client` crate implement the same `/v1` capabilities. None of them depend on an authoring SDK.

Clients are local unless remote is set. An explicit URL wins over remote. `remote` selects Trigora Cloud (`TRIGORA_API_BASE_URL` or `https://api.trigora.dev`). Otherwise the client uses `TRIGORA_RUNTIME_URL` or `http://127.0.0.1:3477`. `TRIGORA_TOKEN` is read only when remote selects Cloud. An explicit token is still sent to an explicit or local URL.

TypeScript uses `createClient({ remote: true })`. Python uses `Client(remote=True)`. Rust uses `ClientOptions { remote: true, .. }`. Cloud without a token fails at construction.

| Capability | TypeScript | Python | Rust |
| --- | --- | --- | --- |
| whoami | `client.whoAmI()` | `client.whoami()` | `client.whoami()` |
| projects | `client.listProjects()` / `createProject` | `client.projects.list()` / `create` | `client.projects().list()` / `create` |
| deploy | `client.deployProgram` | `client.deploy` | `client.deploy` |
| programs | `client.listPrograms` / `getProgram` / `listProgramVersions` | `client.programs.list` / `get` / `versions` | `client.programs().list()` / `get` / `versions` |
| start | `client.start(program, input)` | `client.executions.start` | `client.executions().start` |
| send | `handle.send` | `handle.send` | `handle.send` |
| cancel | `handle.cancel` | `handle.cancel` | `handle.cancel` |
| result | `handle.result()` | `handle.result()` | `handle.result()` |

Python uses the standard library. Rust uses `ureq`. The local dev server and Cloud share these routes except `whoami`, which the local dev server does not serve.

CLI runtime commands target the local runtime by default. Add `--remote` to target Trigora Cloud. `trigora deploy` and `trigora whoami` are Cloud-only.
