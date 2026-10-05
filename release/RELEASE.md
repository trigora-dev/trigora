# Release Runbook

This document describes the maintainer release process for TCC Engine and the Trigora ecosystem.

It is intentionally version-independent. Release-specific changes belong in each repository's changelog or release notes. Record the versions for a release in [versions.toml](versions.toml), and make every package manifest match that file before the tag is created.

## Versioning

### TCC Engine

TCC Engine uses CalVer:

```text
YY.MM.MICRO
```

- `YY` and `MM` identify the release month.
- `MICRO` increases monotonically within that month.
- Git tags use `v<version>`, for example `v26.10.2`.

Package API compatibility, host protocol compatibility, artifact compatibility, and language semantics are versioned independently.

### Trigora packages

Trigora packages use SemVer.

Packages may evolve independently. Major versions are coordinated only when required by breaking ecosystem compatibility.

Git tags use `v<version>` within each repository.

## Release principles

Before creating a release tag:

1. The release commit is on `main`.
2. CI is green on that exact commit.
3. Package metadata contains the intended version.
4. Package artifacts have been built from a clean checkout.
5. Clean-install smoke tests pass against those artifacts.
6. Public dependencies reference released versions, not local paths or unreleased tags.
7. License and notice files match the artifact being distributed.
8. Release notes are prepared before the tag is pushed.

A tag must never be used to discover whether the package is releasable.

Trigora code is MIT. TCC Engine components are BUSL-1.1 and stay under that license when a Trigora binary links them.

## What ships

### TCC Engine

Published surfaces may include:

- crates.io:
  - `tcc-ir`
  - `tcc-state`
  - `tcc-core`
  - `tcc-host`
  - `tcc-host-sqlite`
  - `tcc-rust-frontend`
- npm:
  - `@tcc-engine/frontend-typescript`
  - `@tcc-engine/bindings-javascript`
- PyPI:
  - `tcc-engine`

`tcc-rust-frontend` is the Rust compiler library. The other TCC crates are published so Cargo can resolve `trigora-cli`. `tcc-rust-compile` ships inside the Trigora CLI, not as its own package. Internal compiler fixtures and reference-host-only packages are not published unless explicitly added to the public package set.

### Trigora

Published surfaces include:

- CLI and local runtime:
  - crates.io: `trigora-local`, `trigora-cli`
  - npm: `trigora`
  - PyPI: `trigora-cli`
- contracts:
  - npm: `@trigora/contracts`
- TypeScript:
  - npm: `@trigora/sdk`, `@trigora/client`
- Python:
  - PyPI: `trigora`, `trigora-client`
- Rust:
  - crates.io: `trigora`, `trigora-client`

`cargo install trigora-cli` installs `trigora`, `trigora-local`, and `tcc-rust-compile`. `trigora-local` is also a library crate because `trigora-cli` depends on it. It is not a separate install.

Not every package is released on every ecosystem release.

## Workflow model

Each release repository uses three workflows:

- `ci.yml` — tests and lint; never publishes
- `package.yml` — builds and validates release artifacts; never publishes
- `release.yml` — publishes validated artifacts and creates the GitHub Release

`ci.yml` runs on pull requests and on `main`. `package.yml` runs from a manual dispatch or from `release.yml`. It does not run on push to `main`. `release.yml` runs only for an exact release tag.

`release.yml` must reuse the packaging path rather than rebuilding through a separate release-only process.

Protected release environments should hold registry credentials and approvals. Trusted publishers for PyPI, and for npm packages that already exist, match `release.yml` and the `release` environment. First-time npm packages and crates.io crates still have a short-lived token in that environment.

## Pre-release validation

From a clean checkout:

- run repository tests and lint;
- build/package every artifact that will ship;
- inspect package contents;
- install built packages into empty directories;
- run import/startup smoke tests;
- verify dependency versions;
- verify package metadata and README rendering;
- verify license and notice bundles.

Do not publish artifacts produced by an older package run after the source tree has changed. `package.yml` has to be run again from a clean checkout after the release commit.

## Publish order

Publish dependencies before dependents.

Typical order:

1. TCC Engine dependency crates
2. TCC Engine language bindings and frontends
3. Trigora local runtime and CLI
4. Contracts
5. Language SDKs and clients
6. Public-registry clean-install verification

Within each repository, follow the dependency graph rather than the chronology of an earlier release.

Structural edges:

- `trigora-cli` depends on published `trigora-local`.
- npm `trigora` and PyPI `trigora-cli` are built from the same native binaries.
- `@trigora/sdk` and `@trigora/client` depend on published `@trigora/contracts`.
- PyPI `trigora` depends on published `tcc-engine` and `trigora-cli`. `trigora-client` does not.
- crates.io `trigora` and `trigora-client` do not depend on TCC crates or on `trigora-cli`.

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

## Rust package checks

```bash
cargo package -p <crate>
```

For packages with path dependencies, confirm the packaged manifest resolves only published versions.

Use:

```bash
cargo package --list
```

to inspect the files included in the crate.

## npm package checks

For each package being released:

```bash
npm pack
```

Install the tarball into an empty directory and import its public entry points.

## Python package checks

For each Python package being released:

```bash
python -m build
```

Install the wheel into a fresh virtual environment and import its public modules.

The `trigora` wheel is `py3-none-any` and contains no `trigora` binary, no `trigora-local`, and no `_vendor` directory. The `trigora-client` wheel is `py3-none-any` and does not depend on `tcc-engine` or `trigora-cli`.

## Notices

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

A package run produces, for the packages selected by that release:

- `.crate` files for every crate being published
- npm tarballs
- a `trigora-cli` wheel and a `tcc-engine` wheel for each platform that the run built
- `py3-none-any` wheels for `trigora` and `trigora-client` when those packages ship
- `trigora`, `trigora-local`, and `tcc-rust-compile`, plus `SHA256SUMS`, when the CLI ships
- the npm `trigora` tarball and the `trigora-cli` wheel built from that same binary set

`release-candidate/`, `dist/`, `target/`, and packed tarballs, wheels, and crate files are build output. They are not committed.
