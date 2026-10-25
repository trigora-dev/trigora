use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use tcc_ir::Artifact;
use tcc_state::{decode_continuation, encode_continuation, ContinuationStatus, Value};

use crate::{Adapter, Authority, Commit, Reply};

use super::common::*;

#[test]
fn unconfirmed_progress_is_invisible_and_a_journaled_effect_is_not_redispatched() {
    let authority = Arc::new(Authority::new());
    let program = artifact("two", effects(&["first", "second"]));
    let dir = scratch("fault");
    let mut config = settings(&dir, "A");
    config.fault_after_commits = Some(0);
    let host = Host::open("fault", Arc::clone(&authority), program.clone(), config);
    let _adapter = host.start_with_adapter("exec", "two", &["first", "second"]);
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.abandoned() == 1, Duration::from_secs(5)));
    let view = authority.view("exec").unwrap();
    assert_eq!(view.revision, 0);
    assert!(view.continuation.is_none());
    assert!(view.journal.is_empty());
    drop(host);

    let dir = scratch("again");
    let host = Host::open(
        "again",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "B"),
    );
    authority.advance_clock(60_000).unwrap();
    let _adapter = serve(&host.dir, "two", &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("exec").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(
        keys_of(&host.runtime),
        vec!["first".to_string(), "second".to_string()]
    );
    drop(host);

    let dir = scratch("partial");
    let mut config = settings(&dir, "C");
    config.fault_after_commits = Some(1);
    authority.advance_clock(120_000).unwrap();
    let host = Host::open("partial", Arc::clone(&authority), program.clone(), config);
    let _adapter = serve(&host.dir, "two", &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start("partial").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.abandoned() == 1, Duration::from_secs(5)));
    let view = authority.view("partial").unwrap();
    assert_eq!(view.revision, 1);
    assert!(view.journal.contains_key("first"));
    assert!(!view.journal.contains_key("second"));
    assert!(view.continuation.is_some());
    drop(host);

    let dir = scratch("partial2");
    authority.advance_clock(180_000).unwrap();
    let host = Host::open(
        "partial2",
        Arc::clone(&authority),
        program,
        settings(&dir, "D"),
    );
    let _adapter = serve(&host.dir, "two", &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("partial").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(keys_of(&host.runtime), vec!["second".to_string()]);
    assert!(authority
        .view("partial")
        .unwrap()
        .journal
        .contains_key("first"));
    assert!(authority
        .view("partial")
        .unwrap()
        .journal
        .contains_key("second"));
}

#[test]
fn sleep_resumes_through_a_claim_after_the_simulated_clock() {
    let authority = Arc::new(Authority::new());
    let program = artifact("sleep", sleep_program());
    let dir = scratch("sleep");
    let mut config = settings(&dir, "A");
    config.lease_ms = 1_000;
    let host = Host::open("sleep", Arc::clone(&authority), program.clone(), config);
    host.runtime.start("nap").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.suspended() == 1, Duration::from_secs(5)));
    let view = authority.view("nap").unwrap();
    assert_eq!(view.timers.len(), 1);
    assert_eq!(view.timers[0].due_at, 5_000);
    assert!(host.runtime.dispatches().is_empty());
    drop(host);

    authority.advance_clock(5_000).unwrap();
    let dir = scratch("wake");
    let mut config = settings(&dir, "B");
    config.lease_ms = 1_000;
    let host = Host::open("wake", authority.clone(), program, config);
    let _adapter = serve(&host.dir, "sleep", &[]);
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("nap").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert!(host.runtime.dispatches().is_empty());
    assert_eq!(result_number(&authority, "nap"), 1.0);
    assert!(authority.view("nap").unwrap().timers.is_empty());
}

#[test]
fn create_child_is_rejected() {
    let authority = Arc::new(Authority::new());
    let program = artifact("invoke", invoke_program());
    let dir = scratch("child");
    let host = Host::open("child", authority, program, settings(&dir, "A"));
    host.runtime.start("exec").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.abandoned() == 1, Duration::from_secs(5)));
    let failures = host.runtime.failures();
    assert!(
        failures
            .iter()
            .any(|(_, message)| message.contains("create_child")),
        "{failures:?}"
    );
}

#[test]
fn disconnecting_every_adapter_leaves_the_execution_recoverable() {
    let authority = Arc::new(Authority::new());
    let program = artifact("recover", effects(&["work"]));
    let dir = scratch("recover");
    let host = Host::open(
        "recover",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("recover", &["work"]).unwrap();
    adapter.serve(|_| Reply::Disconnect).unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert_eq!(host.runtime.completed(), 0);
    assert_eq!(host.runtime.abandoned(), 0);
    assert!(authority.view("exec").unwrap().journal.is_empty());
    drop(adapter);
    drop(host);

    authority.advance_clock(60_000).unwrap();
    let dir = scratch("recover2");
    let host = Host::open(
        "recover2",
        Arc::clone(&authority),
        program,
        settings(&dir, "B"),
    );
    let _adapter = serve(&host.dir, "recover", &["work"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("exec").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(result_number(&authority, "exec"), 1.0);
}

#[test]
fn resumed_execution_reads_only_the_claimed_continuation() {
    let authority = Arc::new(Authority::new());
    let program = artifact("claim", effects(&["work"]));
    let dir = scratch("claim");
    let host = Host::open(
        "claim",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let _adapter = host.start_with_adapter("exec", "claim", &["work"]);
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    let committed = authority.view("exec").unwrap().continuation.unwrap();
    drop(host);
    authority.advance_clock(60_000).unwrap();
    let dir = scratch("claim2");
    let host = Host::open(
        "claim2",
        Arc::clone(&authority),
        program,
        settings(&dir, "B"),
    );
    let _adapter = serve(&host.dir, "claim", &["work"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("exec").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    let again = authority.view("exec").unwrap().continuation.unwrap();
    assert_eq!(committed, again);
    assert!(host.runtime.dispatches().is_empty());
}

#[test]
fn reconnecting_to_the_same_host_retries_only_the_outstanding_effect() {
    let authority = Arc::new(Authority::new());
    let program = artifact("two", effects(&["first", "second"]));
    let dir = scratch("live");
    let host = Host::open("live", Arc::clone(&authority), program, settings(&dir, "A"));
    let first_calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&first_calls);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("two", &["first", "second"]).unwrap();
    adapter
        .serve(move |request| {
            if request.key == "first" {
                seen.fetch_add(1, Ordering::SeqCst);
                Reply::Value(one())
            } else {
                Reply::Disconnect
            }
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    assert!(authority
        .view("exec")
        .unwrap()
        .journal
        .contains_key("first"));
    let stale = host.runtime.dispatches().last().unwrap().attempt_id.clone();
    drop(adapter);

    let replacement_calls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&replacement_calls);
    let replacement = Adapter::connect(&dir).unwrap();
    replacement.register("two", &["first", "second"]).unwrap();
    replacement.push_result("exec", &stale, &one()).unwrap();
    replacement
        .serve(move |request| {
            seen.lock().expect("calls").push(request.key);
            Reply::Value(one())
        })
        .unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(
        replacement_calls.lock().expect("calls").as_slice(),
        ["second".to_string()]
    );
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    let journal = authority.view("exec").unwrap().journal;
    assert!(journal.contains_key("first"));
    assert!(journal.contains_key("second"));
}

#[test]
fn a_node_handler_is_not_replayed_after_the_host_is_replaced() {
    let authority = Arc::new(Authority::new());
    let program = artifact("effect", effects(&["work"]));
    let dir = scratch("node-host");
    let host = Host::open(
        "node-host",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let peer = NodePeer::spawn(&dir, "effect", &["work"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(keys_of(&host.runtime), vec!["work".to_string()]);
    assert!(authority.view("exec").unwrap().journal.contains_key("work"));
    drop(peer);
    drop(host);

    authority.advance_clock(60_000).unwrap();
    let dir = scratch("node-restore");
    let restored = Host::open(
        "node-restore",
        Arc::clone(&authority),
        program,
        settings(&dir, "B"),
    );
    let peer = NodePeer::spawn(&dir, "effect", &["work"]);
    wait_adapters(&restored.runtime, 1);
    restored.runtime.resume("exec").unwrap();
    assert!(restored
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert!(restored.runtime.dispatches().is_empty());
    std::thread::sleep(Duration::from_millis(100));
    assert!(restored.runtime.dispatches().is_empty());
    assert_eq!(restored.runtime.completed(), 1);
    assert_eq!(result_number(&authority, "exec"), 1.0);
    drop(peer);
}

#[test]
fn durable_event_waits_on_the_memory_authority() {
    event_suite(&Arc::new(Authority::new()));
}

#[test]
fn durable_event_waits_on_the_sql_authority() {
    let server = crate::remote::serve();
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    event_suite(&authority);
}

#[test]
fn cooperative_cancellation_on_the_memory_authority() {
    cancel_suite(&Arc::new(Authority::new()));
}

#[test]
fn cooperative_cancellation_on_the_sql_authority() {
    let server = crate::remote::serve();
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    cancel_suite(&authority);
}

fn event_suite(authority: &Arc<Authority>) {
    committed_wait_survives_the_host(authority);
    accepted_payload_survives_the_host(authority);
    identical_payload_is_not_delivered_twice(authority);
    expired_owner_cannot_store_a_payload(authority);
    a_previous_wait_id_does_not_satisfy_the_next(authority);
    a_failed_consuming_commit_keeps_the_payload(authority);
    delivery_and_commit_agree_on_the_wait(authority);
}

fn committed_wait_survives_the_host(authority: &Arc<Authority>) {
    let program = artifact("wait-survives", wait_once("approval"));
    let (host, wait) = suspend(authority, "wait-survives", "survives", program.clone());
    assert!(host.runtime.dispatches().is_empty());
    drop(host);
    expire(authority, "survives");
    let (restored, _adapter) = replace(authority, "wait-survives-b", program, "B");
    restored.runtime.resume("survives").unwrap();
    assert!(
        restored
            .runtime
            .wait_until(|runtime| runtime.suspended() == 1, Duration::from_secs(5),),
        "failures {:?}",
        restored.runtime.failures()
    );
    assert!(restored.runtime.dispatches().is_empty());
    assert_eq!(authority.view("survives").unwrap().waits[0].wait_id, wait);
}

fn accepted_payload_survives_the_host(authority: &Arc<Authority>) {
    let program = artifact("wait-crash", wait_once("approval"));
    let (host, wait) = suspend(authority, "wait-crash", "crash", program.clone());
    let payload = encoded(&Value::String("approved".into()));
    let fence = authority.view("crash").unwrap().fence;
    authority
        .accept_event("crash", "A", fence, "approval", &wait, &payload)
        .unwrap();
    drop(host);
    let previous = authority.view("crash").unwrap().fence;
    expire(authority, "crash");
    let claimed = authority
        .claim("crash", "B", 60_000, "wait-crash", &[])
        .unwrap();
    assert!(claimed.fence > previous);
    assert_eq!(claimed.event.expect("payload").payload, payload);
    let still = authority.view("crash").unwrap();
    let stored = decode_continuation(still.continuation.as_ref().unwrap()).unwrap();
    assert!(stored.result.is_none());
    let again = authority
        .claim("crash", "A", 60_000, "wait-crash", &[])
        .unwrap_err();
    assert!(again.to_string().contains("owned"), "{again}");
    let (restored, _adapter) = replace(authority, "wait-crash-b", program, "B");
    restored.runtime.resume("crash").unwrap();
    assert!(
        restored
            .runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5),),
        "abandoned {} failures {:?}",
        restored.runtime.abandoned(),
        restored.runtime.failures()
    );
    assert!(restored.runtime.dispatches().is_empty());
    assert_eq!(result_string(authority, "crash"), "approved");
}

fn identical_payload_is_not_delivered_twice(authority: &Arc<Authority>) {
    let program = artifact("wait-once", wait_once("approval"));
    let (host, wait) = suspend(authority, "wait-once", "once", program);
    let payload = encoded(&Value::String("approved".into()));
    let other = encoded(&Value::String("other".into()));
    let view = authority.view("once").unwrap();
    let first = authority
        .accept_event("once", "A", view.fence, "approval", &wait, &payload)
        .unwrap();
    let second = authority
        .accept_event("once", "A", view.fence, "approval", &wait, &payload)
        .unwrap();
    assert!(!first.idempotent);
    assert!(second.idempotent);
    assert_eq!(second.revision, view.revision);
    let conflict = authority
        .accept_event("once", "A", view.fence, "approval", &wait, &other)
        .unwrap_err();
    assert!(
        conflict.to_string().contains("event conflict"),
        "{conflict}"
    );
    host.runtime
        .send("once", "approval", &wait, &payload)
        .unwrap();
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)),
        "failures {:?}",
        host.runtime.failures()
    );
    let again = host
        .runtime
        .send("once", "approval", &wait, &payload)
        .unwrap_err();
    assert!(
        again.to_string().contains("not waiting") || again.to_string().contains("not current"),
        "{again}"
    );
    assert_eq!(host.runtime.completed(), 1);
    assert_eq!(result_string(authority, "once"), "approved");
}

fn expired_owner_cannot_store_a_payload(authority: &Arc<Authority>) {
    let program = artifact("wait-lease", wait_once("approval"));
    let (host, wait) = suspend(authority, "wait-lease", "lease", program.clone());
    let payload = encoded(&Value::String("approved".into()));
    expire(authority, "lease");
    let stale = host
        .runtime
        .send("lease", "approval", &wait, &payload)
        .unwrap_err();
    assert!(stale.to_string().contains("stale owner"), "{stale}");
    drop(host);
    let (restored, _adapter) = replace(authority, "wait-lease-b", program, "B");
    restored.runtime.resume("lease").unwrap();
    assert!(
        restored
            .runtime
            .wait_until(|runtime| runtime.suspended() == 1, Duration::from_secs(5),),
        "failures {:?}",
        restored.runtime.failures()
    );
    assert_eq!(authority.view("lease").unwrap().waits[0].wait_id, wait);
    restored
        .runtime
        .send("lease", "approval", &wait, &payload)
        .unwrap();
    assert!(
        restored
            .runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5),),
        "failures {:?}",
        restored.runtime.failures()
    );
    assert_eq!(result_string(authority, "lease"), "approved");
}

fn a_previous_wait_id_does_not_satisfy_the_next(authority: &Arc<Authority>) {
    let program = artifact("wait-next", wait_twice("approval"));
    let (host, first) = suspend(authority, "wait-next", "next", program);
    let early = encoded(&Value::String("early".into()));
    let late = encoded(&Value::String("late".into()));
    host.runtime
        .send("next", "approval", &first, &early)
        .unwrap();
    assert!(
        host.runtime.wait_until(
            |_| {
                let waits = authority.view("next").unwrap().waits;
                waits.len() == 1 && waits[0].wait_id != first
            },
            Duration::from_secs(5),
        ),
        "failures {:?}",
        host.runtime.failures()
    );
    let second = authority.view("next").unwrap().waits[0].wait_id.clone();
    let missed = host
        .runtime
        .send("next", "approval", &first, &early)
        .unwrap_err();
    assert!(missed.to_string().contains("not current"), "{missed}");
    assert_eq!(authority.view("next").unwrap().waits[0].wait_id, second);
    host.runtime
        .send("next", "approval", &second, &late)
        .unwrap();
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)),
        "failures {:?}",
        host.runtime.failures()
    );
    assert_eq!(result_string(authority, "next"), "late");
}

fn a_failed_consuming_commit_keeps_the_payload(authority: &Arc<Authority>) {
    let program = artifact("wait-fault", wait_once("approval"));
    let dir = scratch("wait-fault");
    let mut config = settings(&dir, "A");
    config.fault_after_commits = Some(1);
    let host = Host::open("wait-fault", Arc::clone(authority), program.clone(), config);
    host.runtime.start("fault").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.suspended() == 1, Duration::from_secs(5)));
    let wait = authority.view("fault").unwrap().waits[0].wait_id.clone();
    let payload = encoded(&Value::String("approved".into()));
    host.runtime
        .send("fault", "approval", &wait, &payload)
        .unwrap();
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.abandoned() == 1, Duration::from_secs(5)),
        "completed {} failures {:?}",
        host.runtime.completed(),
        host.runtime.failures()
    );
    expire(authority, "fault");
    drop(host);
    let claimed = authority
        .claim("fault", "B", 60_000, "wait-fault", &[])
        .unwrap();
    assert_eq!(claimed.event.expect("payload").payload, payload);
    let (restored, _adapter) = replace(authority, "wait-fault-b", program, "B");
    restored.runtime.resume("fault").unwrap();
    assert!(
        restored
            .runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)),
        "failures {:?}",
        restored.runtime.failures()
    );
    assert!(restored.runtime.dispatches().is_empty());
    assert_eq!(result_string(authority, "fault"), "approved");
}

fn delivery_and_commit_agree_on_the_wait(authority: &Arc<Authority>) {
    let program = artifact("wait-race", wait_once("approval"));
    let (host, wait) = suspend(authority, "wait-race", "race", program.clone());
    drop(host);
    let payload = encoded(&Value::String("approved".into()));
    let view = authority.view("race").unwrap();
    authority
        .accept_event("race", "A", view.fence, "approval", &wait, &payload)
        .unwrap();
    let kept = authority.view("race").unwrap().revision;
    commit_same_wait(authority, "race", "A", view.fence).unwrap();
    let claimed = authority
        .claim("race", "A", 60_000, "wait-race", &[])
        .unwrap();
    assert_eq!(claimed.revision, kept + 1);
    assert_eq!(claimed.event.expect("kept").payload, payload);
    let stale = commit_without_wait(authority, "race", "A", 0);
    assert!(stale.unwrap_err().to_string().contains("stale owner"));
    let still = authority
        .claim("race", "A", 60_000, "wait-race", &[])
        .unwrap();
    assert_eq!(still.event.expect("still waiting").payload, payload);
    commit_without_wait(authority, "race", "A", still.fence).unwrap();
    let gone = authority
        .claim("race", "A", 60_000, "wait-race", &[])
        .unwrap();
    assert!(gone.event.is_none());
    assert!(gone.waits.is_empty());
    let late = authority
        .accept_event("race", "A", gone.fence, "approval", &wait, &payload)
        .unwrap_err();
    assert!(late.to_string().contains("not current"), "{late}");
    assert!(authority
        .claim("race", "A", 60_000, "wait-race", &[])
        .unwrap()
        .event
        .is_none());

    let (host, wait) = suspend(authority, "wait-race-2", "race-2", program);
    drop(host);
    let view = authority.view("race-2").unwrap();
    commit_without_wait(authority, "race-2", "A", view.fence).unwrap();
    let missed = authority
        .accept_event("race-2", "A", view.fence, "approval", &wait, &payload)
        .unwrap_err();
    assert!(missed.to_string().contains("not current"), "{missed}");
    overlap(authority, &payload);
}

fn overlap(authority: &Arc<Authority>, payload: &str) {
    let program = artifact("wait-overlap", wait_once("approval"));
    let (host, wait) = suspend(authority, "wait-overlap", "race-overlap", program);
    drop(host);
    let view = authority.view("race-overlap").unwrap();
    let mut continuation = decode_continuation(view.continuation.as_ref().unwrap()).unwrap();
    continuation.revision = view.revision + 1;
    continuation.status = ContinuationStatus::Completed;
    continuation.pending = None;
    continuation.result = Some(Value::String("done".into()));
    let bytes = encode_continuation(&continuation).unwrap();
    let pin = authority.pin("race-overlap").unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let authority_commit = Arc::clone(authority);
    let barrier_commit = Arc::clone(&barrier);
    let journal = view.journal.clone();
    let hash = pin.artifact_hash.clone();
    let keys = pin.handler_keys.clone();
    let revision = view.revision;
    let fence = view.fence;
    let commit = std::thread::spawn(move || {
        barrier_commit.wait();
        authority_commit.commit(&Commit {
            execution_id: "race-overlap".into(),
            owner_token: "A".into(),
            fence,
            expected_revision: revision,
            continuation: bytes,
            journal,
            timers: None,
            waits: None,
            artifact_hash: hash,
            handler_keys: keys,
        })
    });
    let authority_send = Arc::clone(authority);
    let barrier_send = Arc::clone(&barrier);
    let payload = payload.to_string();
    let send = std::thread::spawn(move || {
        barrier_send.wait();
        authority_send.accept_event("race-overlap", "A", fence, "approval", &wait, &payload)
    });
    let committed = commit.join().expect("commit thread");
    let accepted = send.join().expect("send thread");
    assert!(committed.is_ok(), "{committed:?}");
    if let Err(err) = &accepted {
        assert!(err.to_string().contains("not current"), "{err}");
    }
    let claimed = authority
        .claim(
            "race-overlap",
            "A",
            60_000,
            &pin.artifact_hash,
            &pin.handler_keys,
        )
        .unwrap();
    assert!(claimed.waits.is_empty());
    assert!(claimed.event.is_none());
}

fn cancel_suite(authority: &Arc<Authority>) {
    a_request_does_not_move_the_revision_and_a_suspended_sleep_keeps_its_owner(authority);
    a_suspended_event_wait_cancels_before_the_payload(authority);
    a_remote_request_leaves_an_active_owner_running(authority);
    an_in_flight_effect_is_journaled_and_the_next_is_not_dispatched(authority);
    a_crash_before_the_effect_checkpoint_abandons_the_result(authority);
    completion_wins_when_it_commits_before_cancel_is_observed(authority);
    cancellation_wins_when_it_commits_first(authority);
    a_failed_finalize_leaves_the_request_for_the_next_owner(authority);
    a_stale_owner_cannot_commit_cancellation_after_the_lease(authority);
    a_failed_claim_leaves_the_request(authority);
}

fn a_request_does_not_move_the_revision_and_a_suspended_sleep_keeps_its_owner(
    authority: &Arc<Authority>,
) {
    let program = artifact("cancel-sleep", sleep_program());
    let host = park(authority, "cancel-sleep", "cancel-sleep", program);
    let before = authority.view("cancel-sleep").unwrap();
    let receipt = authority.request_cancel("cancel-sleep").unwrap();
    assert!(!receipt.idempotent);
    assert_eq!(receipt.revision, before.revision);
    let parked = authority.view("cancel-sleep").unwrap();
    assert_eq!(parked.revision, before.revision);
    assert_eq!(parked.fence, before.fence);
    assert_eq!(parked.status, ContinuationStatus::Suspended);
    host.runtime.cancel("cancel-sleep").unwrap();
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.cancelled() == 1, Duration::from_secs(5),),
        "abandoned {} failures {:?}",
        host.runtime.abandoned(),
        host.runtime.failures()
    );
    let after = authority.view("cancel-sleep").unwrap();
    assert_eq!(after.status, ContinuationStatus::Cancelled);
    assert_eq!(after.fence, before.fence);
    assert!(after.revision > before.revision);
    assert!(after.timers.is_empty());
    assert!(after.waits.is_empty());
    let again = authority.request_cancel("cancel-sleep").unwrap();
    assert!(again.idempotent);
    assert_eq!(again.revision, after.revision);
    let claimed = authority
        .claim("cancel-sleep", "A", 60_000, "cancel-sleep", &[])
        .unwrap();
    assert!(!claimed.cancel_requested);
    assert_eq!(claimed.fence, before.fence);
}

fn a_suspended_event_wait_cancels_before_the_payload(authority: &Arc<Authority>) {
    let program = artifact("cancel-wait", wait_once("approval"));
    let (host, wait) = suspend(authority, "cancel-wait", "cancel-wait", program);
    let payload = encoded(&Value::String("approved".into()));
    let fence = authority.view("cancel-wait").unwrap().fence;
    authority
        .accept_event("cancel-wait", "A", fence, "approval", &wait, &payload)
        .unwrap();
    host.runtime.cancel("cancel-wait").unwrap();
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.cancelled() == 1, Duration::from_secs(5),),
        "failures {:?}",
        host.runtime.failures()
    );
    let after = authority.view("cancel-wait").unwrap();
    assert_eq!(after.status, ContinuationStatus::Cancelled);
    assert_eq!(after.fence, fence);
    assert!(after.waits.is_empty());
    assert!(after
        .continuation
        .as_ref()
        .is_some_and(|bytes| { decode_continuation(bytes).unwrap().result.is_none() }));
    let err = authority
        .accept_event("cancel-wait", "A", after.fence, "approval", &wait, &payload)
        .unwrap_err();
    assert!(err.to_string().contains("not current"), "{err}");
    let claimed = authority
        .claim("cancel-wait", "A", 60_000, "cancel-wait", &[])
        .unwrap();
    assert!(claimed.event.is_none());
    assert!(!claimed.cancel_requested);
}

fn a_remote_request_leaves_an_active_owner_running(authority: &Arc<Authority>) {
    let program = artifact("cancel-remote", effects(&["first", "second"]));
    let dir = scratch("cancel-remote");
    let host = Host::open(
        "cancel-remote",
        Arc::clone(authority),
        program,
        settings(&dir, "A"),
    );
    let gate = hold(&host.dir, "cancel-remote", &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start("cancel-remote").unwrap();
    gate.wait_entered();
    let before = authority.view("cancel-remote").unwrap();
    let receipt = authority.request_cancel("cancel-remote").unwrap();
    assert!(!receipt.idempotent);
    assert_eq!(receipt.revision, before.revision);
    assert_eq!(host.runtime.cancelled(), 0);
    assert_eq!(host.runtime.completed(), 0);
    let claimed = authority
        .claim(
            "cancel-remote",
            "A",
            60_000,
            "cancel-remote",
            &["first".into(), "second".into()],
        )
        .unwrap();
    assert!(claimed.cancel_requested);
    assert_eq!(claimed.fence, before.fence);
    assert_eq!(claimed.revision, before.revision);
    assert_eq!(host.runtime.cancelled(), 0);
    gate.release();
}

fn an_in_flight_effect_is_journaled_and_the_next_is_not_dispatched(authority: &Arc<Authority>) {
    let program = artifact("cancel-inflight", effects(&["first", "second"]));
    let dir = scratch("cancel-inflight");
    let host = Host::open(
        "cancel-inflight",
        Arc::clone(authority),
        program,
        settings(&dir, "A"),
    );
    let gate = hold(&host.dir, "cancel-inflight", &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start("cancel-inflight").unwrap();
    gate.wait_entered();
    host.runtime.cancel("cancel-inflight").unwrap();
    gate.release();
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.cancelled() == 1, Duration::from_secs(5),),
        "abandoned {} failures {:?}",
        host.runtime.abandoned(),
        host.runtime.failures()
    );
    assert_eq!(keys_of(&host.runtime), vec!["first".to_string()]);
    let view = authority.view("cancel-inflight").unwrap();
    assert_eq!(view.status, ContinuationStatus::Cancelled);
    assert!(view.journal.contains_key("first"));
    assert!(!view.journal.contains_key("second"));
}

fn a_crash_before_the_effect_checkpoint_abandons_the_result(authority: &Arc<Authority>) {
    let program = artifact("cancel-crash", effects(&["first", "second"]));
    let dir = scratch("cancel-crash");
    let host = Host::open(
        "cancel-crash",
        Arc::clone(authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let gate = hold(&host.dir, "cancel-crash", &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start("cancel-crash").unwrap();
    gate.wait_entered();
    let receipt = authority.request_cancel("cancel-crash").unwrap();
    assert!(!receipt.idempotent);
    drop(host);
    gate.release();
    expire(authority, "cancel-crash");
    let restored = Host::open(
        "cancel-crash-b",
        Arc::clone(authority),
        program,
        settings(&scratch("cancel-crash-b"), "B"),
    );
    let _adapter = serve(&restored.dir, "cancel-crash", &["first", "second"]);
    wait_adapters(&restored.runtime, 1);
    restored.runtime.resume("cancel-crash").unwrap();
    assert!(
        restored
            .runtime
            .wait_until(|runtime| runtime.cancelled() == 1, Duration::from_secs(5),),
        "abandoned {} failures {:?}",
        restored.runtime.abandoned(),
        restored.runtime.failures()
    );
    assert!(restored.runtime.dispatches().is_empty());
    let view = authority.view("cancel-crash").unwrap();
    assert_eq!(view.status, ContinuationStatus::Cancelled);
    assert!(view.journal.is_empty());
}

fn completion_wins_when_it_commits_before_cancel_is_observed(authority: &Arc<Authority>) {
    let program = artifact("cancel-complete", effects(&["first", "second"]));
    let dir = scratch("cancel-complete");
    let host = Host::open(
        "cancel-complete",
        Arc::clone(authority),
        program,
        settings(&dir, "A"),
    );
    let gate = hold(&host.dir, "cancel-complete", &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start("cancel-complete").unwrap();
    gate.wait_entered();
    authority.request_cancel("cancel-complete").unwrap();
    gate.release();
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5),),
        "cancelled {} abandoned {} failures {:?}",
        host.runtime.cancelled(),
        host.runtime.abandoned(),
        host.runtime.failures()
    );
    let view = authority.view("cancel-complete").unwrap();
    assert_eq!(view.status, ContinuationStatus::Completed);
    let claimed = authority
        .claim(
            "cancel-complete",
            "A",
            60_000,
            "cancel-complete",
            &["first".into(), "second".into()],
        )
        .unwrap();
    assert!(!claimed.cancel_requested);
    let err = revise(
        authority,
        "cancel-complete",
        "A",
        view.fence,
        |continuation| {
            continuation.status = ContinuationStatus::Cancelled;
            continuation.pending = None;
            continuation.result = None;
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("terminal state"), "{err}");
}

fn cancellation_wins_when_it_commits_first(authority: &Arc<Authority>) {
    let program = artifact("cancel-first", sleep_program());
    let host = park(authority, "cancel-first", "cancel-first", program);
    host.runtime.cancel("cancel-first").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.cancelled() == 1, Duration::from_secs(5),));
    let view = authority.view("cancel-first").unwrap();
    assert_eq!(view.status, ContinuationStatus::Cancelled);
    let err = revise(authority, "cancel-first", "A", view.fence, |continuation| {
        continuation.status = ContinuationStatus::Completed;
        continuation.pending = None;
        continuation.result = Some(Value::Number(1.0));
    })
    .unwrap_err();
    assert!(err.to_string().contains("terminal state"), "{err}");
    assert_eq!(
        authority.view("cancel-first").unwrap().status,
        ContinuationStatus::Cancelled
    );
}

fn a_failed_finalize_leaves_the_request_for_the_next_owner(authority: &Arc<Authority>) {
    let program = artifact("cancel-fault", sleep_program());
    let dir = scratch("cancel-fault");
    let mut config = settings(&dir, "A");
    config.fault_after_commits = Some(1);
    let host = Host::open(
        "cancel-fault",
        Arc::clone(authority),
        program.clone(),
        config,
    );
    host.runtime.start("cancel-fault").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.suspended() == 1, Duration::from_secs(5),));
    host.runtime.cancel("cancel-fault").unwrap();
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.abandoned() == 1, Duration::from_secs(5),),
        "cancelled {} failures {:?}",
        host.runtime.cancelled(),
        host.runtime.failures()
    );
    assert_eq!(host.runtime.cancelled(), 0);
    let parked = authority.view("cancel-fault").unwrap();
    assert_eq!(parked.status, ContinuationStatus::Suspended);
    let fence = parked.fence;
    expire(authority, "cancel-fault");
    let restored = Host::open(
        "cancel-fault-b",
        Arc::clone(authority),
        program,
        settings(&scratch("cancel-fault-b"), "B"),
    );
    let _adapter = serve(&restored.dir, "cancel-fault", &[]);
    wait_adapters(&restored.runtime, 1);
    restored.runtime.resume("cancel-fault").unwrap();
    assert!(
        restored
            .runtime
            .wait_until(|runtime| runtime.cancelled() == 1, Duration::from_secs(5),),
        "failures {:?}",
        restored.runtime.failures()
    );
    let view = authority.view("cancel-fault").unwrap();
    assert_eq!(view.status, ContinuationStatus::Cancelled);
    assert!(view.fence > fence);
    assert!(view.timers.is_empty());
    assert!(restored.runtime.dispatches().is_empty());
}

fn a_stale_owner_cannot_commit_cancellation_after_the_lease(authority: &Arc<Authority>) {
    let program = artifact("cancel-stale", sleep_program());
    let host = park(authority, "cancel-stale", "cancel-stale", program.clone());
    let before = authority.view("cancel-stale").unwrap();
    authority.request_cancel("cancel-stale").unwrap();
    drop(host);
    expire(authority, "cancel-stale");
    let err = revise(
        authority,
        "cancel-stale",
        "A",
        before.fence,
        |continuation| {
            continuation.status = ContinuationStatus::Cancelled;
            continuation.pending = None;
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("stale owner"), "{err}");
    let restored = Host::open(
        "cancel-stale-b",
        Arc::clone(authority),
        program,
        settings(&scratch("cancel-stale-b"), "B"),
    );
    let _adapter = serve(&restored.dir, "cancel-stale", &[]);
    wait_adapters(&restored.runtime, 1);
    restored.runtime.resume("cancel-stale").unwrap();
    assert!(
        restored
            .runtime
            .wait_until(|runtime| runtime.cancelled() == 1, Duration::from_secs(5),),
        "failures {:?}",
        restored.runtime.failures()
    );
    let view = authority.view("cancel-stale").unwrap();
    assert_eq!(view.status, ContinuationStatus::Cancelled);
    assert!(view.fence > before.fence);
}

fn a_failed_claim_leaves_the_request(authority: &Arc<Authority>) {
    let program = artifact("cancel-owned", sleep_program());
    let _host = park(authority, "cancel-owned", "cancel-owned", program);
    authority.request_cancel("cancel-owned").unwrap();
    let err = authority
        .claim("cancel-owned", "B", 60_000, "cancel-owned", &[])
        .unwrap_err();
    assert!(err.to_string().contains("owned"), "{err}");
    let claimed = authority
        .claim("cancel-owned", "A", 60_000, "cancel-owned", &[])
        .unwrap();
    assert!(claimed.cancel_requested);
    assert_eq!(claimed.fence, 1);
    let again = authority.request_cancel("cancel-owned").unwrap();
    assert!(again.idempotent);
    assert_eq!(again.revision, claimed.revision);
}

struct Gate {
    open: Arc<AtomicBool>,
    entered: mpsc::Receiver<()>,
    _adapter: Adapter,
}

impl Gate {
    fn wait_entered(&self) {
        self.entered
            .recv_timeout(Duration::from_secs(5))
            .expect("effect entered");
    }

    fn release(&self) {
        self.open.store(true, Ordering::SeqCst);
    }
}

fn hold(dir: &std::path::Path, hash: &str, keys: &[&str]) -> Gate {
    let adapter = Adapter::connect(dir).expect("connect");
    adapter.register(hash, keys).expect("register");
    let open = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&open);
    let (entered_tx, entered_rx) = mpsc::channel();
    adapter
        .serve(move |_| {
            let _ = entered_tx.send(());
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Reply::Value(one())
        })
        .expect("serve");
    Gate {
        open,
        entered: entered_rx,
        _adapter: adapter,
    }
}

fn park(authority: &Arc<Authority>, label: &str, id: &str, program: Artifact) -> Host {
    let dir = scratch(label);
    let host = Host::open(label, Arc::clone(authority), program, settings(&dir, "A"));
    host.runtime.start(id).unwrap();
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.suspended() == 1, Duration::from_secs(5),),
        "{label} abandoned {} failures {:?}",
        host.runtime.abandoned(),
        host.runtime.failures()
    );
    host
}

fn suspend(authority: &Arc<Authority>, label: &str, id: &str, program: Artifact) -> (Host, String) {
    let host = park(authority, label, id, program);
    let wait = authority.view(id).unwrap().waits[0].wait_id.clone();
    (host, wait)
}

fn replace(
    authority: &Arc<Authority>,
    label: &str,
    program: Artifact,
    owner: &str,
) -> (Host, Adapter) {
    let hash = program.envelope.artifact_hash.clone();
    let dir = scratch(label);
    let host = Host::open(label, Arc::clone(authority), program, settings(&dir, owner));
    let adapter = serve(&host.dir, &hash, &[]);
    wait_adapters(&host.runtime, 1);
    (host, adapter)
}

fn expire(authority: &Authority, id: &str) {
    let until = authority.view(id).unwrap().lease_until;
    authority.advance_clock(until).unwrap();
}

fn commit_same_wait(authority: &Authority, id: &str, owner: &str, fence: u64) -> crate::Result<()> {
    revise(authority, id, owner, fence, |continuation| {
        let _ = continuation;
    })
}

fn commit_without_wait(
    authority: &Authority,
    id: &str,
    owner: &str,
    fence: u64,
) -> crate::Result<()> {
    revise(authority, id, owner, fence, |continuation| {
        continuation.status = ContinuationStatus::Completed;
        continuation.pending = None;
        continuation.result = Some(Value::String("done".into()));
    })
}

fn revise(
    authority: &Authority,
    id: &str,
    owner: &str,
    fence: u64,
    change: impl FnOnce(&mut tcc_state::Continuation),
) -> crate::Result<()> {
    let view = authority.view(id)?;
    let mut continuation = decode_continuation(view.continuation.as_ref().unwrap()).unwrap();
    change(&mut continuation);
    continuation.revision = view.revision + 1;
    let bytes = encode_continuation(&continuation).unwrap();
    let pin = authority.pin(id)?;
    authority.commit(&Commit {
        execution_id: id.to_string(),
        owner_token: owner.to_string(),
        fence,
        expected_revision: view.revision,
        continuation: bytes,
        journal: view.journal,
        timers: None,
        waits: None,
        artifact_hash: pin.artifact_hash,
        handler_keys: pin.handler_keys,
    })?;
    Ok(())
}
