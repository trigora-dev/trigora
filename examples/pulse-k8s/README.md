# Pulse on Kubernetes

One Pod runs the Pulse host and the example app as sidecars. They share an `emptyDir` mounted at `/pulse`. An init container, running as root, gives uid `1000` ownership of that volume so the host can set it to mode `0700`. Both containers then run as uid `1000`. The token Secret is mounted only on the Pulse container.

The host is ready after `trigora-pulse --check /pulse` completes the socket handshake. The app is ready after it finishes `register` and answers `GET /ready` on port `8080`. `terminationGracePeriodSeconds` is `30`, above the host's 10-second shutdown deadline.

## Run

Build `trigora-pulse:local` and `pulse-compose-app:local` the same way as the Compose example. Both images use `imagePullPolicy: Never`, so they must already be on the node. On kind, load them with `kind load docker-image`.

```bash
kubectl create secret generic trigora-token --from-literal=token="$TRIGORA_TOKEN"
kubectl apply -f examples/pulse-k8s/pod.yaml
kubectl wait --for=condition=Ready pod/pulse-example --timeout=180s
```

The manifest does not start an execution. Add `PULSE_EXECUTION_ID` on the app container to start one, or `PULSE_RESUME_EXECUTION_ID` to resume an id you already have.

`examples/pulse-compose/recover.sh` applies this Pod when `kubectl` can reach a `docker-desktop` or kind cluster. Otherwise Compose is the recovery proof.

## Limitations

Automatic discovery and takeover of abandoned executions are not yet implemented. A new Pod does not adopt an execution left behind by a previous one. Resume is an explicit request for an id the caller already has.

`1000` is this example's uid, not a value the Pulse binary requires. Both containers must use the same numeric user, and that user must be able to change the socket directory to mode `0700`. The host rejects a different peer uid.

The images are local. This manifest does not set the direct authority variables. Without `TRIGORA_TOKEN`, the host exits before it binds the socket.
