use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{Adapter, Authority, Reply};

use super::common::*;

#[test]
fn a_compatible_worker_does_not_bypass_a_live_lease() {
    let authority = Arc::new(Authority::new());
    let v1 = artifact("v1", effects(&["work"]));
    let dir = scratch("lease-owner");
    let mut config = settings(&dir, "A");
    config.lease_ms = 60_000;
    let host = Host::open("lease-owner", Arc::clone(&authority), v1.clone(), config);
    let held_adapter = host.start_with_adapter("held", "v1", &["work"]);
    let renew_adapter = host.start_with_adapter("renew", "v1", &["work"]);
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 2, Duration::from_secs(5)));
    let held = authority.view("held").unwrap();
    let renew = authority.view("renew").unwrap();
    drop(held_adapter);
    drop(renew_adapter);
    drop(host);

    let dir = scratch("lease-foreign");
    let mut config = settings(&dir, "B");
    config.lease_ms = 60_000;
    let foreign = Host::open("lease-foreign", Arc::clone(&authority), v1.clone(), config);
    foreign.runtime.resume("held").unwrap();
    assert_eq!(foreign.runtime.waiting()[0].artifact_hash, "v1");
    assert_eq!(foreign.runtime.waiting()[0].reason, "waiting-for-version");
    assert_eq!(authority.view("held").unwrap().owner, held.owner);
    assert_eq!(authority.view("held").unwrap().fence, held.fence);
    assert_eq!(
        authority.view("held").unwrap().lease_until,
        held.lease_until
    );

    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let adapter = Adapter::connect(&foreign.dir).unwrap();
    let v1_json = tcc_ir::encode_artifact(&v1).unwrap();
    adapter
        .register_artifact("v1", &["work"], &v1_json)
        .unwrap();
    adapter
        .serve(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&foreign.runtime, 1);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(foreign.runtime.completed(), 0);
    assert!(foreign.runtime.dispatches().is_empty());
    assert!(foreign.runtime.failures().is_empty());
    assert_eq!(foreign.runtime.waiting()[0].execution_id, "held");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let blocked = authority.view("held").unwrap();
    assert_eq!(blocked.owner, held.owner);
    assert_eq!(blocked.fence, held.fence);
    assert_eq!(blocked.lease_until, held.lease_until);

    let dir = scratch("lease-holder");
    let mut config = settings(&dir, "A");
    config.lease_ms = 60_000;
    let holder = Host::open("lease-holder", Arc::clone(&authority), v1.clone(), config);
    holder.runtime.resume("renew").unwrap();
    assert_eq!(holder.runtime.waiting()[0].execution_id, "renew");
    assert_eq!(authority.view("renew").unwrap().fence, renew.fence);
    assert_eq!(
        authority.view("renew").unwrap().lease_until,
        renew.lease_until
    );
    let again = Adapter::connect(&holder.dir).unwrap();
    again.register_artifact("v1", &["work"], &v1_json).unwrap();
    again.serve(|_| Reply::Value(one())).unwrap();
    assert!(holder
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert!(holder.runtime.dispatches().is_empty());
    assert!(holder.runtime.waiting().is_empty());
    assert_eq!(authority.view("renew").unwrap().owner.as_deref(), Some("A"));
    assert_eq!(authority.view("renew").unwrap().fence, renew.fence);
    assert_eq!(authority.view("held").unwrap().fence, held.fence);
    drop(holder);

    authority.advance_clock(60_000).unwrap();
    foreign.runtime.resume("held").unwrap();
    assert!(foreign
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert!(foreign.runtime.dispatches().is_empty());
    assert!(foreign.runtime.waiting().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(authority.view("held").unwrap().owner.as_deref(), Some("B"));
    assert_eq!(authority.view("held").unwrap().fence, held.fence + 1);
}

#[test]
fn a_v2_worker_cannot_claim_a_v1_execution() {
    let authority = Arc::new(Authority::new());
    let v1 = artifact("v1", effects(&["work"]));
    let v2 = artifact("v2", effects(&["other"]));
    let dir = scratch("pin-v1");
    let mut config = settings(&dir, "A");
    config.lease_ms = 1_000;
    let host = Host::open("pin-v1", Arc::clone(&authority), v1.clone(), config);
    let _adapter = host.start_with_adapter("exec", "v1", &["work"]);
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    let before = authority.view("exec").unwrap();
    drop(host);

    authority.advance_clock(60_000).unwrap();
    let dir = scratch("pin-v2");
    let mut config = settings(&dir, "B");
    config.lease_ms = 1_000;
    let host = Host::open("pin-v2", Arc::clone(&authority), v2.clone(), config);
    let v2_calls = Arc::new(AtomicUsize::new(0));
    let hits = Arc::clone(&v2_calls);
    let v2_adapter = Adapter::connect(&host.dir).unwrap();
    let v2_json = tcc_ir::encode_artifact(&v2).unwrap();
    v2_adapter
        .register_artifact("v2", &["other"], &v2_json)
        .unwrap();
    v2_adapter
        .serve(move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("exec").unwrap();
    let waiting = host.runtime.waiting();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].execution_id, "exec");
    assert_eq!(waiting[0].artifact_hash, "v1");
    assert_eq!(waiting[0].reason, "waiting-for-version");
    assert!(waiting[0].message.contains("artifact v1"));
    assert!(waiting[0].message.contains("Deploy a worker"));
    let held = authority.view("exec").unwrap();
    assert_eq!(held.owner, before.owner);
    assert_eq!(held.fence, before.fence);
    assert_eq!(held.lease_until, before.lease_until);
    assert_eq!(v2_calls.load(Ordering::SeqCst), 0);

    let mismatch = Adapter::connect(&host.dir).unwrap();
    let err = mismatch
        .register_artifact("v1", &["work"], &v2_json)
        .unwrap_err();
    assert!(err.to_string().contains("do not match"), "{err}");
    drop(mismatch);

    let v1_json = tcc_ir::encode_artifact(&v1).unwrap();
    let wrong_keys = Adapter::connect(&host.dir).unwrap();
    wrong_keys
        .register_artifact("v1", &["other"], &v1_json)
        .unwrap();
    wrong_keys.serve(|_| Reply::Value(one())).unwrap();
    wait_adapters(&host.runtime, 2);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(authority.view("exec").unwrap().fence, before.fence);
    assert_eq!(host.runtime.waiting().len(), 1);

    let v1_calls = Arc::new(AtomicUsize::new(0));
    let hits = Arc::clone(&v1_calls);
    let v1_adapter = Adapter::connect(&host.dir).unwrap();
    v1_adapter
        .register_artifact("v1", &["work"], &v1_json)
        .unwrap();
    v1_adapter
        .serve(move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            Reply::Value(one())
        })
        .unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert!(host.runtime.dispatches().is_empty());
    assert!(host.runtime.waiting().is_empty());
    assert_eq!(v1_calls.load(Ordering::SeqCst), 0);
    assert_eq!(v2_calls.load(Ordering::SeqCst), 0);
    assert_eq!(authority.view("exec").unwrap().fence, before.fence + 1);
    assert_eq!(result_number(&authority, "exec"), 1.0);
}

#[test]
fn one_host_routes_each_execution_to_its_artifact() {
    let authority = Arc::new(Authority::new());
    let v1 = artifact("v1", effects(&["left"]));
    let v2 = artifact("v2", effects(&["right"]));
    let dir = scratch("both");
    let host = Host::open("both", authority, v1, settings(&dir, "A"));
    let left = Arc::new(Mutex::new(Vec::new()));
    let right = Arc::new(Mutex::new(Vec::new()));
    let left_seen = Arc::clone(&left);
    let v1_adapter = Adapter::connect(&dir).unwrap();
    v1_adapter.register("v1", &["left"]).unwrap();
    let right_seen = Arc::clone(&right);
    let v2_adapter = Adapter::connect(&dir).unwrap();
    let v2_json = tcc_ir::encode_artifact(&v2).unwrap();
    v2_adapter
        .register_artifact("v2", &["right"], &v2_json)
        .unwrap();
    wait_adapters(&host.runtime, 2);
    v1_adapter.start("left-exec").unwrap();
    v2_adapter.start("right-exec").unwrap();
    v1_adapter
        .serve(move |request| {
            left_seen.lock().expect("left").push(request.key);
            Reply::Value(one())
        })
        .unwrap();
    v2_adapter
        .serve(move |request| {
            right_seen.lock().expect("right").push(request.key);
            Reply::Value(one())
        })
        .unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 2, Duration::from_secs(5)));
    assert_eq!(left.lock().expect("left").as_slice(), ["left".to_string()]);
    assert_eq!(
        right.lock().expect("right").as_slice(),
        ["right".to_string()]
    );
}

#[test]
fn a_dropped_v1_worker_resumes_only_the_outstanding_effect() {
    let authority = Arc::new(Authority::new());
    let v1 = artifact("v1", effects(&["first", "second"]));
    let v2 = artifact("v2", effects(&["first", "second"]));
    let dir = scratch("drop-v1");
    let mut config = settings(&dir, "A");
    config.lease_ms = 1_000;
    let host = Host::open("drop-v1", Arc::clone(&authority), v1.clone(), config);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register("v1", &["first", "second"]).unwrap();
    adapter
        .serve(|request| {
            if request.key == "second" {
                Reply::Disconnect
            } else {
                Reply::Value(one())
            }
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start("exec").unwrap();
    assert!(host.runtime.wait_until(
        |runtime| runtime.waiting_for_adapter() == 1,
        Duration::from_secs(5)
    ));
    assert!(authority
        .view("exec")
        .unwrap()
        .journal
        .contains_key("first"));
    assert!(!authority
        .view("exec")
        .unwrap()
        .journal
        .contains_key("second"));
    assert_eq!(host.runtime.abandoned(), 0);
    let before = authority.view("exec").unwrap();
    drop(adapter);
    drop(host);

    authority.advance_clock(60_000).unwrap();
    let dir = scratch("drop-v2");
    let mut config = settings(&dir, "B");
    config.lease_ms = 1_000;
    let host = Host::open("drop-v2", Arc::clone(&authority), v2.clone(), config);
    let v2_calls = Arc::new(AtomicUsize::new(0));
    let hits = Arc::clone(&v2_calls);
    let other = Adapter::connect(&host.dir).unwrap();
    let v2_json = tcc_ir::encode_artifact(&v2).unwrap();
    other
        .register_artifact("v2", &["first", "second"], &v2_json)
        .unwrap();
    other
        .serve(move |_| {
            hits.fetch_add(1, Ordering::SeqCst);
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.resume("exec").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(host.runtime.waiting()[0].artifact_hash, "v1");
    assert_eq!(authority.view("exec").unwrap().fence, before.fence);
    assert_eq!(
        authority.view("exec").unwrap().lease_until,
        before.lease_until
    );
    assert_eq!(v2_calls.load(Ordering::SeqCst), 0);
    assert_eq!(host.runtime.completed(), 0);

    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    let again = Adapter::connect(&host.dir).unwrap();
    let v1_json = tcc_ir::encode_artifact(&v1).unwrap();
    again
        .register_artifact("v1", &["first", "second"], &v1_json)
        .unwrap();
    again
        .serve(move |request| {
            recorded.lock().expect("seen").push(request.key);
            Reply::Value(one())
        })
        .unwrap();
    assert!(host
        .runtime
        .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(5)));
    assert_eq!(
        seen.lock().expect("seen").as_slice(),
        ["second".to_string()]
    );
    assert_eq!(v2_calls.load(Ordering::SeqCst), 0);
    assert!(host.runtime.waiting().is_empty());
    assert_eq!(authority.view("exec").unwrap().fence, before.fence + 1);
}
