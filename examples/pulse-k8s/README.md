# Pulse on Kubernetes

One Pod runs the Pulse host and the example app as sidecars. They share an `emptyDir` mounted at `/pulse`. An init container, running as root, gives uid `1000` ownership of that volume so the host can set it to mode `0700`. Both containers then run as uid `1000`. The token Secret is mounted only on the Pulse container.

The host is ready after `trigora-pulse --check /pulse` completes a live socket handshake. The app is ready after this process finishes `register` and answers `GET /ready` on port `8080`. `terminationGracePeriodSeconds` is `30`, above the host's 10-second shutdown deadline.

The checked-in manifest does not start an execution. Add `PULSE_EXECUTION_ID` on the app container to start one, or `PULSE_RESUME_EXECUTION_ID` to resume an id you already have.

## Run

`examples/pulse-k8s/smoke.sh` is the Kind proof. It needs Docker, `kind`, and `kubectl`. It creates a cluster named `trigora-pulse` when that cluster is missing, builds `trigora-pulse:local` and `pulse-compose-app:local` when those images are missing, and loads them into the node.

```bash
./examples/pulse-k8s/smoke.sh
```

The script leaves the cluster up and stops the local authority when it exits. Both Pods in the proof use that one authority. The host is not started with `--ephemeral`.

To apply the deployment shape yourself, build the same images and create the token Secret. Both images use `imagePullPolicy: Never`. On Kind, load them with `kind load docker-image`.

```bash
kubectl create secret generic trigora-token --from-literal=token="$TRIGORA_TOKEN"
kubectl apply -f examples/pulse-k8s/pod.yaml
kubectl wait --for=condition=Ready pod/pulse-example --timeout=180s
```

A container run through `TRIGORA_TOKEN` on `https://api.trigora.dev` is a Phase 6E check. This smoke test does not do that.

## Limitations

Automatic discovery and takeover of abandoned executions are not yet implemented. A new Pod does not adopt an execution left behind by a previous one. Resume is an explicit request for an id the caller already has.

The Kind smoke passed: shared socket permissions, registration with the current host, `SIGTERM` with exit 0, and a replacement Pod that resumes a known id into the next effect without replaying the first. Kubernetes is not an advertised install.

`1000` is this example's uid, not a value the Pulse binary requires. Both containers must use the same numeric user, and that user must be able to change the socket directory to mode `0700`. The host rejects a different peer uid.

The images are local. This manifest does not set the direct authority variables. Without `TRIGORA_TOKEN`, the host exits before it binds the socket.
