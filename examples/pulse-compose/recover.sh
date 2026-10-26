#!/usr/bin/env bash
# Manual recovery of a known execution id. A second container does not search
# for an abandoned execution. Resume happens only when PULSE_RESUME_EXECUTION_ID
# names an id the caller already has.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
uid_num=${PULSE_UID:-1000}
gid_num=${PULSE_GID:-1000}
work=$(mktemp -d /tmp/pulse6d.XXXXXX)
sock=$work/sock
log_dir=$work/log
locked=$work/locked
token_dir=$work/token
mkdir -p "$sock" "$log_dir" "$locked" "$token_dir"
auth_pid=
echo "work directory: $work"
echo "This is manual recovery of a known id. A new container does not adopt an abandoned execution."

cleanup() {
  docker compose -f "$here/compose.yaml" down --remove-orphans >/dev/null 2>&1 || true
  docker rm -f pulse6d-mode >/dev/null 2>&1 || true
  docker volume rm -f pulse6d-locked pulse6d-mode pulse6d-sock pulse6d-token >/dev/null 2>&1 || true
  if [[ -n ${auth_pid} ]]; then
    kill "$auth_pid" >/dev/null 2>&1 || true
    wait "$auth_pid" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if ! docker info >/dev/null 2>&1; then
  echo "Docker is not available. Start Docker and run examples/pulse-compose/recover.sh again." >&2
  exit 1
fi

echo "building host and CLI for the container"
bins=$work/bins
mkdir -p "$bins"
if [[ $(uname -s) == Linux ]]; then
  cargo build --release --target-dir "$root/target" -p trigora-pulse -p trigora-cli --manifest-path "$root/Cargo.toml"
  cp "$root/target/release/trigora-pulse" "$root/target/release/trigora" "$root/target/release/trigora-local" "$bins/"
else
  pulled=
  for _ in 1 2 3; do
    if docker pull rust:1-bookworm; then
      pulled=1
      break
    fi
    sleep 5
  done
  if [[ -z $pulled ]]; then
    echo "could not pull the Rust image used to build the container binaries" >&2
    exit 1
  fi
  src=$work/src
  linux_target=${PULSE_LINUX_TARGET:-/tmp/pulse6d-linux-target}
  cargo_home=${PULSE_CARGO_HOME:-/tmp/pulse6d-cargo}
  mkdir -p "$src/crates" "$src/tcc-engine" "$linux_target" "$cargo_home"
  cp "$root/Cargo.toml" "$root/Cargo.lock" "$src/"
  cp -R "$root/crates/trigora-pulse" "$root/crates/trigora-cli" "$root/crates/trigora-local" "$src/crates/"
  rsync -a --exclude target --exclude .git "$root/../tcc-engine/" "$src/tcc-engine/"
  echo "compiling in the Rust image"
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
  file "$bins/trigora-pulse" >&2
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
echo "building images"
docker build -t trigora-pulse:local "$stage"
docker build -f "$stage/Dockerfile.app" -t pulse-compose-app:local "$stage"

socket_in() {
  docker run --rm --entrypoint find --user 0:0 -v "$1":/pulse trigora-pulse:local /pulse -name pulse.sock -print
}

echo "checking a directory this user cannot write"
docker volume create pulse6d-locked >/dev/null
docker run --rm --entrypoint chown --user 0:0 -v pulse6d-locked:/pulse trigora-pulse:local 0:0 /pulse
docker run --rm --entrypoint chmod --user 0:0 -v pulse6d-locked:/pulse trigora-pulse:local 755 /pulse
set +e
docker run --rm --user "${uid_num}:${gid_num}" -v pulse6d-locked:/pulse trigora-pulse:local --ephemeral \
  >"$work/locked.out" 2>"$work/locked.err"
locked_code=$?
set -e
if [[ $locked_code -eq 0 ]]; then
  echo "a root-owned socket directory was accepted" >&2
  exit 1
fi
if ! grep -q "directory mode:" "$work/locked.err"; then
  echo "expected the directory mode error" >&2
  cat "$work/locked.err" >&2
  exit 1
fi
if [[ -n $(socket_in pulse6d-locked) ]]; then
  echo "the unwritable directory still produced a socket" >&2
  exit 1
fi

echo "checking this user can create the socket, and a different uid is refused"
docker volume create pulse6d-mode >/dev/null
docker run --rm --entrypoint chown --user 0:0 -v pulse6d-mode:/pulse trigora-pulse:local "${uid_num}:${gid_num}" /pulse
docker run -d --name pulse6d-mode --user "${uid_num}:${gid_num}" -v pulse6d-mode:/pulse trigora-pulse:local --ephemeral >/dev/null
for _ in $(seq 1 50); do
  if docker exec pulse6d-mode trigora-pulse --check /pulse >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done
docker exec pulse6d-mode trigora-pulse --check /pulse >/dev/null
mode=$(docker exec pulse6d-mode stat -c '%a' /pulse)
sock_mode=$(docker exec pulse6d-mode stat -c '%a' /pulse/pulse.sock)
if [[ $mode != 700 || $sock_mode != 600 ]]; then
  echo "socket directory mode ${mode} socket mode ${sock_mode}" >&2
  exit 1
fi
docker run --rm --entrypoint chmod --user 0:0 -v pulse6d-mode:/pulse trigora-pulse:local -R a+rwx /pulse
set +e
docker run --rm --entrypoint trigora-pulse --user "$((uid_num + 1)):${gid_num}" -v pulse6d-mode:/pulse \
  trigora-pulse:local --check /pulse >"$work/peer.out" 2>"$work/peer.err"
peer_code=$?
set -e
if [[ $peer_code -eq 0 ]]; then
  echo "a different uid completed the socket handshake" >&2
  exit 1
fi
docker rm -f pulse6d-mode >/dev/null

echo "starting the local authority"
auth_log=$work/authority.log
PULSE_LISTEN_HOST=0.0.0.0 PULSE_TEST_HOOKS=1 PULSE_WORKSPACE_TOKEN=workspace-token \
  node --experimental-strip-types "$root/../trigora-platform/apps/pulse-authority/src/serve.ts" \
  >"$auth_log" &
auth_pid=$!
port=
for _ in $(seq 1 50); do
  if grep -q '^listening ' "$auth_log"; then
    port=$(awk '/^listening / { print $2 }' "$auth_log")
    break
  fi
  sleep 0.2
done
if [[ -z $port ]]; then
  echo "the local authority did not listen" >&2
  cat "$auth_log" >&2
  exit 1
fi

export PULSE_UID=$uid_num
export PULSE_GID=$gid_num
export PULSE_LOG_DIR=$log_dir
export TRIGORA_PULSE_AUTHORITY_URL="http://host.docker.internal:${port}"
export TRIGORA_PULSE_WORKSPACE_ID=ws
export TRIGORA_PULSE_WORKSPACE_TOKEN=workspace-token
unset TRIGORA_TOKEN || true

version_of() {
  curl -fsS -H "authorization: Bearer workspace-token" \
    "http://127.0.0.1:${port}/v1/workspaces/ws/executions/$1/sql-version"
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
  echo "actual:" >&2
  cat "$log_dir/effects" >&2 || true
  echo "app logs:" >&2
  docker logs pulse6d-app >&2 || true
  echo "host logs:" >&2
  docker logs pulse6d-host >&2 || true
  exit 1
}

echo "starting execution recover-1"
export PULSE_EXECUTION_ID=recover-1
export PULSE_RESUME_EXECUTION_ID=
export PULSE_PROGRAM=
docker volume create pulse6d-sock >/dev/null
docker run --rm --entrypoint chown --user 0:0 -v pulse6d-sock:/pulse trigora-pulse:local "${uid_num}:${gid_num}" /pulse
docker run --rm --entrypoint chown --user 0:0 -v "$log_dir":/log trigora-pulse:local "${uid_num}:${gid_num}" /log
if ! docker compose -f "$here/compose.yaml" up -d --wait --force-recreate; then
  docker logs pulse6d-app >&2 || true
  docker logs pulse6d-host >&2 || true
  exit 1
fi
wait_log $'a 1\n'
before=$(version_of recover-1)
node -e 'const body=JSON.parse(process.argv[1]); if (!(body.revision >= 1) || !body.artifactHash) process.exit(1)' "$before"
sleep 1
if ! cmp -s <(printf 'a 1\n') "$log_dir/effects"; then
  echo "effect B ran before resume" >&2
  exit 1
fi

echo "stopping both containers"
docker stop -t 15 pulse6d-app pulse6d-host >/dev/null
for container in pulse6d-app pulse6d-host; do
  code=$(docker inspect -f '{{.State.ExitCode}}' "$container")
  if [[ $code != 0 ]]; then
    echo "$container exited $code" >&2
    docker logs "$container" >&2 || true
    exit 1
  fi
done
if docker logs pulse6d-host 2>&1 | grep -q "Pulse shutdown exceeded"; then
  echo "the host exceeded its shutdown deadline" >&2
  exit 1
fi
docker compose -f "$here/compose.yaml" down --remove-orphans >/dev/null

echo "resuming recover-1 on a fresh host"
export PULSE_EXECUTION_ID=
export PULSE_RESUME_EXECUTION_ID=recover-1
export PULSE_PROGRAM=
docker compose -f "$here/compose.yaml" up -d --wait --force-recreate
wait_log $'a 1\nb 2\n'
after=$(version_of recover-1)
node -e 'const before=JSON.parse(process.argv[1]); const after=JSON.parse(process.argv[2]); if (after.artifactHash !== before.artifactHash || !(after.revision > before.revision)) process.exit(1)' "$before" "$after"
a_count=$(grep -c '^a ' "$log_dir/effects")
b_count=$(grep -c '^b ' "$log_dir/effects")
if [[ $a_count != 1 || $b_count != 1 ]]; then
  echo "effects ran a=${a_count} b=${b_count}" >&2
  exit 1
fi

echo "resuming recover-1 with a different artifact"
docker stop -t 15 pulse6d-app >/dev/null
export PULSE_PROGRAM=other.ts
docker compose -f "$here/compose.yaml" up -d --wait --force-recreate --no-deps app
seen=
for _ in $(seq 1 40); do
  if docker logs pulse6d-app 2>&1 | grep -q 'waiting-for-version'; then
    seen=1
    break
  fi
  sleep 0.5
done
if [[ -z $seen ]]; then
  echo "a different artifact did not report waiting-for-version" >&2
  docker logs pulse6d-app >&2 || true
  exit 1
fi
if ! cmp -s <(printf 'a 1\nb 2\n') "$log_dir/effects"; then
  echo "a different artifact ran an effect" >&2
  exit 1
fi
docker compose -f "$here/compose.yaml" down --remove-orphans >/dev/null

echo "starting the host without durability"
docker volume create pulse6d-token >/dev/null
docker run --rm --entrypoint chown --user 0:0 -v pulse6d-token:/pulse trigora-pulse:local "${uid_num}:${gid_num}" /pulse
set +e
docker run --rm --user "${uid_num}:${gid_num}" -v pulse6d-token:/pulse trigora-pulse:local \
  >"$work/token.out" 2>"$work/token.err"
token_code=$?
set -e
if [[ $token_code != 2 ]]; then
  echo "missing token exited ${token_code}" >&2
  cat "$work/token.err" >&2
  exit 1
fi
if ! grep -q "Durability is not configured." "$work/token.err"; then
  echo "missing token did not report that durability is not configured" >&2
  cat "$work/token.err" >&2
  exit 1
fi
if [[ -n $(socket_in pulse6d-token) ]]; then
  echo "missing token still created a socket" >&2
  exit 1
fi

echo "recovery passed"
