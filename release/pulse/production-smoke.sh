#!/usr/bin/env bash
# Build the production Pulse image and restore one execution through
# https://api.trigora.dev. The execution id is unique. Local containers,
# volumes, and temp files are removed. The production checkpoint is left in place.
#
# The Pulse container receives TRIGORA_TOKEN and no direct-authority variables.
# The script prints the source commit and the local image id. That id is not a
# GHCR release digest. Set PULSE_IMAGE_PLATFORM=linux/amd64 or linux/arm64 to
# build that candidate. A pass on one architecture does not cover the other.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
compose=$root/examples/pulse-compose
uid_num=${PULSE_UID:-$(id -u)}
gid_num=${PULSE_GID:-$(id -g)}
platform=${PULSE_IMAGE_PLATFORM:-}

if [[ -z ${TRIGORA_TOKEN:-} || -z ${TRIGORA_TOKEN//[[:space:]]/} ]]; then
  echo "TRIGORA_TOKEN is required. The value is not printed or written to disk." >&2
  exit 2
fi
unset TRIGORA_PULSE_AUTHORITY_URL TRIGORA_PULSE_WORKSPACE_ID TRIGORA_PULSE_WORKSPACE_TOKEN

if ! docker info >/dev/null 2>&1; then
  echo "Docker is not available." >&2
  exit 1
fi

if [[ -n "$(git -C "$root" status --porcelain)" ]]; then
  echo "Error: working tree is dirty; commit changes before running production smoke." >&2
  git -C "$root" status --short >&2
  exit 1
fi

suffix=$(python3 -c 'import secrets,time; print(f"{int(time.time())}-{secrets.token_hex(4)}")')
execution_id="pulse6e-${suffix}"
host_name="pulse6e-host-${suffix}"
app_name="pulse6e-app-${suffix}"
volume="pulse6e-sock-${suffix}"
work=$(mktemp -d "${TMPDIR:-/tmp}/pulse6e.XXXXXX")
log_dir=$work/log
mkdir -p "$log_dir"
commit=$(git -C "$root" rev-parse HEAD)

cleanup() {
  docker rm -f "$host_name" "$app_name" >/dev/null 2>&1 || true
  docker volume rm -f "$volume" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

echo "source commit: $commit"
echo "execution id: $execution_id"

build_in_docker=0
if [[ $(uname -s) != Linux || -n $platform ]]; then
  build_in_docker=1
fi

bins=$work/bins
mkdir -p "$bins"
if [[ $build_in_docker -eq 0 ]]; then
  cargo build --release --target-dir "$root/target" -p trigora-pulse -p trigora-cli --manifest-path "$root/Cargo.toml"
  cp "$root/target/release/trigora-pulse" "$root/target/release/trigora" "$root/target/release/trigora-local" "$bins/"
else
  src=$work/src
  linux_target=$work/linux-target
  cargo_home=$work/cargo-home
  mkdir -p "$src/crates" "$src/tcc-engine" "$linux_target" "$cargo_home"
  cp "$root/Cargo.toml" "$root/Cargo.lock" "$src/"
  cp -R "$root/crates/trigora-pulse" "$root/crates/trigora-cli" "$root/crates/trigora-local" "$src/crates/"
  rsync -a --exclude target --exclude .git "$root/../tcc-engine/" "$src/tcc-engine/"
  docker_run=(docker run --rm)
  if [[ -n $platform ]]; then
    docker_run+=(--platform "$platform")
  fi
  "${docker_run[@]}" \
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

for required in \
  "$root/packages/cli/node_modules/typescript" \
  "$root/packages/cli/node_modules/@tcc-engine/frontend-typescript"
do
  if [[ ! -d $required ]]; then
    echo "missing $required" >&2
    echo "Install the CLI helper dependencies before running this smoke." >&2
    exit 1
  fi
done

stage=$work/image
mkdir -p "$stage/helper" "$stage/app" "$stage/node_modules/@tcc-engine"
cp "$bins/trigora-pulse" "$stage/trigora-pulse"
cp "$root/crates/trigora-pulse/Dockerfile" "$stage/Dockerfile"
cp "$bins/trigora" "$bins/trigora-local" "$stage/"
cp "$root/packages/cli/helper/node-helper.js" \
  "$root/packages/cli/helper/pulse-adapter.mjs" \
  "$root/packages/cli/helper/ts-adapter.js" \
  "$stage/helper/"
cp -RL "$root/packages/cli/node_modules/typescript" "$stage/node_modules/typescript"
cp -RL "$root/packages/cli/node_modules/@tcc-engine/frontend-typescript" \
  "$stage/node_modules/@tcc-engine/frontend-typescript"
cp "$compose/Dockerfile.app" "$stage/Dockerfile.app"
cp -R "$compose/app/." "$stage/app/"

build_image() {
  local file=$1
  local tag=$2
  if [[ -n $platform ]]; then
    docker build --platform "$platform" -f "$file" -t "$tag" "$stage"
  else
    docker build -f "$file" -t "$tag" "$stage"
  fi
}
build_image "$stage/Dockerfile" trigora-pulse:candidate
build_image "$stage/Dockerfile.app" pulse-6e-app:candidate

image_id=$(docker image inspect -f '{{.Id}}' trigora-pulse:candidate)
image_arch=$(docker image inspect -f '{{.Architecture}}' trigora-pulse:candidate)
case "$image_arch" in
  amd64) file "$bins/trigora-pulse" | grep -q -E 'x86-64|x86_64' ;;
  arm64) file "$bins/trigora-pulse" | grep -q -E 'ARM aarch64|aarch64' ;;
  *)
    echo "unexpected image architecture ${image_arch}" >&2
    exit 1
    ;;
esac
echo "local candidate image: trigora-pulse:candidate"
echo "local candidate architecture: ${image_arch}"
echo "local candidate image id: ${image_id}"
echo "This image id is the local candidate. It is not a GHCR release digest."

wait_log() {
  local want=$1
  for _ in $(seq 1 90); do
    if [[ -f $log_dir/effects ]] && cmp -s <(printf '%s' "$want") "$log_dir/effects"; then
      return 0
    fi
    sleep 0.5
  done
  echo "effect log did not match" >&2
  cat "$log_dir/effects" >&2 || true
  docker logs "$app_name" >&2 || true
  docker logs "$host_name" >&2 || true
  exit 1
}

wait_host() {
  for _ in $(seq 1 60); do
    if docker exec "$host_name" trigora-pulse --check /pulse >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.5
  done
  echo "host did not become ready" >&2
  docker logs "$host_name" >&2 || true
  exit 1
}

wait_app() {
  for _ in $(seq 1 60); do
    if docker exec "$app_name" node -e 'fetch("http://127.0.0.1:8080/ready").then((response)=>process.exit(response.ok?0:1)).catch(()=>process.exit(1))' >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.5
  done
  echo "app did not become ready" >&2
  docker logs "$app_name" >&2 || true
  docker logs "$host_name" >&2 || true
  exit 1
}

version_of() {
  local out=$work/version.json
  local code
  code=$(curl -sS -o "$out" -w '%{http_code}' \
    -H "Authorization: Bearer ${TRIGORA_TOKEN}" \
    "https://api.trigora.dev/v1/pulse/executions/${execution_id}/sql-version") || {
    echo "sql-version request failed" >&2
    exit 1
  }
  if [[ $code != 200 ]]; then
    echo "sql-version returned HTTP ${code}" >&2
    python3 -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).read_text()[:500])' "$out" >&2 || true
    exit 1
  fi
  python3 -c 'import json,sys; body=json.load(open(sys.argv[1])); print(body["revision"]); print(body["artifactHash"])' "$out"
}

prepare_volume() {
  docker volume create "$volume" >/dev/null
  docker run --rm --entrypoint chown --user 0:0 -v "$volume":/pulse trigora-pulse:candidate "${uid_num}:${gid_num}" /pulse
  docker run --rm --entrypoint chown --user 0:0 -v "$log_dir":/log trigora-pulse:candidate "${uid_num}:${gid_num}" /log
}

start_pair() {
  local start_id=$1
  local resume_id=$2
  docker rm -f "$host_name" "$app_name" >/dev/null 2>&1 || true
  docker volume rm -f "$volume" >/dev/null 2>&1 || true
  prepare_volume
  docker run -d --name "$host_name" \
    --user "${uid_num}:${gid_num}" \
    -e TRIGORA_TOKEN \
    -v "$volume":/pulse \
    trigora-pulse:candidate >/dev/null
  wait_host
  docker run -d --name "$app_name" \
    --user "${uid_num}:${gid_num}" \
    -e PULSE_DIR=/pulse \
    -e PULSE_EFFECT_LOG=/var/pulse-log/effects \
    -e "PULSE_EXECUTION_ID=${start_id}" \
    -e "PULSE_RESUME_EXECUTION_ID=${resume_id}" \
    -v "$volume":/pulse \
    -v "$log_dir":/var/pulse-log \
    pulse-6e-app:candidate >/dev/null
  wait_app
}

stop_pair() {
  docker stop -t 15 "$app_name" "$host_name" >/dev/null
  local container code
  for container in "$app_name" "$host_name"; do
    code=$(docker inspect -f '{{.State.ExitCode}}' "$container")
    if [[ $code != 0 ]]; then
      echo "$container exited $code" >&2
      docker logs "$container" >&2 || true
      exit 1
    fi
  done
  if docker logs "$host_name" 2>&1 | grep -q "Pulse shutdown exceeded"; then
    echo "the host exceeded its shutdown deadline" >&2
    exit 1
  fi
}

echo "starting ${execution_id}"
start_pair "$execution_id" ""
wait_log $'a 1\n'
sleep 1
if ! cmp -s <(printf 'a 1\n') "$log_dir/effects"; then
  echo "effect B ran before resume" >&2
  exit 1
fi
before=$(version_of) || exit 1
python3 -c 'import sys; revision,digest=sys.argv[1].splitlines();
raise SystemExit(0 if int(revision) >= 1 and digest else 1)' "$before"

echo "stopping both containers"
stop_pair

echo "resuming ${execution_id} on a fresh container"
start_pair "" "$execution_id"
wait_log $'a 1\nb 2\n'
after=$(version_of) || exit 1
python3 -c 'import sys; before=sys.argv[1].splitlines(); after=sys.argv[2].splitlines();
raise SystemExit(0 if after[1]==before[1] and int(after[0])>int(before[0]) else 1)' "$before" "$after"
a_count=$(grep -c '^a ' "$log_dir/effects")
b_count=$(grep -c '^b ' "$log_dir/effects")
if [[ $a_count != 1 || $b_count != 1 ]]; then
  echo "effects ran a=${a_count} b=${b_count}" >&2
  exit 1
fi

echo "production-token recovery passed"
echo "source commit: $commit"
echo "execution id: $execution_id"
echo "local candidate image id: ${image_id}"
echo "This image id is the local candidate. It is not a GHCR release digest."
echo "The production checkpoint for ${execution_id} was left in place."
