# Release Runbook

This document describes the maintainer release process.

Trigora `1.0.0` and TCC Engine `26.10.2` are one release set. Package versions in each repository match [versions.toml](versions.toml) before a tag is created. Trigora code is MIT. TCC Engine components are BUSL-1.1 and stay under that license when a Trigora binary links them.

## Versions

| Set | Version | Tag |
| --- | --- | --- |
| TCC Engine | 26.10.2 | `v26.10.2` |
| Trigora | 1.0.0 | `v1.0.0` |

Tags are exact. `v26.10.2` is only the TCC Engine repository. `v1.0.0` is only the Trigora repositories. `26.10.1` stays on the registries. `26.10.2` adds optional SQLite-host tracing for `trigora bench` and `trigora verify`. It does not change the host protocol, artifact format, language semantics, or continuation semantics. Tag the engine before Trigora `v1.0.0`. Do not point Trigora CI at `v26.10.2` until that tag exists.

TCC Engine uses CalVer `YY.MM.MICRO`. `YY` and `MM` identify the release month. `MICRO` is a monotonically increasing release counter within that month; it is not a SemVer patch-only field. Releases within the same year maintain backward compatibility across supported public package APIs, so ordinary npm caret and Cargo version ranges on `26.x` stay safe. Breaking package API changes may occur when the year component changes. Wire, artifact, and language compatibility are versioned independently (`host_protocol_version`, `engine_format_version`, and the language-semantics versions).

## What ships

TCC Engine `26.10.2`:

- crates.io: `tcc-ir`, `tcc-state`, `tcc-core`, `tcc-host`, `tcc-host-sqlite`, `tcc-rust-frontend`
- npm: `@tcc-engine/frontend-typescript`, `@tcc-engine/bindings-javascript`
- PyPI: `tcc-engine`

`tcc-rust-frontend` is the Rust compiler library. The other TCC crates are published so Cargo can resolve `trigora-cli`. `@tcc-engine/host-node` and `@tcc-engine/frontend-rust` are not part of this release. `tcc-rust-compile` is installed with the Trigora CLI, not as its own package.

Trigora `1.0.0`:

- crates.io: `trigora-local`, then `trigora-cli`
- npm: `trigora`, then `@trigora/contracts`, then `@trigora/sdk` and `@trigora/client`
- PyPI: `trigora-cli`, then `trigora` and `trigora-client`
- crates.io authoring, independent of the CLI: `trigora` and `trigora-client`

`cargo install trigora-cli` installs `trigora`, `trigora-local`, and `tcc-rust-compile`. `trigora-local` is also a library crate because `trigora-cli` depends on it. It is not a separate install.

## Publish order

1. TCC Engine crates, in dependency order: `tcc-ir`, `tcc-state`, `tcc-core`, `tcc-host`, `tcc-host-sqlite`, `tcc-rust-frontend`.
2. TCC Engine npm packages and the PyPI `tcc-engine` wheels. Create the `v26.10.2` GitHub Release with those artifacts.
3. `trigora-local`, then `trigora-cli`.
4. npm `trigora` and PyPI `trigora-cli`, from the same native build, then `@trigora/contracts`.
5. `@trigora/sdk` and `@trigora/client`.
6. PyPI `trigora-client`, then PyPI `trigora`. `trigora` depends on `tcc-engine==26.10.2` and `trigora-cli==1.0.0`.
7. crates.io `trigora` and `trigora-client`.
8. Clean installs from the public registries.

## Platforms

npm `trigora`, PyPI `trigora-cli`, and PyPI `tcc-engine` are built for:

| Platform | npm slot |
| --- | --- |
| macOS arm64 | `darwin-arm64` |
| macOS x64 | `darwin-x64` |
| Linux x64 | `linux-x64` |
| Linux arm64 | `linux-arm64` |
| Windows x64 | `win32-x64` |

`tcc-engine` wheels are native builds for those platforms and the supported Python versions. `trigora` and `trigora-client` on PyPI are `py3-none-any`. One `cargo build --release -p trigora-cli` per platform produces `trigora`, `trigora-local`, and `tcc-rust-compile`. The npm package receives all three. The `trigora-cli` wheel receives `trigora` and `trigora-local` only. Each binary set has a `SHA256SUMS` file.

## Workflows

Each repository has three workflows:

- `ci.yml` runs tests and lint on pull requests and on `main`. It does not publish.
- `package.yml` builds release artifacts, runs the clean-install smoke, and uploads the artifacts. It runs from a manual dispatch or from `release.yml`. It does not run on push to `main`, and it does not publish.
- `release.yml` runs the same package workflow, then publishes and creates the GitHub Release. It runs only for an exact release tag and requires the protected release environment.

## Before publishing

Publishing waits until all of the following are true:

- Package versions match [versions.toml](versions.toml).
- Trusted publishers for PyPI, and for npm packages that already exist, match `release.yml` and the `release` environment. First-time npm packages and crates.io crates still have a short-lived token in that environment.
- `package.yml` has been run again from a clean checkout after those updates.
- The clean-install smoke tests passed on those artifacts.

Artifacts from an earlier package run are not published.

## Package checks

From a clean checkout, before a tag:

```bash
cargo package -p tcc-ir
cargo package -p tcc-state
cargo package -p tcc-core
cargo package -p tcc-host
cargo package -p tcc-host-sqlite
cargo package -p tcc-rust-frontend
```

Each packaged manifest depends on versions, not on local paths. `cargo package --list` shows the files that ship.

```bash
npm pack
```

for `@tcc-engine/frontend-typescript`, `@tcc-engine/bindings-javascript`, `trigora`, `@trigora/contracts`, `@trigora/sdk`, and `@trigora/client`. Install each tarball in an empty directory and import it.

```bash
python -m build
```

for `tcc-engine`, `trigora-cli`, `trigora`, and `trigora-client`. The `trigora` wheel is `py3-none-any` and contains no `trigora` binary, no `trigora-local`, and no `_vendor` directory. The `trigora-client` wheel is `py3-none-any` and does not depend on `tcc-engine` or `trigora-cli`.

```bash
cargo package -p trigora-local
cargo package -p trigora-cli
cargo package -p trigora
cargo package -p trigora-client
```

`trigora` and `trigora-client` do not depend on TCC crates or on `trigora-cli`.

Trigora notices describe Trigora artifacts. The package job writes `THIRD_PARTY_LICENSES` from the binaries that artifact ships. The npm package and the GitHub bundle include `trigora`, `trigora-local`, and `tcc-rust-compile`. The `trigora-cli` wheel includes `trigora` and `trigora-local`. TCC Engine notices are produced by the engine repository, for its wheel and its wasm package. The published TCC crates ship their `LICENSE` and do not carry that bundle. The job fails if the bundle is missing from an artifact that redistributes the code.

## Clean install

The scripts in [smoke/](smoke/) install only from built artifacts, in a directory outside the source trees. They check:

```bash
npm install trigora @trigora/sdk
npx trigora --version
```

```bash
pip install trigora trigora-client
trigora --version
```

```text
import trigora
import trigora_cli
```

and a separate environment where `trigora-client` does not install `tcc-engine` or `trigora-cli`.

```bash
cargo install trigora-cli
trigora dev
```

`trigora`, `trigora-local`, and `tcc-rust-compile` are on `PATH` after that install. `trigora dev` reaches ready without `TRIGORA_LOCAL_BIN`.

After the registries are published, repeat those installs from the public registries in brand-new directories, including `trigora init` and a Cloud deploy from TypeScript, Python, and Rust.

## Artifacts

A package run produces:

- `.crate` files for every crate in the publish order
- npm tarballs for the packages listed above
- a `trigora-cli` wheel and a `tcc-engine` wheel for each platform that the run built
- `py3-none-any` wheels for `trigora` and `trigora-client`
- `trigora`, `trigora-local`, and `tcc-rust-compile`, plus `SHA256SUMS`
- the npm `trigora` tarball and the `trigora-cli` wheel built from that same binary set

`release-candidate/`, `dist/`, `target/`, and packed tarballs, wheels, and crate files are build output. They are not committed.
