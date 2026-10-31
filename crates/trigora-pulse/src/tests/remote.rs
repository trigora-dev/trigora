use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tcc_state::{encode_continuation, Continuation, ContinuationStatus, PendingOp, WaitKind};

use crate::authority::Commit;
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

#[test]
fn remote_claim_uses_the_authority_clock_for_a_timer() {
    let server = crate::remote::serve();
    let authority = Authority::new().with_remote(server.client());
    authority.advance_clock(1_000).unwrap();
    authority.open("timer", "A", 1_000).unwrap();
    let mut continuation = Continuation::start("timer", "artifact", 1, "ts.subset.v1", 0, 0);
    continuation.revision = 1;
    continuation.status = ContinuationStatus::Suspended;
    continuation.pending = Some(PendingOp::Wait {
        kind: WaitKind::Timer {
            resume_at_ms: 5_000,
        },
    });
    authority
        .commit(&Commit {
            execution_id: "timer".into(),
            owner_token: "A".into(),
            fence: 1,
            expected_revision: 0,
            continuation: encode_continuation(&continuation).unwrap(),
            journal: BTreeMap::new(),
            timers: None,
            waits: None,
            artifact_hash: "artifact".into(),
            handler_keys: Vec::new(),
        })
        .unwrap();
    authority.advance_clock(2_500).unwrap();
    let early = authority
        .claim("timer", "B", 1_000, "artifact", &[])
        .unwrap();
    assert!(!early.timer_due);
    authority.advance_clock(5_000).unwrap();
    let due = authority
        .claim("timer", "C", 1_000, "artifact", &[])
        .unwrap();
    assert!(due.timer_due);
    assert_eq!(due.revision, 1);
}
