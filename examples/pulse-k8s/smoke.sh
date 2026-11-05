#!/usr/bin/env bash
# Kind proof for one Pod. Resume uses PULSE_RESUME_EXECUTION_ID.
# A new Pod does not search for an abandoned execution.
# The host is not started with --ephemeral. Both Pods use one local authority.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
compose=$root/examples/pulse-compose
cluster=${PULSE_KIND_CLUSTER:-trigora-pulse}
context=kind-$cluster
node=$cluster-control-plane
uid_num=${PULSE_UID:-1000}
exec_id=k8s-1
pod=pulse-smoke
work=$(mktemp -d /tmp/pulse-k8s.XXXXXX)
auth_pid=
port=
authority_ip=
authority_url=
kc() { kubectl --context "$context" "$@"; }

echo "work directory: $work"
echo "This is manual recovery of a known id. A new Pod does not adopt an abandoned execution."

cleanup() {
  kc delete pod "$pod" pulse-authority-check --ignore-not-found --wait=false >/dev/null 2>&1 || true
  if [[ -n ${auth_pid} ]]; then
    kill "$auth_pid" >/dev/null 2>&1 || true
    wait "$auth_pid" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if ! docker info >/dev/null 2>&1; then
  echo "Docker is not available. Start Docker and run examples/pulse-k8s/smoke.sh again." >&2
  exit 1
fi
if ! command -v kind >/dev/null 2>&1; then
  echo "kind is not installed" >&2
  exit 1
fi
if ! kind get clusters | grep -qx "$cluster"; then
  kind create cluster --name "$cluster"
fi

ensure_images() {
  if docker image inspect trigora-pulse:local >/dev/null 2>&1 \
    && docker image inspect pulse-compose-app:local >/dev/null 2>&1; then
    echo "using local images trigora-pulse:local and pulse-compose-app:local"
    return
  fi
  echo "building host and CLI for the container"
  local bins=$work/bins
  mkdir -p "$bins"
  if [[ $(uname -s) == Linux ]]; then
    cargo build --release --target-dir "$root/target" -p trigora-pulse -p trigora-cli --manifest-path "$root/Cargo.toml"
    cp "$root/target/release/trigora-pulse" "$root/target/release/trigora" "$root/target/release/trigora-local" "$bins/"
  else
    local pulled=
    local _
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
    local src=$work/src
    local linux_target=${PULSE_LINUX_TARGET:-/tmp/pulse6d-linux-target}
    local cargo_home=${PULSE_CARGO_HOME:-/tmp/pulse6d-cargo}
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
  local stage=$work/image
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
  echo "building images"
  docker build -t trigora-pulse:local "$stage"
  docker build -f "$stage/Dockerfile.app" -t pulse-compose-app:local "$stage"
}

dump_pod() {
  kc describe pod "$pod" >&2 || true
  kc logs "$pod" -c pulse >&2 || true
  kc logs "$pod" -c app >&2 || true
}

wait_container() {
  local name=$1
  local _
  for _ in $(seq 1 90); do
    local ready
    ready=$(kc get pod "$pod" -o jsonpath="{.status.containerStatuses[?(@.name==\"${name}\")].ready}" 2>/dev/null || true)
    if [[ $ready == true ]]; then
      return 0
    fi
    local phase
    phase=$(kc get pod "$pod" -o jsonpath='{.status.phase}' 2>/dev/null || true)
    if [[ $phase == Failed ]]; then
      echo "container ${name} did not become ready" >&2
      dump_pod
      exit 1
    fi
    sleep 1
  done
  echo "container ${name} did not become ready" >&2
  dump_pod
  exit 1
}

read_effects() {
  if ! docker exec "$node" cat /opt/pulse-smoke/log/effects >"$work/effects" 2>/dev/null; then
    : >"$work/effects"
  fi
}

log_is() {
  read_effects
  cmp -s <(printf '%s' "$1") "$work/effects"
}

wait_log() {
  local want=$1
  local _
  for _ in $(seq 1 90); do
    if log_is "$want"; then
      return 0
    fi
    sleep 1
  done
  echo "effect log did not become:" >&2
  printf '%s\n' "$want" >&2
  echo "actual:" >&2
  cat "$work/effects" >&2 || true
  dump_pod
  exit 1
}

version_of() {
  curl -fsS -H "authorization: Bearer workspace-token" \
    "http://127.0.0.1:${port}/v1/workspaces/ws/executions/${exec_id}/sql-version"
}

write_pod() {
  local execution=$1
  local resume=$2
  local program=$3
  cat >"$work/pod.yaml" <<EOF
apiVersion: v1
kind: Pod
metadata:
  name: ${pod}
  labels:
    app: pulse-smoke
spec:
  terminationGracePeriodSeconds: 30
  hostAliases:
    - ip: "${authority_ip}"
      hostnames:
        - pulse-authority
  securityContext:
    fsGroup: ${uid_num}
    runAsUser: ${uid_num}
    runAsGroup: ${uid_num}
  initContainers:
    - name: prepare-socket
      image: trigora-pulse:local
      imagePullPolicy: Never
      command: ["chown", "${uid_num}:${uid_num}", "/pulse"]
      securityContext:
        runAsUser: 0
        runAsGroup: 0
      volumeMounts:
        - name: pulse
          mountPath: /pulse
  containers:
    - name: pulse
      image: trigora-pulse:local
      imagePullPolicy: Never
      env:
        - name: TRIGORA_PULSE_AUTHORITY_URL
          value: "${authority_url}"
        - name: TRIGORA_PULSE_WORKSPACE_ID
          value: ws
        - name: TRIGORA_PULSE_WORKSPACE_TOKEN
          value: workspace-token
      readinessProbe:
        exec:
          command: ["trigora-pulse", "--check", "/pulse"]
        periodSeconds: 2
      volumeMounts:
        - name: pulse
          mountPath: /pulse
    - name: app
      image: pulse-compose-app:local
      imagePullPolicy: Never
      env:
        - name: PULSE_DIR
          value: /pulse
        - name: PULSE_EXECUTION_ID
          value: "${execution}"
        - name: PULSE_RESUME_EXECUTION_ID
          value: "${resume}"
        - name: PULSE_PROGRAM
          value: "${program}"
        - name: PULSE_EFFECT_LOG
          value: /var/pulse-log/effects
      readinessProbe:
        httpGet:
          path: /ready
          port: 8080
        periodSeconds: 2
      ports:
        - name: ready
          containerPort: 8080
      volumeMounts:
        - name: pulse
          mountPath: /pulse
        - name: log
          mountPath: /var/pulse-log
  volumes:
    - name: pulse
      emptyDir: {}
    - name: log
      hostPath:
        path: /opt/pulse-smoke/log
        type: Directory
EOF
}

apply_pod() {
  write_pod "$1" "$2" "$3"
  kc apply -f "$work/pod.yaml" >/dev/null
}

assert_authority_env() {
  local url workspace token
  url=$(kc get pod "$pod" -o jsonpath='{.spec.containers[?(@.name=="pulse")].env[?(@.name=="TRIGORA_PULSE_AUTHORITY_URL")].value}')
  workspace=$(kc get pod "$pod" -o jsonpath='{.spec.containers[?(@.name=="pulse")].env[?(@.name=="TRIGORA_PULSE_WORKSPACE_ID")].value}')
  token=$(kc get pod "$pod" -o jsonpath='{.spec.containers[?(@.name=="pulse")].env[?(@.name=="TRIGORA_PULSE_WORKSPACE_TOKEN")].value}')
  if [[ $url != "$authority_url" || $workspace != ws || $token != workspace-token ]]; then
    echo "the Pod authority configuration is ${url} ${workspace} ${token}" >&2
    exit 1
  fi
  if kc get pod "$pod" -o json | grep -q 'TRIGORA_TOKEN'; then
    echo "the smoke Pod includes TRIGORA_TOKEN" >&2
    exit 1
  fi
  local command args
  command=$(kc get pod "$pod" -o jsonpath='{.spec.containers[?(@.name=="pulse")].command}')
  args=$(kc get pod "$pod" -o jsonpath='{.spec.containers[?(@.name=="pulse")].args}')
  if [[ -n $command || -n $args ]]; then
    echo "the host command was replaced: ${command} ${args}" >&2
    exit 1
  fi
}

assert_registration() {
  wait_container pulse
  wait_container app
  assert_authority_env
  if kc logs "$pod" -c pulse | grep -q 'Pulse running in ephemeral mode.'; then
    echo "the host fell back to an in-memory authority" >&2
    dump_pod
    exit 1
  fi
  local host_uid app_uid
  host_uid=$(kc exec "$pod" -c pulse -- id -u)
  app_uid=$(kc exec "$pod" -c app -- id -u)
  if [[ $host_uid != "$uid_num" || $app_uid != "$uid_num" ]]; then
    echo "pod users host=${host_uid} app=${app_uid}" >&2
    exit 1
  fi
  local host_dir app_dir
  host_dir=$(kc exec "$pod" -c pulse -- stat -c '%a %u' /pulse)
  app_dir=$(kc exec "$pod" -c app -- stat -c '%a %u' /pulse)
  if [[ $host_dir != "700 ${uid_num}" || $app_dir != "700 ${uid_num}" ]]; then
    echo "socket directory host=${host_dir} app=${app_dir}" >&2
    exit 1
  fi
  local started started_epoch host_stat app_stat
  started=$(kc get pod "$pod" -o jsonpath='{.status.containerStatuses[?(@.name=="pulse")].state.running.startedAt}')
  started_epoch=$(node -e 'const t=Date.parse(process.argv[1]); if(!Number.isFinite(t)) process.exit(1); process.stdout.write(String(Math.floor(t/1000)))' "$started")
  host_stat=$(kc exec "$pod" -c pulse -- stat -c '%i %Y %a %u' /pulse/pulse.sock)
  app_stat=$(kc exec "$pod" -c app -- stat -c '%i %Y %a %u' /pulse/pulse.sock)
  local host_inode host_mtime host_mode host_owner
  local app_inode app_mtime app_mode app_owner
  read -r host_inode host_mtime host_mode host_owner <<<"$host_stat"
  read -r app_inode app_mtime app_mode app_owner <<<"$app_stat"
  if [[ $host_inode != "$app_inode" || $host_mtime != "$app_mtime" ]]; then
    echo "the containers do not share one socket: host=${host_stat} app=${app_stat}" >&2
    exit 1
  fi
  if [[ $host_mode != 600 || $host_owner != "$uid_num" || $app_mode != 600 || $app_owner != "$uid_num" ]]; then
    echo "socket mode host=${host_stat} app=${app_stat}" >&2
    exit 1
  fi
  if [[ $host_mtime -lt $started_epoch ]]; then
    echo "the socket predates this host: started=${started} mtime=${host_mtime}" >&2
    exit 1
  fi
  kc exec "$pod" -c pulse -- trigora-pulse --check /pulse >/dev/null
  kc exec "$pod" -c app -- node -e 'fetch("http://127.0.0.1:8080/ready").then((response)=>process.exit(response.ok?0:1)).catch(()=>process.exit(1))'
  if ! kc logs "$pod" -c app | grep -qx 'ready'; then
    echo "the app log has no ready line from this process" >&2
    dump_pod
    exit 1
  fi
}

capture_stop() {
  local label=$1
  local dest=$work/$label
  mkdir -p "$dest"
  : >"$dest/pulse.log"
  : >"$dest/app.log"
  kc logs -f "$pod" -c pulse >"$dest/pulse.log" 2>/dev/null &
  local pulse_logs=$!
  kc logs -f "$pod" -c app >"$dest/app.log" 2>/dev/null &
  local app_logs=$!
  : >"$dest/watch.json"
  kc get pod "$pod" --watch -o json >"$dest/watch.json" 2>"$dest/watch.err" &
  local watch_pid=$!
  sleep 0.3
  kc delete pod "$pod" --wait=false --grace-period=30 >/dev/null
  local recorded=
  local _
  for _ in $(seq 1 90); do
    if node - "$dest/watch.json" "$dest/status.json" <<'JS'
const fs = require('fs');
const raw = fs.readFileSync(process.argv[2], 'utf8');
const docs = [];
let start = 0;
let depth = 0;
let inString = false;
let escape = false;
for (let i = 0; i < raw.length; i++) {
  const ch = raw[i];
  if (inString) {
    if (escape) escape = false;
    else if (ch === '\\') escape = true;
    else if (ch === '"') inString = false;
    continue;
  }
  if (ch === '"') {
    inString = true;
    continue;
  }
  if (ch === '{') {
    if (depth === 0) start = i;
    depth++;
  } else if (ch === '}') {
    depth--;
    if (depth === 0) docs.push(raw.slice(start, i + 1));
  }
}
let found = null;
for (const doc of docs) {
  let pod;
  try {
    pod = JSON.parse(doc);
  } catch {
    continue;
  }
  const item = {};
  for (const status of pod.status?.containerStatuses ?? []) {
    const term = status.state?.terminated;
    if (!term) continue;
    item[status.name] = {
      exitCode: term.exitCode,
      reason: term.reason,
      finishedAt: term.finishedAt,
    };
  }
  if (item.pulse && item.app) found = item;
}
if (!found) process.exit(2);
fs.writeFileSync(process.argv[3], JSON.stringify(found, null, 2));
JS
    then
      recorded=1
      break
    fi
    sleep 0.5
  done
  kill "$watch_pid" "$pulse_logs" "$app_logs" >/dev/null 2>&1 || true
  wait "$watch_pid" >/dev/null 2>&1 || true
  wait "$pulse_logs" >/dev/null 2>&1 || true
  wait "$app_logs" >/dev/null 2>&1 || true
  if [[ -z $recorded ]]; then
    echo "the Pod disappeared before its termination status was recorded" >&2
    cat "$dest/watch.err" >&2 || true
    exit 1
  fi
  docker exec "$node" mkdir -p "/opt/pulse-smoke/evidence/${label}"
  docker cp "$dest/status.json" "$node:/opt/pulse-smoke/evidence/${label}/status.json"
  docker cp "$dest/pulse.log" "$node:/opt/pulse-smoke/evidence/${label}/pulse.log"
  docker cp "$dest/app.log" "$node:/opt/pulse-smoke/evidence/${label}/app.log"
  if ! docker exec "$node" test -s "/opt/pulse-smoke/evidence/${label}/status.json"; then
    echo "termination status was not stored on the Kind node" >&2
    exit 1
  fi
  if ! node -e '
    const status = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    const log = require("fs").readFileSync(process.argv[2], "utf8");
    for (const name of ["pulse", "app"]) {
      const item = status[name];
      if (item.exitCode !== 0 || item.reason === "OOMKilled") {
        console.error(name + " exited " + item.exitCode + " " + item.reason);
        process.exit(1);
      }
    }
    if (log.includes("Pulse shutdown exceeded")) process.exit(1);
  ' "$dest/status.json" "$dest/pulse.log"; then
    echo "the host did not exit within its deadline" >&2
    cat "$dest/status.json" >&2
    cat "$dest/pulse.log" >&2
    exit 1
  fi
  for _ in $(seq 1 60); do
    if ! kc get pod "$pod" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.5
  done
  echo "the Pod was not removed" >&2
  exit 1
}

reach_authority() {
  local where=$1
  shift
  if ! "$@"; then
    echo "the authority is not reachable ${where}" >&2
    echo "address ${authority_ip} port ${port}" >&2
    echo "url ${authority_url}/v1/test/renewals/preflight" >&2
    return 1
  fi
}

ensure_images
echo "loading images into ${cluster}"
kind load docker-image trigora-pulse:local --name "$cluster"
kind load docker-image pulse-compose-app:local --name "$cluster"

echo "starting the local authority"
auth_log=$work/authority.log
PULSE_LISTEN_HOST=0.0.0.0 PULSE_TEST_HOOKS=1 PULSE_WORKSPACE_TOKEN=workspace-token \
  node --experimental-strip-types "$root/../trigora-platform/apps/pulse-authority/src/serve.ts" \
  >"$auth_log" &
auth_pid=$!
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
echo "authority port ${port}; it is not published as a Service or a Docker port"

authority_ip=$(docker exec "$node" getent hosts host.docker.internal | awk '{ print $1; exit }')
if [[ -z $authority_ip ]]; then
  echo "the Kind node cannot resolve host.docker.internal" >&2
  exit 1
fi
authority_url=http://pulse-authority:${port}
check_path=/v1/test/renewals/preflight

echo "checking the authority from the Kind node"
if ! reach_authority "from the Kind node" \
  docker run --rm --network "container:${node}" --entrypoint node pulse-compose-app:local -e "
    fetch('http://${authority_ip}:${port}${check_path}').then(async (response) => {
      const body = await response.text();
      if (response.status !== 200 || body !== '0') {
        console.error(response.status);
        console.error(body);
        process.exit(1);
      }
      process.exit(0);
    }).catch((error) => {
      console.error(error instanceof Error ? error.message : String(error));
      process.exit(1);
    })
  "; then
  exit 1
fi

echo "checking the authority from a Pod"
cat >"$work/check.yaml" <<EOF
apiVersion: v1
kind: Pod
metadata:
  name: pulse-authority-check
spec:
  restartPolicy: Never
  hostAliases:
    - ip: "${authority_ip}"
      hostnames:
        - pulse-authority
  containers:
    - name: check
      image: pulse-compose-app:local
      imagePullPolicy: Never
      env:
        - name: PULSE_AUTHORITY_CHECK
          value: "${authority_url}${check_path}"
      command:
        - node
        - -e
        - |
          fetch(process.env.PULSE_AUTHORITY_CHECK).then(async (response) => {
            const body = await response.text();
            if (response.status !== 200 || body !== '0') {
              console.error(response.status);
              console.error(body);
              process.exit(1);
            }
            process.exit(0);
          }).catch((error) => {
            console.error(error instanceof Error ? error.message : String(error));
            process.exit(1);
          });
EOF
kc delete pod pulse-authority-check --ignore-not-found --wait=true >/dev/null 2>&1 || true
kc apply -f "$work/check.yaml" >/dev/null
check_phase=
for _ in $(seq 1 40); do
  check_phase=$(kc get pod pulse-authority-check -o jsonpath='{.status.phase}' 2>/dev/null || true)
  if [[ $check_phase == Succeeded ]]; then
    break
  fi
  if [[ $check_phase == Failed ]]; then
    break
  fi
  sleep 1
done
if [[ $check_phase != Succeeded ]]; then
  echo "the authority is not reachable from a Pod" >&2
  echo "address ${authority_ip} port ${port}" >&2
  echo "url ${authority_url}${check_path}" >&2
  kc logs pulse-authority-check >&2 || true
  kc describe pod pulse-authority-check >&2 || true
  exit 1
fi
kc delete pod pulse-authority-check --wait=false >/dev/null 2>&1 || true

docker exec "$node" mkdir -p /opt/pulse-smoke/log /opt/pulse-smoke/evidence
docker exec "$node" chown "${uid_num}:${uid_num}" /opt/pulse-smoke/log
docker exec "$node" rm -f /opt/pulse-smoke/log/effects

echo "starting execution ${exec_id}"
kc delete pod "$pod" --ignore-not-found --wait=true >/dev/null 2>&1 || true
apply_pod "$exec_id" "" ""
assert_registration
wait_log $'a 1\n'
before=$(version_of)
node -e 'const body=JSON.parse(process.argv[1]); if (!(body.revision >= 1) || !body.artifactHash) process.exit(1)' "$before"
printf '%s\n' "$before" >"$work/version-before.json"
sleep 1
if ! log_is $'a 1\n'; then
  echo "effect B ran before resume" >&2
  exit 1
fi

echo "stopping the Pod"
capture_stop start

echo "resuming ${exec_id} on a fresh Pod"
apply_pod "" "$exec_id" ""
assert_registration
wait_log $'a 1\nb 2\n'
after=$(version_of)
printf '%s\n' "$after" >"$work/version-after.json"
node -e 'const before=JSON.parse(process.argv[1]); const after=JSON.parse(process.argv[2]); if (after.artifactHash !== before.artifactHash || !(after.revision > before.revision)) process.exit(1)' "$before" "$after"
a_count=$(docker exec "$node" grep -c '^a ' /opt/pulse-smoke/log/effects || true)
b_count=$(docker exec "$node" grep -c '^b ' /opt/pulse-smoke/log/effects || true)
if [[ $a_count != 1 || $b_count != 1 ]]; then
  echo "effects ran a=${a_count} b=${b_count}" >&2
  exit 1
fi

echo "stopping the replacement Pod"
capture_stop resume

echo "resuming ${exec_id} with a different artifact"
apply_pod "" "$exec_id" "other.ts"
assert_registration
seen=
for _ in $(seq 1 45); do
  if kc logs "$pod" -c app 2>/dev/null | grep -q 'waiting-for-version'; then
    seen=1
    break
  fi
  sleep 1
done
if [[ -z $seen ]]; then
  echo "a different artifact did not report waiting-for-version" >&2
  dump_pod
  exit 1
fi
if ! log_is $'a 1\nb 2\n'; then
  echo "a different artifact ran an effect" >&2
  exit 1
fi
held=$(version_of)
node -e 'const after=JSON.parse(process.argv[1]); const held=JSON.parse(process.argv[2]); if (held.artifactHash !== after.artifactHash || held.revision !== after.revision) process.exit(1)' "$after" "$held"

echo "Kind smoke passed. A new Pod does not adopt an abandoned execution."
