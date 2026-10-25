use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::{Adapter, Authority, Reply};

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
