#!/usr/bin/env bash
# Start one built host with --ephemeral and require the Node adapter's authOk
# to match the version and protocol read from this tree. This does not call
# api.trigora.dev and does not publish anything.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: release/pulse/handshake.sh <trigora-pulse>" >&2
  exit 2
fi

binary=$1
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
if [[ ! -x $binary ]]; then
  echo "Pulse binary is not executable: $binary" >&2
  exit 1
fi

if [[ -d $root/tcc-engine/crates/tcc-ir ]]; then
  ir=$root/tcc-engine/crates/tcc-ir
elif [[ -d $root/../tcc-engine/crates/tcc-ir ]]; then
  ir=$(cd "$root/../tcc-engine/crates/tcc-ir" && pwd)
else
  echo "tcc-engine checkout is missing" >&2
  exit 1
fi

host_pid=
adapter_pid=
work=$(mktemp -d "${TMPDIR:-/tmp}/pulse-handshake.XXXXXX")
sock=$work/sock
mkdir -p "$sock"

cleanup() {
  if [[ -n ${adapter_pid} ]]; then
    kill "$adapter_pid" >/dev/null 2>&1 || true
    wait "$adapter_pid" >/dev/null 2>&1 || true
  fi
  if [[ -n ${host_pid} ]]; then
    kill "$host_pid" >/dev/null 2>&1 || true
    wait "$host_pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

writer=$work/writer
mkdir -p "$writer/src"
cat >"$writer/Cargo.toml" <<EOF
[package]
name = "pulse-artifact-writer"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
sha2 = "0.10"
tcc-ir = { path = "$ir" }
EOF
cp "$here/minimal_artifact.rs" "$writer/src/main.rs"
hash=$(cargo run --quiet --manifest-path "$writer/Cargo.toml" -- "$work/artifact.json")

mkdir -p "$work/handlers"
printf '%s\n' 'export const handlers = {};' >"$work/handlers/0.mjs"
python3 - "$work/manifest.json" "$hash" "$work/handlers/0.mjs" "$work/artifact.json" <<'PY'
import json
import sys
path, digest, module, artifact = sys.argv[1:]
with open(path, "w", encoding="utf-8") as file:
    json.dump(
        {
            "programs": [
                {
                    "file": "program.ts",
                    "module": module,
                    "artifact": artifact,
                    "hash": digest,
                    "keys": [],
                }
            ]
        },
        file,
    )
PY

version_line=$("$binary" --version)
printf '%s\n' "$version_line"
python3 "$here/compatibility.py" check-host --version-output "$version_line"
expected_version=$(python3 -c 'import sys; print(sys.argv[1].split()[1])' "$version_line")
expected_protocol=$(python3 -c 'import sys; print(sys.argv[1].split()[3])' "$version_line")

"$binary" --ephemeral "$sock" >"$work/host.out" 2>"$work/host.err" &
host_pid=$!
for _ in $(seq 1 50); do
  if [[ -S $sock/pulse.sock ]]; then
    break
  fi
  if ! kill -0 "$host_pid" 2>/dev/null; then
    echo "host exited before the socket appeared" >&2
    cat "$work/host.err" >&2 || true
    exit 1
  fi
  sleep 0.1
done
if [[ ! -S $sock/pulse.sock ]]; then
  echo "Pulse socket did not appear" >&2
  cat "$work/host.err" >&2 || true
  exit 1
fi

TRIGORA_PULSE_EXPECT_VERSION=$expected_version \
  TRIGORA_PULSE_EXPECT_PROTOCOL=$expected_protocol \
  node "$root/packages/cli/helper/pulse-adapter.mjs" "$sock" "$work/manifest.json" \
  >"$work/adapter.out" 2>"$work/adapter.err" &
adapter_pid=$!
ready=
for _ in $(seq 1 50); do
  if [[ -f $work/adapter.out ]] && grep -q '^ready$' "$work/adapter.out"; then
    ready=1
    break
  fi
  if ! kill -0 "$adapter_pid" 2>/dev/null; then
    echo "adapter exited before ready" >&2
    cat "$work/adapter.err" >&2 || true
    cat "$work/host.err" >&2 || true
    exit 1
  fi
  sleep 0.1
done
if [[ -z $ready ]]; then
  echo "adapter did not report ready" >&2
  cat "$work/adapter.err" >&2 || true
  exit 1
fi
if grep -q 'Incompatible Pulse host' "$work/adapter.err"; then
  echo "adapter rejected the host version" >&2
  cat "$work/adapter.err" >&2 || true
  exit 1
fi
echo "authOk matched ${expected_version} protocol ${expected_protocol}"
