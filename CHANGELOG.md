# Changelog

## 1.0.2

### Fixed

- `trigora start` with no `--input` passes an empty argument list. `--input '{}'` is still one object argument.
- Rust effect scaffolding no longer treats words inside string and character literals as captures. `String::from("hello")` scaffolds.
- `trigora init --language rust` tells you to start the Cargo package name.
- Cloud commands share one project context from `trigora.toml`, including `X-Trigora-Project-Id` on deploy.
- `trigora inspect` and execution lists print `programName` when the API sends it.

## 1.0.1

### Fixed

- Fix `trigora dev` in Rust Cargo workspaces by isolating generated effect crates from the enclosing workspace.

### Changed

- Clarify that `TRIGORA_TOKEN` does not switch CLI commands from local to Cloud; `--remote` selects Cloud explicitly.

## 1.0.0

First public release of the Trigora CLI, local runtime, contracts, and the TypeScript, Python, and Rust packages.
