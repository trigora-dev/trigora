use std::sync::Arc;
use std::time::Duration;

use crate::{Adapter, Authority, Reply};

use super::common::*;

#[test]
fn effect_commits_and_a_new_host_does_not_replay_it() {
    let authority = Arc::new(Authority::new());
    let program = artifact("effect", effects(&["work"]));
    let dir = scratch("effect");
    let host = Host::open(
        "effect",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let _adapter = host.start_with_adapter("exec", "effect", &["work"]);
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(keys_of(&host.runtime), vec!["work".to_string()]);
    assert_eq!(result_number(&authority, "exec"), 1.0);
    let journal = authority.view("exec").unwrap().journal;
    assert_eq!(journal["work"].status, "completed");
    drop(host);

    let dir = scratch("restore");
    let mut restored_config = settings(&dir, "B");
    restored_config.lease_ms = 1_000;
    authority.advance_clock(60_000).unwrap();
    let restored = Host::open("restore", Arc::clone(&authority), program, restored_config);
    let _adapter = serve(&restored.dir, "effect", &["work"]);
    wait_adapters(&restored.runtime, 1);
    restored.runtime.resume("exec").expect("resume");
    assert!(restored
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert!(restored.runtime.dispatches().is_empty());
    assert_eq!(result_number(&authority, "exec"), 1.0);
    let timings = restored.runtime.timings();
    eprintln!("warm restore {:?}", timings.restore);
    assert!(!timings.restore.is_empty());
}

#[test]
fn stale_duplicate_and_disconnected_results_do_not_journal_a_success() {
    let authority = Arc::new(Authority::new());
    let program = artifact("once", effects(&["work"]));

    let dir = scratch("stale");
    let host = Host::open(
        "stale",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("once", &["work"]).unwrap();
    adapter.serve(|_| Reply::StaleThen(one())).unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("stale").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(authority.view("stale").unwrap().journal.len(), 1);
    drop(adapter);
    drop(host);

    let dir = scratch("dup");
    let host = Host::open(
        "dup",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("once", &["work"]).unwrap();
    adapter.serve(|_| Reply::Duplicate(one())).unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("dup").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(authority.view("dup").unwrap().journal.len(), 1);
    drop(adapter);
    drop(host);

    let dir = scratch("drop");
    let host = Host::open("drop", Arc::clone(&authority), program, settings(&dir, "A"));
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("once", &["work"]).unwrap();
    adapter.serve(|_| Reply::Disconnect).unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("drop").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert_eq!(host.runtime.completed(), 0);
    assert_eq!(host.runtime.abandoned(), 0);
    let view = authority.view("drop").unwrap();
    assert!(view.journal.is_empty());
    assert!(view.continuation.is_none());
    assert_eq!(view.revision, 0);
}

#[test]
fn timeout_retries_with_a_new_attempt_and_ignores_the_late_result() {
    let authority = Arc::new(Authority::new());
    let program = artifact("slow", effects(&["work"]));
    let dir = scratch("timeout");
    let mut config = settings(&dir, "A");
    config.effect_timeout = Duration::from_millis(50);
    config.max_effect_attempts = 2;
    let host = Host::open("timeout", authority.clone(), program, config);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("slow", &["work"]).unwrap();
    adapter
        .serve(|request| {
            if request.attempt_id.ends_with(":1") {
                Reply::Delay(Duration::from_millis(200), one())
            } else {
                Reply::Value(one())
            }
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("slow").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    let attempts: Vec<_> = host
        .runtime
        .dispatches()
        .into_iter()
        .map(|dispatch| dispatch.attempt_id)
        .collect();
    assert_eq!(attempts, vec!["slow:1".to_string(), "slow:2".to_string()]);
    assert_eq!(authority.view("slow").unwrap().journal.len(), 1);
    assert_eq!(
        authority.view("slow").unwrap().journal["work"].status,
        "completed"
    );
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(authority.view("slow").unwrap().journal.len(), 1);
    drop(adapter);
}

#[test]
fn payload_and_commit_timings_are_recorded() {
    let authority = Arc::new(Authority::new());
    for bytes in [4 * 1024, 64 * 1024, 1024 * 1024] {
        let name = format!("echo-{bytes}");
        let program = artifact(&name, echo(bytes));
        let dir = scratch("echo");
        let host = Host::open("echo", Arc::clone(&authority), program, settings(&dir, "A"));
        let adapter = Adapter::connect(&dir).unwrap();
        adapter.register(&name, &["echo"]).unwrap();
        adapter
            .serve(|request| Reply::Value(request.input))
            .unwrap();
        wait_adapters(&host.runtime, 1);
        let id = format!("echo-{bytes}");
        host.runtime.start(&id).unwrap();
        assert!(
            host.runtime
                .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(20)),
            "{bytes} abandoned {} failures {:?}",
            host.runtime.abandoned(),
            host.runtime.failures()
        );
        let timings = host.runtime.timings();
        eprintln!(
            "payload {bytes} socket {:?} commit {:?}",
            timings.socket, timings.commit
        );
        assert!(!timings.socket.is_empty());
        assert!(!timings.commit.is_empty());
        assert_eq!(result_number(&authority, &id), 1.0);
        drop(adapter);
    }
}

#[test]
fn started_accepts_a_fresh_id_and_rejects_a_reused_one() {
    let authority = Arc::new(Authority::new());
    let program = artifact("once", effects(&["work"]));
    let dir = scratch("start");
    let host = Host::open("start", authority, program, settings(&dir, "A"));
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("once", &["work"]).unwrap();
    adapter.start("exec").unwrap();
    let err = host.runtime.start("exec").unwrap_err();
    assert_eq!(err.to_string(), "execution id is already in use");
    adapter.serve(|_| Reply::Value(one())).unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    let err = host.runtime.start("exec").unwrap_err();
    assert_eq!(err.to_string(), "execution id is already in use");
}

#[test]
fn a_missing_handler_key_is_not_dispatched() {
    let authority = Arc::new(Authority::new());
    let program = artifact("once", effects(&["work"]));
    let dir = scratch("key");
    let host = Host::open("key", authority, program, settings(&dir, "A"));
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("once", &["other"]).unwrap();
    adapter.serve(|_| Reply::Value(one())).unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert!(host.runtime.dispatches().is_empty());
    assert_eq!(host.runtime.completed(), 0);
    assert!(host
        .runtime
        .failures()
        .iter()
        .any(|(_, message)| message.contains("no adapter registered for effect `work`")));
}

#[test]
fn registering_an_adapter_does_not_resume_sleep() {
    let authority = Arc::new(Authority::new());
    let program = artifact("sleep", sleep_program());
    let dir = scratch("sleep-reg");
    let host = Host::open("sleep-reg", authority, program, settings(&dir, "A"));
    host.runtime.start("nap").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.suspended() == 1, Duration::from_secs(5)));
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("sleep", &["work"]).unwrap();
    wait_adapters(&host.runtime, 1);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(host.runtime.suspended(), 1);
    assert_eq!(host.runtime.completed(), 0);
    assert_eq!(host.runtime.waiting_for_adapter(), 0);
    drop(adapter);
}
