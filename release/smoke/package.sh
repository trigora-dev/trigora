#!/usr/bin/env bash
# Build release-candidate crate packages. Does not publish.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
if [ -f "${root}/tcc-engine/scripts/package-crates.sh" ]; then
  engine="${root}/tcc-engine"
else
  engine="$(cd "${root}/../tcc-engine" && pwd)"
fi
out="${root}/release-candidate"
mkdir -p "${out}/cargo" "${out}/npm" "${out}/python"
export CARGO_TARGET_DIR="${root}/target"

bash "${engine}/scripts/package-crates.sh"
find "${engine}/target/package" -name 'tcc-*.crate' -exec cp {} "${out}/cargo/" \;

cd "$root"
cargo package --no-verify --allow-dirty -p trigora-local \
  --config 'patch.crates-io.tcc-host.path="tcc-engine/crates/tcc-host"' \
  --config 'patch.crates-io.tcc-host-sqlite.path="tcc-engine/crates/tcc-host-sqlite"' \
  --config 'patch.crates-io.tcc-state.path="tcc-engine/crates/tcc-state"' \
  --config 'patch.crates-io.tcc-ir.path="tcc-engine/crates/tcc-ir"'
cargo package --no-verify --allow-dirty -p trigora-cli \
  --config 'patch.crates-io.trigora-local.path="crates/trigora-local"' \
  --config 'patch.crates-io.tcc-rust-frontend.path="tcc-engine/frontends/rust"' \
  --config 'patch.crates-io.tcc-ir.path="tcc-engine/crates/tcc-ir"'

find "${root}/target/package" -name 'trigora-*.crate' -exec cp {} "${out}/cargo/" \;

mkdir -p "${out}/cargo/manifests"
shopt -s nullglob
for crate in "${out}/cargo/"*.crate; do
  toml="$(tar -tzf "$crate" | grep -E '/Cargo.toml$' | head -n 1)"
  dest="${out}/cargo/manifests/$(basename "$crate" .crate).toml"
  tar -xOf "$crate" "$toml" >"$dest"
  if grep -E 'path[[:space:]]*=[[:space:]]*"\.\.' "$dest"; then
    echo "path dependency remains in ${crate}" >&2
    exit 1
  fi
done

echo "crate packages are in ${out}/cargo"
