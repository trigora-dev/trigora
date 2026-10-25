use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{Adapter, Authority, Reply};

use super::common::*;

#[test]
fn sqlite_commit_survives_a_fresh_host_and_is_not_replayed() {
    let server = crate::remote::serve();
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    let program = artifact("sqlite-remote", effects(&["work"]));
    let dir = scratch("sqlite-remote");
    let host = Host::open(
        "sqlite-remote",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let _adapter = host.start_with_adapter("exec", "sqlite-remote", &["work"]);
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(15)),
        "abandoned {} failures {:?}",
        host.runtime.abandoned(),
        host.runtime.failures()
    );
    let committed = authority
        .view("exec")
        .expect("view")
        .continuation
        .expect("continuation");
    drop(host);
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    let dir = scratch("sqlite-remote-restored");
    let host = Host::open(
        "sqlite-remote-restored",
        Arc::clone(&authority),
        program,
        settings(&dir, "A"),
    );
    let _adapter = serve(&host.dir, "sqlite-remote", &["work"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("exec").expect("resume");
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(15)),
        "restored abandoned {} failures {:?}",
        host.runtime.abandoned(),
        host.runtime.failures()
    );
    assert!(host.runtime.dispatches().is_empty());
    let again = authority
        .view("exec")
        .expect("view")
        .continuation
        .expect("continuation");
    assert_eq!(committed, again);
}

#[test]
fn a_late_callback_does_not_advance_after_ownership_is_lost() {
    let server = crate::remote::serve();
    let probe = server.client();
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    let dir = scratch("late-callback");
    let mut config = settings(&dir, "A");
    config.lease_ms = 400;
    let host = Host::open(
        "late-callback",
        Arc::clone(&authority),
        artifact("late", effects(&["work"])),
        config,
    );
    let (gate_tx, gate_rx) = std::sync::mpsc::channel();
    let gate_rx = Arc::new(Mutex::new(gate_rx));
    let adapter = Adapter::connect(&dir).expect("connect");
    adapter.register("late", &["work"]).expect("register");
    let gate = Arc::clone(&gate_rx);
    adapter
        .serve(move |_| {
            let _ = gate.lock().expect("gate").recv();
            Reply::Value(one())
        })
        .expect("serve");
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").expect("start");
    assert!(
        host.runtime.wait_until(
            |_| probe.test_renewals("exec").unwrap_or(0) >= 1,
            Duration::from_secs(5),
        ),
        "renewal did not run"
    );
    probe
        .test_hook("/v1/test/fail-renew", "exec")
        .expect("fail renew");
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.abandoned() >= 1, Duration::from_secs(5)),
        "ownership loss did not stop the execution"
    );
    let _ = gate_tx.send(());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(host.runtime.completed(), 0);
    let view = authority.view("exec").expect("view");
    assert_eq!(view.revision, 0);
    assert!(view.journal.is_empty());
}
