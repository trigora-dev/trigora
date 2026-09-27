#!/usr/bin/env bash
# Install release artifacts from release-candidate/ and exercise them.
# The script does not publish and does not read credentials.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="${root}/release-candidate"
tmp="$(mktemp -d)"
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT

fail() {
  echo "smoke: $*" >&2
  exit 1
}

test -d "$out" || fail "missing ${out}. Run release/smoke/package.sh first."

if compgen -G "${out}/npm/trigora-0*.tgz" >/dev/null; then
  npm_home="${tmp}/npm"
  mkdir -p "$npm_home"
  install_args=()
  shopt -s nullglob
  for pattern in "trigora-contracts-*.tgz" "tcc-engine-frontend-typescript-*.tgz" "trigora-0*.tgz" "trigora-sdk-*.tgz" "trigora-client-*.tgz"; do
    for file in "${out}/npm/"${pattern}; do
      install_args+=("$file")
    done
  done
  shopt -u nullglob
  if [ "${#install_args[@]}" -eq 0 ]; then
    fail "no npm tarballs matched in ${out}/npm"
  fi
  npm install --prefix "$npm_home" "${install_args[@]}"
  "$npm_home/node_modules/.bin/trigora" --version >"$npm_home/version.out" 2>"$npm_home/version.err"
  cat "$npm_home/version.out" "$npm_home/version.err"
  grep -q . "$npm_home/version.out" "$npm_home/version.err" || fail "npm trigora --version was empty"
  if [ -d "$npm_home/node_modules/@trigora/sdk" ] && [ -d "$npm_home/node_modules/@trigora/client" ]; then
    (
      cd "$npm_home"
      node --input-type=module -e "import { effect } from '@trigora/sdk'; import { createClient } from '@trigora/client'; const client = createClient({ url: 'http://127.0.0.1:9' }); if (typeof effect !== 'function' || typeof client.start !== 'function') process.exit(1)"
    )
    echo "npm authoring imports ok"
  fi
  proj="${tmp}/ts-dev"
  mkdir -p "$proj/src"
  printf '%s\n' '[project]' 'name = "smoke"' 'programs = ["src/program.ts"]' >"$proj/trigora.toml"
  printf '%s\n' 'export default async function run() { return "ok"; }' >"$proj/src/program.ts"
  (
    cd "$proj"
    env -u TRIGORA_LOCAL_BIN -u TRIGORA_BIN -u TRIGORA_NODE_HELPER \
      "$npm_home/node_modules/.bin/trigora" dev >"$proj/out" 2>"$proj/err" &
    pid=$!
    for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
      if grep -q "Local runtime ready" "$proj/out"; then
        kill "$pid" || true
        wait "$pid" || true
        echo "npm trigora dev reached ready"
        exit 0
      fi
      sleep 1
    done
    cat "$proj/out" "$proj/err" || true
    kill "$pid" || true
    exit 1
  )
fi

if compgen -G "${out}/python/trigora_cli-"*.whl >/dev/null && compgen -G "${out}/python/trigora-0"*.whl >/dev/null && compgen -G "${out}/python/tcc_engine-"*.whl >/dev/null; then
  py="${tmp}/py"
  python3 -m venv "$py"
  "$py/bin/pip" install --no-index --find-links "${out}/python" trigora
  "$py/bin/python" -c "import trigora, trigora_cli"
  "$py/bin/trigora" --version
  client="${tmp}/client"
  python3 -m venv "$client"
  "$client/bin/pip" install --no-index --find-links "${out}/python" trigora-client
  "$client/bin/python" -c "import trigora_client; import importlib.util as u; assert u.find_spec('trigora') is None; assert u.find_spec('trigora_cli') is None; assert u.find_spec('tcc_engine') is None"
fi

if compgen -G "${out}/cargo/trigora-cli-"*.crate >/dev/null; then
  cargo_root="${tmp}/cargo"
  mkdir -p "$cargo_root"
  # The packaged crate depends on registry versions. Until those versions are
  # published, install the workspace package, which is the same three binaries.
  cargo install --path "${root}/crates/trigora-cli" --root "$cargo_root" --locked --force
  test -x "${cargo_root}/bin/trigora"
  test -x "${cargo_root}/bin/trigora-local"
  test -x "${cargo_root}/bin/tcc-rust-compile"
  env -u TRIGORA_LOCAL_BIN -u TRIGORA_BIN -u TRIGORA_NODE_HELPER \
    "${cargo_root}/bin/trigora" --version >"$cargo_root/version.out" 2>"$cargo_root/version.err"
  cat "$cargo_root/version.out" "$cargo_root/version.err"
  grep -q . "$cargo_root/version.out" "$cargo_root/version.err" || fail "cargo trigora --version was empty"
  proj="${tmp}/rust-dev"
  mkdir -p "$proj/src"
  printf '%s\n' '[project]' 'name = "smoke"' 'programs = ["src/lib.rs"]' >"$proj/trigora.toml"
  printf '%s\n' 'pub async fn main() -> Result<String, String> {' '    Ok(String::from("ok"))' '}' >"$proj/src/lib.rs"
  (
    cd "$proj"
    env -u TRIGORA_LOCAL_BIN -u TRIGORA_BIN -u TRIGORA_NODE_HELPER \
      "${cargo_root}/bin/trigora" dev >"$proj/out" 2>"$proj/err" &
    pid=$!
    for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
      if grep -q "Local runtime ready" "$proj/out"; then
        kill "$pid" || true
        wait "$pid" || true
        echo "cargo install trigora-cli: trigora dev reached ready"
        exit 0
      fi
      sleep 1
    done
    cat "$proj/out" "$proj/err" || true
    kill "$pid" || true
    exit 1
  )
fi

echo "clean install smoke passed"
