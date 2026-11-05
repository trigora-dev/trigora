use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tcc_state::{encode_continuation, Continuation, ContinuationStatus, PendingOp, Value, WaitKind};

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

fn serialized_continuation(id: &str, revision: u64, target: usize) -> Vec<u8> {
    fn encode(id: &str, revision: u64, pad: usize) -> Vec<u8> {
        let mut continuation = Continuation::start(id, "artifact", 1, "ts.subset.v1", 0, 0);
        continuation.revision = revision;
        continuation.status = ContinuationStatus::Completed;
        if pad > 0 {
            continuation.result = Some(Value::String("x".repeat(pad)));
        }
        encode_continuation(&continuation).expect("encode")
    }
    let empty = encode(id, revision, 0).len();
    let each = encode(id, revision, 1).len() - empty;
    let mut pad = target.saturating_sub(empty) / each.max(1);
    for _ in 0..8 {
        let bytes = encode(id, revision, pad);
        match bytes.len().cmp(&target) {
            std::cmp::Ordering::Equal => return bytes,
            std::cmp::Ordering::Less => pad += target - bytes.len(),
            std::cmp::Ordering::Greater => {
                let over = bytes.len() - target;
                pad = pad.saturating_sub(over.max(1));
            }
        }
    }
    panic!("serialized continuation did not land on {target} bytes");
}

fn boundary_commit(id: &str, fence: u64, expected: u64, continuation: Vec<u8>) -> Commit {
    Commit {
        execution_id: id.to_string(),
        owner_token: "A".to_string(),
        fence,
        expected_revision: expected,
        continuation,
        journal: BTreeMap::new(),
        timers: None,
        waits: None,
        artifact_hash: "artifact".to_string(),
        handler_keys: Vec::new(),
    }
}

#[test]
fn an_oversized_serialized_continuation_is_rejected_before_a_request() {
    let limit = crate::remote::HARD_CONTINUATION_MAX;
    let mut bytes = serialized_continuation("limit-over", 1, limit);
    bytes.push(0);
    assert_eq!(bytes.len(), limit + 1);
    let client = crate::RemoteClient::sqlite("http://127.0.0.1:1", "ws", "workspace-token", limit)
        .expect("client");
    let err = client
        .commit(&boundary_commit("limit-over", 1, 0, bytes))
        .expect_err("oversize must be rejected");
    assert!(
        err.to_string().starts_with("TRIGORA_CHECKPOINT_TOO_LARGE"),
        "{err}"
    );
    assert_eq!(client.connections_opened(), 0);
}

#[test]
fn a_serialized_continuation_at_16_mib_is_stored_and_one_extra_byte_is_not() {
    let server = crate::remote::serve();
    let client = server.client();
    let limit = crate::remote::HARD_CONTINUATION_MAX;
    let id = "limit-exact";
    let claimed = client
        .claim(id, "A", 60_000, "artifact", &[])
        .expect("claim");
    let bytes = serialized_continuation(id, 1, limit);
    assert_eq!(bytes.len(), limit);
    client
        .commit(&boundary_commit(id, claimed.fence, 0, bytes.clone()))
        .expect("exact commit");
    let stored = client.fresh().view_as(id, "A").expect("stored");
    assert_eq!(stored.revision, 1);
    assert_eq!(stored.continuation.expect("stored bytes").len(), limit);

    let mut over = bytes;
    over.push(0);
    let opened = client.connections_opened();
    let err = client
        .commit(&boundary_commit(id, claimed.fence, 1, over))
        .expect_err("one extra byte must be rejected");
    assert!(
        err.to_string().starts_with("TRIGORA_CHECKPOINT_TOO_LARGE"),
        "{err}"
    );
    assert_eq!(client.connections_opened(), opened);
    let again = client.fresh().view_as(id, "A").expect("unchanged");
    assert_eq!(again.revision, 1);
    assert_eq!(again.continuation.expect("unchanged bytes").len(), limit);
}

#[test]
fn a_refused_claim_keeps_the_previous_owner_hint() {
    let server = crate::remote::serve();
    let client = server.client();
    client
        .claim("refused-owner", "A", 60_000, "artifact", &[])
        .expect("claim");
    let err = client
        .claim("refused-owner", "B", 60_000, "artifact", &[])
        .expect_err("owned");
    assert_eq!(err.to_string(), "execution is owned");
    let view = client.view("refused-owner").expect("previous owner still reads");
    assert_eq!(view.owner.as_deref(), Some("A"));
}
