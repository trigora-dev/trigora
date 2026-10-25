use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::{Adapter, Authority, Reply};

use super::common::*;

#[test]
fn two_adapters_for_one_hash_and_a_late_result_after_a_new_owner() {
    let authority = Arc::new(Authority::new());
    let program = artifact("shared", effects(&["work"]));
    let dir = scratch("shared");
    let host = Host::open(
        "shared",
        Arc::clone(&authority),
        program.clone(),
        settings(&dir, "A"),
    );
    let first_hits = Arc::new(AtomicUsize::new(0));
    let second_hits = Arc::new(AtomicUsize::new(0));
    let first = Adapter::connect(&dir).unwrap();
    first.register("shared", &["work"]).unwrap();
    let hits = Arc::clone(&first_hits);
    first
        .serve(move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    let second = Adapter::connect(&dir).unwrap();
    second.register("shared", &["work"]).unwrap();
    let hits = Arc::clone(&second_hits);
    second
        .serve(move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 2);
    host.runtime.start("shared").unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(first_hits.load(Ordering::SeqCst), 1);
    assert_eq!(second_hits.load(Ordering::SeqCst), 0);
    second.push_result("shared", "shared:9", &one()).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(authority.view("shared").unwrap().journal.len(), 1);
    drop(first);
    drop(second);
    drop(host);

    let dir = scratch("owner");
    let mut config = settings(&dir, "A");
    config.lease_ms = 1_000;
    config.effect_timeout = Duration::from_secs(5);
    let host = Host::open("owner", Arc::clone(&authority), program, config);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let parked = Arc::clone(&gate);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("shared", &["work"]).unwrap();
    adapter
        .serve(move |_| {
            let (lock, cv) = &*parked;
            let mut open = lock.lock().expect("gate");
            while !*open {
                open = cv.wait(open).expect("gate wait");
            }
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("owned").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.dispatches().len() == 1,
        Duration::from_secs(5)
    ));
    let attempt = host.runtime.dispatches()[0].attempt_id.clone();
    authority.advance_clock(1_000).unwrap();
    authority.claim("owned", "B", 1_000, "", &[]).unwrap();
    adapter.disconnect();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert_eq!(host.runtime.abandoned(), 0);
    *gate.0.lock().expect("gate") = true;
    gate.1.notify_all();
    let late = Adapter::connect(&dir).unwrap();
    late.register("shared", &["work"]).unwrap();
    wait_adapters(&host.runtime, 1);
    late.push_result("owned", &attempt, &one()).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let view = authority.view("owned").unwrap();
    assert!(view.journal.is_empty());
    assert_eq!(view.revision, 0);
    assert_eq!(view.owner.as_deref(), Some("B"));
    assert_eq!(host.runtime.completed(), 0);
    drop(late);
}

#[test]
fn blocked_callbacks_do_not_stall_the_other_executions() {
    let authority = Arc::new(Authority::new());
    let program = artifact("mix", effects(&["work"]));
    let dir = scratch("mix");
    let mut config = settings(&dir, "A");
    config.workers = 4;
    config.cap = 20;
    config.effect_timeout = Duration::from_secs(10);
    let host = Host::open("mix", authority, program, config);
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let gate = Arc::clone(&release);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("mix", &["work"]).unwrap();
    adapter
        .serve(move |request| {
            if request.execution_id.starts_with("block") {
                let (lock, cv) = &*gate;
                let mut open = lock.lock().expect("gate");
                while !*open {
                    open = cv.wait(open).expect("gate wait");
                }
            }
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    for index in 0..10 {
        host.runtime.start(&format!("block-{index}")).unwrap();
    }
    assert!(host.runtime.wait_until(
        |runtime| runtime.dispatches().len() == 10,
        Duration::from_secs(5)
    ));
    for index in 0..90 {
        host.runtime.start(&format!("fast-{index}")).unwrap();
    }
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 90, Duration::from_secs(15)),
        "completed {} inflight {} abandoned {}",
        host.runtime.completed(),
        host.runtime.inflight(),
        host.runtime.abandoned()
    );
    assert!(host.runtime.backlogged());
    assert!(host.runtime.max_inflight() <= 20);
    assert_eq!(host.runtime.completed(), 90);
    *release.0.lock().expect("gate") = true;
    release.1.notify_all();
    assert!(host.runtime.wait_until(
        |runtime| runtime.completed() == 100,
        Duration::from_secs(15)
    ));
    drop(adapter);
}

#[test]
fn a_held_commit_does_not_stall_another_execution_and_a_late_ack_is_dropped() {
    let server = crate::remote::serve();
    let probe = server.client();
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    let dir = scratch("held");
    let mut config = settings(&dir, "A");
    config.lease_ms = 400;
    let host = Host::open(
        "held",
        Arc::clone(&authority),
        artifact("pair", effects(&["first", "second"])),
        config,
    );
    probe.test_hook("/v1/test/hold", "slow").expect("hold");
    let _adapter = host.start_with_adapter("fast", "pair", &["first", "second"]);
    host.runtime.start("slow").expect("start slow");
    assert!(
        host.runtime.wait_until(
            |runtime| { runtime.completed() == 1 && probe.test_renewals("slow").unwrap_or(0) >= 1 },
            Duration::from_secs(8),
        ),
        "fast completed {} slow renewals {:?} abandoned {} failures {:?}",
        host.runtime.completed(),
        probe.test_renewals("slow"),
        host.runtime.abandoned(),
        host.runtime.failures()
    );
    probe
        .test_hook("/v1/test/fail-renew", "slow")
        .expect("fail renew");
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.abandoned() >= 1, Duration::from_secs(5)),
        "slow was not dropped"
    );
    probe
        .test_hook("/v1/test/release", "slow")
        .expect("release");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(host.runtime.completed(), 1);
    assert_eq!(
        keys_of(&host.runtime)
            .iter()
            .filter(|key| key.as_str() == "second")
            .count(),
        1
    );
}
