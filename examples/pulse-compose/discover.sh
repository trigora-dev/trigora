#!/usr/bin/env bash
# Automatic recovery. The replacement host and app receive neither an execution
# id nor a resume command. The harness keeps the id only to inspect the checkpoint.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
uid_num=${PULSE_UID:-1000}
gid_num=${PULSE_GID:-1000}
if [[ -z ${TRIGORA_TOKEN:-} || -z ${TRIGORA_API_BASE_URL:-} ]]; then
  echo "TRIGORA_TOKEN and TRIGORA_API_BASE_URL are required" >&2
  exit 1
fi
case $TRIGORA_API_BASE_URL in
  *api.trigora.dev*)
    echo "discover.sh refuses the production API" >&2
    exit 1
    ;;
esac

work=$(mktemp -d /tmp/pulse7a.XXXXXX)
sock=$work/sock
log_dir=$work/log
mkdir -p "$sock" "$log_dir"
echo "work directory: $work"
id="discover-$(date +%s)"
echo "harness execution id: $id"

cleanup() {
  docker compose -f "$here/compose.yaml" down --remove-orphans >/dev/null 2>&1 || true
  docker volume rm -f pulse6d-sock >/dev/null 2>&1 || true
}
trap cleanup EXIT

if ! docker info >/dev/null 2>&1; then
  echo "Docker is not available." >&2
  exit 1
fi

echo "building host and CLI for the container"
bins=$work/bins
mkdir -p "$bins"
if [[ $(uname -s) == Linux ]]; then
  cargo build --release --target-dir "$root/target" -p trigora-pulse -p trigora-cli --manifest-path "$root/Cargo.toml"
  cp "$root/target/release/trigora-pulse" "$root/target/release/trigora" "$root/target/release/trigora-local" "$bins/"
else
  docker pull rust:1-bookworm
  src=$work/src
  linux_target=${PULSE_LINUX_TARGET:-/tmp/pulse6d-linux-target}
  cargo_home=${PULSE_CARGO_HOME:-/tmp/pulse6d-cargo}
  mkdir -p "$src/crates" "$src/tcc-engine" "$linux_target" "$cargo_home"
  cp "$root/Cargo.toml" "$root/Cargo.lock" "$src/"
  cp -R "$root/crates/trigora-pulse" "$root/crates/trigora-cli" "$root/crates/trigora-local" "$src/crates/"
  rsync -a --exclude target --exclude .git "$root/../tcc-engine/" "$src/tcc-engine/"
  docker run --rm \
    -v "$src":/src \
    -v "$linux_target":/linux-target \
    -v "$cargo_home":/cargo-home \
    -w /src \
    -e CARGO_TARGET_DIR=/linux-target \
    -e CARGO_HOME=/cargo-home \
    rust:1-bookworm \
    bash -c 'export PATH=/usr/local/cargo/bin:$PATH; export DEBIAN_FRONTEND=noninteractive; apt-get update && apt-get install -y --no-install-recommends pkg-config libclang-dev && cargo build --release -p trigora-pulse -p trigora-cli --manifest-path /src/Cargo.toml'
  cp "$linux_target/release/trigora-pulse" "$linux_target/release/trigora" "$linux_target/release/trigora-local" "$bins/"
fi
if ! file "$bins/trigora-pulse" | grep -q 'ELF'; then
  echo "the host binary is not a Linux executable" >&2
  exit 1
fi

stage=$work/image
mkdir -p "$stage/helper" "$stage/app"
cp "$bins/trigora-pulse" "$stage/trigora-pulse"
cp "$root/crates/trigora-pulse/Dockerfile" "$stage/Dockerfile"
cp "$bins/trigora" "$bins/trigora-local" "$stage/"
cp "$root/packages/cli/helper/node-helper.js" \
  "$root/packages/cli/helper/pulse-adapter.mjs" \
  "$root/packages/cli/helper/ts-adapter.js" \
  "$stage/helper/"
mkdir -p "$stage/node_modules/@tcc-engine"
cp -RL "$root/packages/cli/node_modules/typescript" "$stage/node_modules/typescript"
cp -RL "$root/packages/cli/node_modules/@tcc-engine/frontend-typescript" \
  "$stage/node_modules/@tcc-engine/frontend-typescript"
cp "$here/Dockerfile.app" "$stage/Dockerfile.app"
cp -R "$here/app/." "$stage/app/"
docker build -t trigora-pulse:local "$stage"
docker build -f "$stage/Dockerfile.app" -t pulse-compose-app:local "$stage"

export PULSE_UID=$uid_num
export PULSE_GID=$gid_num
export PULSE_LOG_DIR=$log_dir
export TRIGORA_PULSE_LEASE_MS=15000
unset TRIGORA_PULSE_AUTHORITY_URL || true
unset TRIGORA_PULSE_WORKSPACE_ID || true
unset TRIGORA_PULSE_WORKSPACE_TOKEN || true

api=${TRIGORA_API_BASE_URL%/}
version_of() {
  curl -fsS -H "authorization: Bearer ${TRIGORA_TOKEN}" \
    "$api/v1/pulse/executions/$1/sql-version"
}

wait_log() {
  local want=$1
  for _ in $(seq 1 90); do
    if [[ -f $log_dir/effects ]] && cmp -s <(printf '%s' "$want") "$log_dir/effects"; then
      return 0
    fi
    sleep 0.5
  done
  echo "effect log did not become:" >&2
  printf '%s\n' "$want" >&2
  cat "$log_dir/effects" >&2 || true
  docker logs pulse6d-app >&2 || true
  docker logs pulse6d-host >&2 || true
  exit 1
}

compose_up() {
  if docker compose -f "$here/compose.yaml" up -d --wait --force-recreate; then
    return 0
  fi
  echo "compose up failed" >&2
  docker inspect pulse6d-app --format 'app status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} error={{.State.Error}}' >&2 || true
  docker logs pulse6d-app >&2 || true
  docker logs pulse6d-host >&2 || true
  return 1
}

refuse_identity() {
  local name=$1
  local env args
  env=$(docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$name")
  if printf '%s\n' "$env" | grep -Eq '^(PULSE_EXECUTION_ID|PULSE_RESUME_EXECUTION_ID)=.+'; then
    echo "$name received an execution id" >&2
    printf '%s\n' "$env" >&2
    exit 1
  fi
  args=$(docker inspect -f '{{json .Args}}{{json .Config.Entrypoint}}{{json .Config.Cmd}}' "$name")
  if printf '%s\n' "$args" | grep -q 'resume'; then
    echo "$name received a resume command" >&2
    printf '%s\n' "$args" >&2
    exit 1
  fi
}

echo "starting $id"
export PULSE_EXECUTION_ID=$id
export PULSE_RESUME_EXECUTION_ID=
export PULSE_PROGRAM=
docker volume create pulse6d-sock >/dev/null
docker run --rm --entrypoint chown --user 0:0 -v pulse6d-sock:/pulse trigora-pulse:local "${uid_num}:${gid_num}" /pulse
docker run --rm --entrypoint chown --user 0:0 -v "$log_dir":/log trigora-pulse:local "${uid_num}:${gid_num}" /log
compose_up
wait_log $'a 1\n'
before=$(version_of "$id")

echo "stopping both containers"
docker stop -t 15 pulse6d-app pulse6d-host >/dev/null
owner=$(docker logs pulse6d-host 2>&1 | awk '/^Pulse owner / { print $3 }' | tail -1)
if [[ -z $owner ]]; then
  echo "the host did not report an owner" >&2
  docker logs pulse6d-host >&2 || true
  exit 1
fi
docker compose -f "$here/compose.yaml" down --remove-orphans >/dev/null

echo "accepting the continue event while no host owns the process"
frame=$work/frame
curl -fsS -H "authorization: Bearer ${TRIGORA_TOKEN}" -H "x-pulse-owner: ${owner}" \
  "$api/v1/pulse/executions/$id/sql-restore" -o "$frame"
node --input-type=module - "$frame" "$work/event.json" "$root/packages/cli/helper/pulse-adapter.mjs" <<'EOF'
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';
const [framePath, bodyPath, adapterPath] = process.argv.slice(2);
const frame = fs.readFileSync(framePath);
const length = frame.readUInt32BE(0);
const header = JSON.parse(frame.subarray(4, 4 + length).toString());
const wait = (header.waits ?? []).find((item) => item.event === 'continue');
if (!wait) {
  console.error('continue wait missing');
  process.exit(1);
}
const { encodePayload } = await import(pathToFileURL(adapterPath).href);
fs.writeFileSync(bodyPath, JSON.stringify({
  fence: header.fence,
  waitId: wait.waitId,
  eventName: 'continue',
  payload: encodePayload('ok'),
}));
EOF
curl -fsS -X POST -H "authorization: Bearer ${TRIGORA_TOKEN}" \
  -H "content-type: application/json" -H "x-pulse-owner: ${owner}" \
  --data-binary @"$work/event.json" \
  "$api/v1/pulse/executions/$id/sql-events" >/dev/null

echo "waiting for the stopped host's lease to expire"
sleep 16

echo "replacing the containers without an execution id"
export PULSE_EXECUTION_ID=
export PULSE_RESUME_EXECUTION_ID=
export PULSE_PROGRAM=
compose_up
refuse_identity pulse6d-host
refuse_identity pulse6d-app
wait_log $'a 1\nb 2\n'
after=$(version_of "$id")
node -e 'const before=JSON.parse(process.argv[1]); const after=JSON.parse(process.argv[2]); if (after.artifactHash !== before.artifactHash || !(after.revision > before.revision)) { console.error("replacement did not advance the checkpoint", before, after); process.exit(1) }' "$before" "$after"
a_count=$(grep -c '^a ' "$log_dir/effects")
b_count=$(grep -c '^b ' "$log_dir/effects")
if [[ $a_count != 1 || $b_count != 1 ]]; then
  echo "effects ran a=${a_count} b=${b_count}" >&2
  exit 1
fi

echo "an incompatible adapter must not claim or run"
docker stop -t 15 pulse6d-app pulse6d-host >/dev/null
docker compose -f "$here/compose.yaml" down --remove-orphans >/dev/null
sleep 16
pinned=$(version_of "$id")
node -e 'const seen=JSON.parse(process.argv[1]); const pinned=JSON.parse(process.argv[2]); if (pinned.artifactHash !== seen.artifactHash || pinned.revision < seen.revision) { console.error("revision moved backward", seen, pinned); process.exit(1) }' "$after" "$pinned"
export PULSE_EXECUTION_ID=
export PULSE_RESUME_EXECUTION_ID=
export PULSE_PROGRAM=other.ts
compose_up
refuse_identity pulse6d-host
refuse_identity pulse6d-app
sleep 8
if ! cmp -s <(printf 'a 1\nb 2\n') "$log_dir/effects"; then
  echo "an incompatible adapter ran an effect" >&2
  exit 1
fi
node -e 'const pinned=JSON.parse(process.argv[1]); const now=JSON.parse(process.argv[2]); if (now.artifactHash !== pinned.artifactHash || now.revision !== pinned.revision) { console.error("incompatible adapter changed the checkpoint", pinned, now); process.exit(1) }' "$pinned" "$(version_of "$id")"
docker compose -f "$here/compose.yaml" down --remove-orphans >/dev/null

echo "automatic recovery passed"
