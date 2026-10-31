use std::collections::BTreeMap;
use std::env;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tcc_ir::{ConstValue, Instruction, Pc};
use tcc_state::{
    decode_continuation, encode_continuation, export_value, BranchOp, Continuation,
    ContinuationStatus, Value,
};

use crate::authority::{commit_identity, sha256, timers_of, waits_of, Authority, Commit, View};
use crate::{Adapter, Config, RemoteClient, Reply};

use super::common::*;

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn client(token: &str) -> RemoteClient {
    RemoteClient::gateway(required("TRIGORA_API_BASE_URL"), token, 16 * 1024 * 1024)
        .expect("gateway")
}

fn continuation(id: &str, revision: u64, status: ContinuationStatus, pad: usize) -> Vec<u8> {
    let mut continuation = Continuation::start(id, "artifact", 1, "ts.subset.v1", 0, 0);
    continuation.revision = revision;
    continuation.status = status;
    if pad > 0 {
        continuation.result = Some(Value::String("x".repeat(pad)));
    }
    encode_continuation(&continuation).expect("encode")
}

fn sized(id: &str, revision: u64, target: usize) -> Vec<u8> {
    let empty = continuation(id, revision, ContinuationStatus::Completed, 0).len();
    let one = continuation(id, revision, ContinuationStatus::Completed, 1).len();
    let each = one - empty;
    let mut pad = target.saturating_sub(empty) / each;
    loop {
        let bytes = continuation(id, revision, ContinuationStatus::Completed, pad);
        match bytes.len().cmp(&target) {
            std::cmp::Ordering::Equal => return bytes,
            std::cmp::Ordering::Less => pad += target - bytes.len(),
            std::cmp::Ordering::Greater => pad -= bytes.len() - target,
        }
    }
}

fn apply(
    client: &RemoteClient,
    id: &str,
    owner: &str,
    fence: u64,
    revision: u64,
    bytes: Vec<u8>,
) -> String {
    let decoded = decode_continuation(&bytes).expect("decode");
    let digest = sha256(&bytes);
    let timers = timers_of(&decoded);
    let waits = waits_of(&decoded);
    let identity = commit_identity(revision, &digest, &BTreeMap::new(), &timers, &waits);
    client
        .commit(&Commit {
            execution_id: id.to_string(),
            owner_token: owner.to_string(),
            fence,
            expected_revision: revision - 1,
            continuation: bytes,
            journal: BTreeMap::new(),
            timers: None,
            waits: None,
            artifact_hash: "artifact".to_string(),
            handler_keys: Vec::new(),
        })
        .unwrap_or_else(|err| panic!("{id} commit {revision}: {err}"));
    identity
}

#[test]
#[ignore]
fn staging_gateway_path() {
    let token = required("TRIGORA_TOKEN");
    let host = client(&token);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let id = format!("p{stamp}-proto");

    let opened = host
        .claim(&id, "A", 60_000, "artifact", &[])
        .expect("claim");
    assert_eq!(opened.fence, 1);
    let first = continuation(&id, 1, ContinuationStatus::Runnable, 0);
    let identity = apply(&host, &id, "A", opened.fence, 1, first.clone());
    let again = host
        .commit(&Commit {
            execution_id: id.clone(),
            owner_token: "A".to_string(),
            fence: opened.fence,
            expected_revision: 0,
            continuation: first.clone(),
            journal: BTreeMap::new(),
            timers: None,
            waits: None,
            artifact_hash: "artifact".to_string(),
            handler_keys: Vec::new(),
        })
        .expect("idempotent");
    assert!(again.idempotent);
    assert_eq!(
        host.connections_opened(),
        1,
        "tls connection was not reused"
    );

    let restored = client(&token)
        .claim(&id, "A", 60_000, "artifact", &[])
        .expect("restore")
        .continuation
        .expect("continuation");
    assert_eq!(restored, first);
    let mismatch = client(&token)
        .claim(&id, "A", 60_000, "other-artifact", &[])
        .unwrap_err();
    assert_eq!(mismatch.to_string(), "artifact version mismatch");

    let renewed = host.renew(&id, "A", opened.fence, 60_000).expect("renew");
    assert_eq!(renewed.fence, opened.fence);
    let stale = host.renew(&id, "A", 0, 60_000).unwrap_err();
    assert_eq!(stale.to_string(), "stale owner");
    let found = host.reconcile(&id, 1, &identity).expect("reconcile");
    assert!(found.accepted);
    let owned = host.claim(&id, "B", 60_000, "artifact", &[]).unwrap_err();
    assert_eq!(owned.to_string(), "execution is owned");

    let second = continuation(&id, 2, ContinuationStatus::Completed, 0);
    apply(&host, &id, "A", opened.fence, 2, second);
    let terminal = host
        .commit(&Commit {
            execution_id: id.clone(),
            owner_token: "A".to_string(),
            fence: opened.fence,
            expected_revision: 2,
            continuation: continuation(&id, 3, ContinuationStatus::Runnable, 0),
            journal: BTreeMap::new(),
            timers: None,
            waits: None,
            artifact_hash: "artifact".to_string(),
            handler_keys: Vec::new(),
        })
        .unwrap_err();
    assert_eq!(terminal.to_string(), "terminal state");

    let authority = Arc::new(Authority::new().with_remote(client(&token)));
    let effect_id = format!("p{stamp}-effect");
    let effect_program = artifact("stage-effect", effects(&["work"]));
    let dir = scratch("stage-effect");
    let running = Host::open(
        "stage-effect",
        Arc::clone(&authority),
        effect_program.clone(),
        settings(&dir, "A"),
    );
    let _adapter = running.start_with_adapter(&effect_id, "stage-effect", &["work"]);
    assert!(
        running.runtime.wait_until(
            |runtime| runtime.completed() == 1,
            std::time::Duration::from_secs(20),
        ),
        "effect {:?}",
        running.runtime.failures()
    );
    let committed = authority.view(&effect_id).unwrap().continuation.unwrap();
    drop(running);
    let authority = Arc::new(Authority::new().with_remote(client(&token)));
    let dir = scratch("stage-effect-restored");
    let restored_host = Host::open(
        "stage-effect-restored",
        Arc::clone(&authority),
        effect_program,
        settings(&dir, "A"),
    );
    let _adapter = serve(&restored_host.dir, "stage-effect", &["work"]);
    wait_adapters(&restored_host.runtime, 1);
    restored_host.runtime.resume(&effect_id).unwrap();
    assert!(
        restored_host.runtime.wait_until(
            |runtime| runtime.completed() == 1,
            std::time::Duration::from_secs(20),
        ),
        "restored {:?}",
        restored_host.runtime.failures()
    );
    assert!(restored_host.runtime.dispatches().is_empty());
    assert_eq!(
        authority.view(&effect_id).unwrap().continuation.unwrap(),
        committed
    );

    let authority = Arc::new(Authority::new().with_remote(client(&token)));
    let event_id = format!("p{stamp}-event");
    let event_program = artifact("stage-event", wait_once("approval"));
    let dir = scratch("stage-event");
    let waiting = Host::open(
        "stage-event",
        Arc::clone(&authority),
        event_program,
        settings(&dir, "A"),
    );
    let _adapter = serve(&waiting.dir, "stage-event", &[]);
    wait_adapters(&waiting.runtime, 1);
    waiting.runtime.start(&event_id).unwrap();
    assert!(
        waiting.runtime.wait_until(
            |runtime| runtime.suspended() == 1,
            std::time::Duration::from_secs(20),
        ),
        "event {:?}",
        waiting.runtime.failures()
    );
    let wait = authority.view(&event_id).unwrap().waits[0].wait_id.clone();
    let payload = encoded(&Value::String("approved".into()));
    waiting
        .runtime
        .send(&event_id, "approval", &wait, &payload)
        .unwrap();
    assert!(waiting.runtime.wait_until(
        |runtime| runtime.completed() == 1,
        std::time::Duration::from_secs(20),
    ));
    assert_eq!(result_string(&authority, &event_id), "approved");

    let cancel_id = format!("p{stamp}-cancel");
    let dir = scratch("stage-cancel");
    let cancelling = Host::open(
        "stage-cancel",
        Arc::clone(&authority),
        artifact("stage-cancel", wait_once("approval")),
        settings(&dir, "A"),
    );
    let _adapter = serve(&cancelling.dir, "stage-cancel", &[]);
    wait_adapters(&cancelling.runtime, 1);
    cancelling.runtime.start(&cancel_id).unwrap();
    assert!(
        cancelling.runtime.wait_until(
            |runtime| runtime.suspended() == 1,
            std::time::Duration::from_secs(20),
        ),
        "cancel wait {:?}",
        cancelling.runtime.failures()
    );
    cancelling.runtime.cancel(&cancel_id).unwrap();
    assert!(
        cancelling.runtime.wait_until(
            |runtime| runtime.cancelled() == 1,
            std::time::Duration::from_secs(20),
        ),
        "cancel {:?}",
        cancelling.runtime.failures()
    );

    let parent_id = format!("p{stamp}-parent");
    let parent = Host::open(
        "stage-parent",
        Arc::clone(&authority),
        join_program("stage-parent"),
        settings(&scratch("stage-parent"), "A"),
    );
    parent.runtime.start(&parent_id).unwrap();
    assert!(
        parent.runtime.wait_until(
            |_| authority.view(&parent_id).unwrap().status == ContinuationStatus::Suspended,
            std::time::Duration::from_secs(20),
        ),
        "parent {:?}",
        parent.runtime.failures()
    );
    let child = child_of(&authority, &parent_id);
    drop(parent);
    let child_host = Host::open(
        "stage-child",
        Arc::clone(&authority),
        program("stage-unused", vec![Instruction::Return]),
        settings(&scratch("stage-child"), "A"),
    );
    child_host
        .runtime
        .install(program(
            "stage-child",
            vec![
                Instruction::LoadConst {
                    value: ConstValue::String("same".into()),
                },
                Instruction::Return,
            ],
        ))
        .unwrap();
    child_host
        .runtime
        .start_child(&child, "stage-child")
        .unwrap();
    assert_eq!(
        finished(&authority, &child_host, &child),
        Value::String("same".into())
    );
    drop(child_host);
    let parent = Host::open(
        "stage-parent-restored",
        Arc::clone(&authority),
        join_program("stage-parent"),
        settings(&scratch("stage-parent-restored"), "A"),
    );
    let _adapter = serve(&parent.dir, "stage-parent", &[]);
    wait_adapters(&parent.runtime, 1);
    parent.runtime.resume(&parent_id).unwrap();
    assert_eq!(
        finished(&authority, &parent, &parent_id),
        Value::Array(vec![Value::String("same".into())])
    );
}

#[test]
#[ignore]
fn staging_wide_continuation() {
    let token = required("TRIGORA_TOKEN");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let wide = format!("p{stamp}-wide");
    let bytes = sized(&wide, 1, 16 * 1024 * 1024);
    let digest = sha256(&bytes);
    let wide_client = client(&token);
    apply(&wide_client, &wide, "A", 1, 1, bytes);
    let restored = client(&token)
        .claim(&wide, "A", 60_000, "artifact", &[])
        .expect("wide restore")
        .continuation
        .expect("wide continuation");
    assert_eq!(restored.len(), 16 * 1024 * 1024);
    assert_eq!(sha256(&restored), digest);
}

#[test]
#[ignore]
fn staging_workspace_boundary() {
    let token_a = required("TRIGORA_TOKEN");
    let token_b = required("TRIGORA_TOKEN_B");
    let outsider = required("TRIGORA_TOKEN_OFFLIST");
    let secret = required("TRIGORA_AUTHORITY_TOKEN");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let id = format!("p{stamp}-owned");
    let owner = client(&token_a);
    let claimed = owner
        .claim(&id, "A", 60_000, "artifact", &[])
        .expect("claim");
    let bytes = continuation(&id, 1, ContinuationStatus::Runnable, 32);
    let digest = sha256(&bytes);
    apply(&owner, &id, "A", claimed.fence, 1, bytes);
    let other = client(&token_b);
    let read = other
        .claim(&id, "B", 60_000, "artifact", &[])
        .expect("other workspace");
    assert_eq!(read.revision, 0);
    assert!(read.continuation.is_none());
    let write = other
        .commit(&Commit {
            execution_id: id.clone(),
            owner_token: "B".to_string(),
            fence: read.fence,
            expected_revision: 1,
            continuation: continuation(&id, 2, ContinuationStatus::Runnable, 0),
            journal: BTreeMap::new(),
            timers: None,
            waits: None,
            artifact_hash: "artifact".to_string(),
            handler_keys: Vec::new(),
        })
        .unwrap_err();
    assert_eq!(write.to_string(), "commit conflict");
    let kept = client(&token_a)
        .claim(&id, "A", 60_000, "artifact", &[])
        .expect("reread")
        .continuation
        .expect("continuation");
    assert_eq!(sha256(&kept), digest);

    let blocked = client(&outsider)
        .claim(&id, "A", 60_000, "artifact", &[])
        .unwrap_err();
    assert!(blocked.to_string().contains("Not Found"), "{blocked}");
    let rejected = client("not-a-workspace-token")
        .claim(&id, "A", 60_000, "artifact", &[])
        .unwrap_err();
    let bad_owner = client(&token_a)
        .claim(&id, "bad owner", 60_000, "artifact", &[])
        .unwrap_err();
    let presented = client(&secret)
        .claim(&id, "A", 60_000, "artifact", &[])
        .unwrap_err();
    for err in [&write, &blocked, &rejected, &bad_owner, &presented] {
        assert!(
            !err.to_string().contains(&secret),
            "a response contained the internal credential"
        );
    }
}

#[test]
#[ignore]
fn production_smoke() {
    let token = required("TRIGORA_TOKEN");
    let secret = required("TRIGORA_AUTHORITY_TOKEN");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let id = format!("p{stamp}-smoke");
    let host = client(&token);
    let bytes = continuation(&id, 1, ContinuationStatus::Runnable, 32);
    let digest = sha256(&bytes);
    apply(&host, &id, "A", 1, 1, bytes);
    let restored = client(&token)
        .claim(&id, "A", 60_000, "artifact", &[])
        .expect("restore")
        .continuation
        .expect("continuation");
    assert_eq!(sha256(&restored), digest);
    let presented = client(&secret)
        .claim(&id, "A", 60_000, "artifact", &[])
        .unwrap_err();
    assert!(
        !presented.to_string().contains(&secret),
        "a response contained the internal credential"
    );
    assert!(
        presented.to_string().contains("401")
            || presented
                .to_string()
                .to_ascii_lowercase()
                .contains("unauthorized")
            || presented.to_string().contains("Not Found"),
        "{presented}"
    );
}

#[test]
#[ignore]
fn staging_automatic_recovery() {
    let authority = gateway();
    let stamp = stamp();
    eprintln!("fence");
    fence_transition(&authority, stamp);
    eprintln!("replay");
    committed_effect_is_not_replayed(&authority, stamp);
    eprintln!("version");
    incompatible_hash_is_not_discovered(&authority, stamp);
    eprintln!("timer");
    timer_waits_for_the_authority_clock(&authority, stamp);
    eprintln!("event");
    accepted_event_is_delivered_after_discovery(&authority, stamp);
    eprintln!("cancel");
    cancel_is_finalized_after_discovery(&authority, stamp);
    eprintln!("child");
    child_completion_discovers_the_parent(&authority, stamp);
    eprintln!("capacity");
    capacity_and_disconnect_bound_discovery(&authority, stamp);
    eprintln!("automatic recovery passed");
}

fn gateway() -> Arc<Authority> {
    Arc::new(Authority::new().with_remote(client(&required("TRIGORA_TOKEN"))))
}

fn stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis()
}

fn leased(dir: &std::path::Path, owner: &str, lease_ms: u64) -> Config {
    let mut config = settings(dir, owner);
    config.lease_ms = lease_ms;
    config.effect_timeout = Duration::from_secs(30);
    config
}

fn expire_lease() {
    thread::sleep(Duration::from_millis(3_500));
}

fn observe(authority: &Authority, id: &str, owners: &[&str]) -> View {
    let mut last = String::from("unread");
    for owner in owners {
        match authority.read_as(id, owner) {
            Ok(view) => return view,
            Err(err) => last = err.to_string(),
        }
    }
    panic!("{id} not readable as {owners:?}: {last}");
}

fn fence_transition(authority: &Arc<Authority>, stamp: u128) {
    let id = format!("p{stamp}-fence");
    let hash = format!("p{stamp}-fence-hash");
    let program = artifact(&hash, effects(&["pin", "work"]));
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let dir = scratch("fence-a");
    let host = Host::open(
        "fence-a",
        Arc::clone(authority),
        program.clone(),
        leased(&dir, "pulse-a", 2_000),
    );
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register(&hash, &["pin", "work"]).unwrap();
    adapter
        .serve(move |request| {
            if request.key == "pin" {
                return Reply::Value(one());
            }
            while !flag.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(20));
            }
            Reply::Disconnect
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start(&id).unwrap();
    assert!(
        host.runtime.wait_until(
            |_| {
                authority.read_as(&id, "pulse-a").ok().is_some_and(|view| {
                    view.owner.as_deref() == Some("pulse-a")
                        && view.fence >= 1
                        && view.journal.contains_key("pin")
                })
            },
            Duration::from_secs(20),
        ),
        "A did not pin {id}: {:?}",
        host.runtime.failures()
    );
    let fence = observe(authority, &id, &["pulse-a"]).fence;
    let keys = vec!["pin".to_string(), "work".to_string()];
    let refused = authority
        .claim(&id, "pulse-b", 2_000, &hash, &keys)
        .unwrap_err();
    assert!(
        refused.to_string().contains("owned"),
        "B claimed a live lease: {refused}"
    );
    assert_eq!(observe(authority, &id, &["pulse-a"]).fence, fence);
    stop.store(true, Ordering::SeqCst);
    drop(adapter);
    drop(host);
    expire_lease();

    let dir = scratch("fence-b");
    let host = Host::open(
        "fence-b",
        Arc::clone(authority),
        program,
        leased(&dir, "pulse-b", 30_000),
    );
    let _adapter = serve(&dir, &hash, &["pin", "work"]);
    wait_adapters(&host.runtime, 1);
    assert!(
        host.runtime.wait_until(
            |_| {
                authority
                    .read_as(&id, "pulse-b")
                    .ok()
                    .is_some_and(|view| view.fence > fence)
            },
            Duration::from_secs(30),
        ),
        "B did not take a newer fence: {:?} pin {:?} {:?}",
        authority.read_as(&id, "pulse-b"),
        authority.pin(&id),
        host.runtime.failures()
    );
    let held = observe(authority, &id, &["pulse-b"]);
    let newer = held.fence;
    let probe = Authority::new().with_remote(client(&required("TRIGORA_TOKEN")));
    let stale = probe
        .commit(&Commit {
            execution_id: id.clone(),
            owner_token: "pulse-a".into(),
            fence,
            expected_revision: held.revision,
            continuation: continuation(&id, held.revision + 1, ContinuationStatus::Runnable, 0),
            journal: BTreeMap::new(),
            timers: None,
            waits: None,
            artifact_hash: hash,
            handler_keys: keys,
        })
        .unwrap_err();
    assert!(
        stale.to_string().contains("stale owner"),
        "A committed with fence {fence}: {stale}"
    );
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(20)),
        "B did not commit {:?}",
        host.runtime.failures()
    );
    let done = observe(authority, &id, &["pulse-b"]);
    assert!(done.revision >= 1);
    assert_eq!(done.fence, newer);
    assert_eq!(done.owner.as_deref(), Some("pulse-b"));
    assert!(done.journal.contains_key("pin"));
    assert!(done.journal.contains_key("work"));
}

fn committed_effect_is_not_replayed(authority: &Arc<Authority>, stamp: u128) {
    let id = format!("p{stamp}-replay");
    let hash = format!("p{stamp}-replay-hash");
    let program = artifact(&hash, effects(&["first", "second"]));
    let dir = scratch("replay-a");
    let host = Host::open(
        "replay-a",
        Arc::clone(authority),
        program.clone(),
        leased(&dir, "pulse-replay-a", 2_000),
    );
    let first_calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&first_calls);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register(&hash, &["first", "second"]).unwrap();
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
    host.runtime.start(&id).unwrap();
    assert!(
        host.runtime.wait_until(
            |_| {
                authority
                    .read_as(&id, "pulse-replay-a")
                    .ok()
                    .is_some_and(|view| view.journal.contains_key("first"))
            },
            Duration::from_secs(20),
        ),
        "first effect did not commit {:?}",
        host.runtime.failures()
    );
    let revision = observe(authority, &id, &["pulse-replay-a"]).revision;
    drop(adapter);
    drop(host);
    expire_lease();

    let dir = scratch("replay-b");
    let host = Host::open(
        "replay-b",
        Arc::clone(authority),
        program,
        leased(&dir, "pulse-replay-b", 30_000),
    );
    let calls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&calls);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register(&hash, &["first", "second"]).unwrap();
    adapter
        .serve(move |request| {
            seen.lock().expect("calls").push(request.key);
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(30)),
        "replacement did not finish {:?} {:?}",
        authority.read_as(&id, "pulse-replay-b"),
        host.runtime.failures()
    );
    assert_eq!(
        calls.lock().expect("calls").as_slice(),
        ["second".to_string()]
    );
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    let view = observe(authority, &id, &["pulse-replay-b"]);
    assert!(view.revision > revision);
    assert!(view.journal.contains_key("first"));
    assert!(view.journal.contains_key("second"));
    drop(adapter);
}

fn incompatible_hash_is_not_discovered(authority: &Arc<Authority>, stamp: u128) {
    let id = format!("p{stamp}-version");
    let hash = format!("p{stamp}-pinned");
    let program = artifact(&hash, effects(&["first", "second"]));
    let dir = scratch("version-a");
    let host = Host::open(
        "version-a",
        Arc::clone(authority),
        program,
        leased(&dir, "pulse-version", 2_000),
    );
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register(&hash, &["first", "second"]).unwrap();
    adapter
        .serve(|request| {
            if request.key == "first" {
                Reply::Value(one())
            } else {
                Reply::Disconnect
            }
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start(&id).unwrap();
    assert!(
        host.runtime.wait_until(
            |_| {
                authority
                    .read_as(&id, "pulse-version")
                    .ok()
                    .is_some_and(|view| view.journal.contains_key("first"))
            },
            Duration::from_secs(20),
        ),
        "pinned effect {:?}",
        host.runtime.failures()
    );
    let pinned = observe(authority, &id, &["pulse-version"]);
    drop(adapter);
    drop(host);
    expire_lease();

    let other = format!("p{stamp}-other");
    let dir = scratch("version-b");
    let host = Host::open(
        "version-b",
        Arc::clone(authority),
        artifact(&other, effects(&["first", "second"])),
        leased(&dir, "pulse-other", 30_000),
    );
    let _adapter = serve(&dir, &other, &["first", "second"]);
    wait_adapters(&host.runtime, 1);
    thread::sleep(Duration::from_secs(6));
    let view = observe(authority, &id, &["pulse-version"]);
    assert_eq!(view.fence, pinned.fence);
    assert_eq!(view.revision, pinned.revision);
    assert_eq!(authority.pin(&id).unwrap().artifact_hash, hash);
    assert!(host.runtime.dispatches().is_empty());
    assert_eq!(host.runtime.completed(), 0);
}

fn timer_waits_for_the_authority_clock(authority: &Arc<Authority>, stamp: u128) {
    let id = format!("p{stamp}-timer");
    let hash = format!("p{stamp}-timer-hash");
    let due = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis() as f64
        + 20_000.0;
    let program = artifact(
        &hash,
        vec![
            Instruction::LoadConst {
                value: ConstValue::Number(due),
            },
            Instruction::Sleep,
            Instruction::Pop,
            Instruction::LoadConst {
                value: ConstValue::String("after".into()),
            },
            Instruction::Effect { has_input: false },
            Instruction::Pop,
            Instruction::LoadConst {
                value: ConstValue::Number(1.0),
            },
            Instruction::Return,
        ],
    );
    let dir = scratch("timer-a");
    let host = Host::open(
        "timer-a",
        Arc::clone(authority),
        program.clone(),
        leased(&dir, "pulse-timer-a", 2_000),
    );
    let _adapter = serve(&dir, &hash, &["after"]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start(&id).unwrap();
    assert!(
        host.runtime.wait_until(
            |_| {
                authority
                    .read_as(&id, "pulse-timer-a")
                    .ok()
                    .is_some_and(|view| view.status == ContinuationStatus::Suspended)
            },
            Duration::from_secs(20),
        ),
        "timer did not park {:?}",
        host.runtime.failures()
    );
    drop(host);
    expire_lease();

    let dir = scratch("timer-b");
    let host = Host::open(
        "timer-b",
        Arc::clone(authority),
        program,
        leased(&dir, "pulse-timer-b", 30_000),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register(&hash, &["after"]).unwrap();
    adapter
        .serve(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    thread::sleep(Duration::from_secs(3));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        observe(authority, &id, &["pulse-timer-b", "pulse-timer-a"]).status,
        ContinuationStatus::Suspended
    );
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(30)),
        "due timer {:?}",
        host.runtime.failures()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(adapter);
}

fn accepted_event_is_delivered_after_discovery(authority: &Arc<Authority>, stamp: u128) {
    let id = format!("p{stamp}-event");
    let hash = format!("p{stamp}-event-hash");
    let program = artifact(&hash, wait_once("approval"));
    let dir = scratch("event-a");
    let host = Host::open(
        "event-a",
        Arc::clone(authority),
        program.clone(),
        leased(&dir, "pulse-event", 8_000),
    );
    let _adapter = serve(&dir, &hash, &[]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start(&id).unwrap();
    assert!(
        host.runtime.wait_until(
            |_| {
                authority
                    .read_as(&id, "pulse-event")
                    .ok()
                    .is_some_and(|view| view.status == ContinuationStatus::Suspended)
            },
            Duration::from_secs(20),
        ),
        "event wait {:?}",
        host.runtime.failures()
    );
    let view = observe(authority, &id, &["pulse-event"]);
    let wait = view.waits[0].wait_id.clone();
    let payload = encoded(&Value::String("approved".into()));
    drop(host);
    authority
        .accept_event(&id, "pulse-event", view.fence, "approval", &wait, &payload)
        .unwrap();
    expire_lease();
    let dir = scratch("event-b");
    let host = Host::open(
        "event-b",
        Arc::clone(authority),
        program,
        leased(&dir, "pulse-event-b", 30_000),
    );
    let _adapter = serve(&dir, &hash, &[]);
    wait_adapters(&host.runtime, 1);
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.completed() == 1, Duration::from_secs(30)),
        "event discovery {:?}",
        host.runtime.failures()
    );
    assert!(host.runtime.dispatches().is_empty());
    let bytes = observe(authority, &id, &["pulse-event-b"])
        .continuation
        .expect("continuation");
    let decoded = decode_continuation(&bytes).expect("decode");
    assert_eq!(decoded.result, Some(Value::String("approved".into())));
}

fn cancel_is_finalized_after_discovery(authority: &Arc<Authority>, stamp: u128) {
    let id = format!("p{stamp}-cancel");
    let hash = format!("p{stamp}-cancel-hash");
    let program = artifact(&hash, wait_once("approval"));
    let dir = scratch("cancel-a");
    let host = Host::open(
        "cancel-a",
        Arc::clone(authority),
        program.clone(),
        leased(&dir, "pulse-cancel", 2_000),
    );
    let _adapter = serve(&dir, &hash, &[]);
    wait_adapters(&host.runtime, 1);
    host.runtime.start(&id).unwrap();
    assert!(
        host.runtime.wait_until(
            |_| {
                authority
                    .read_as(&id, "pulse-cancel")
                    .ok()
                    .is_some_and(|view| view.status == ContinuationStatus::Suspended)
            },
            Duration::from_secs(20),
        ),
        "cancel wait {:?}",
        host.runtime.failures()
    );
    drop(host);
    authority.request_cancel(&id).unwrap();
    expire_lease();
    let dir = scratch("cancel-b");
    let host = Host::open(
        "cancel-b",
        Arc::clone(authority),
        program,
        leased(&dir, "pulse-cancel-b", 30_000),
    );
    let _adapter = serve(&dir, &hash, &[]);
    wait_adapters(&host.runtime, 1);
    assert!(
        host.runtime
            .wait_until(|runtime| runtime.cancelled() == 1, Duration::from_secs(30)),
        "cancel discovery {:?}",
        host.runtime.failures()
    );
    assert_eq!(
        observe(authority, &id, &["pulse-cancel-b"]).status,
        ContinuationStatus::Cancelled
    );
    assert!(host.runtime.dispatches().is_empty());
}

fn child_completion_discovers_the_parent(authority: &Arc<Authority>, stamp: u128) {
    let parent_id = format!("p{stamp}-parent");
    let parent_hash = format!("p{stamp}-parent-hash");
    let child_hash = format!("p{stamp}-child-hash");
    let parent = Host::open(
        "child-parent",
        Arc::clone(authority),
        join_program(&parent_hash),
        leased(&scratch("child-parent"), "pulse-parent", 2_000),
    );
    parent.runtime.start(&parent_id).unwrap();
    assert!(
        parent.runtime.wait_until(
            |_| {
                authority
                    .read_as(&parent_id, "pulse-parent")
                    .ok()
                    .is_some_and(|view| view.status == ContinuationStatus::Suspended)
            },
            Duration::from_secs(20),
        ),
        "parent {:?}",
        parent.runtime.failures()
    );
    let child = child_owned(authority, &parent_id, "pulse-parent");
    drop(parent);
    let child_host = Host::open(
        "child-run",
        Arc::clone(authority),
        program(&format!("{child_hash}-unused"), vec![Instruction::Return]),
        leased(&scratch("child-run"), "pulse-child", 30_000),
    );
    child_host
        .runtime
        .install(program(
            &child_hash,
            vec![
                Instruction::LoadConst {
                    value: ConstValue::String("same".into()),
                },
                Instruction::Return,
            ],
        ))
        .unwrap();
    child_host.runtime.start_child(&child, &child_hash).unwrap();
    assert_eq!(
        finished_as(authority, &child_host, &child, "pulse-child"),
        Value::String("same".into())
    );
    let child_bytes = observe(authority, &child, &["pulse-child"])
        .continuation
        .unwrap();
    drop(child_host);
    expire_lease();
    let restored = Host::open(
        "child-restore",
        Arc::clone(authority),
        join_program(&parent_hash),
        leased(&scratch("child-restore"), "pulse-parent-b", 30_000),
    );
    let _adapter = serve(&restored.dir, &parent_hash, &[]);
    wait_adapters(&restored.runtime, 1);
    assert_eq!(
        finished_as(authority, &restored, &parent_id, "pulse-parent-b"),
        Value::Array(vec![Value::String("same".into())])
    );
    let parent_bytes = observe(authority, &parent_id, &["pulse-parent-b"])
        .continuation
        .unwrap();
    assert_ne!(parent_bytes, child_bytes);
}

fn capacity_and_disconnect_bound_discovery(authority: &Arc<Authority>, stamp: u128) {
    let hash = format!("p{stamp}-cap-hash");
    let program = artifact(&hash, effects(&["pin", "work"]));
    let first = format!("p{stamp}-cap-a");
    let second = format!("p{stamp}-cap-b");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let dir = scratch("cap-a");
    let host = Host::open(
        "cap-a",
        Arc::clone(authority),
        program.clone(),
        leased(&dir, "pulse-cap-a", 2_000),
    );
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register(&hash, &["pin", "work"]).unwrap();
    adapter
        .serve(move |request| {
            if request.key == "pin" {
                return Reply::Value(one());
            }
            while !flag.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(20));
            }
            Reply::Disconnect
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start(&first).unwrap();
    host.runtime.start(&second).unwrap();
    assert!(
        host.runtime.wait_until(
            |_| {
                authority
                    .read_as(&first, "pulse-cap-a")
                    .ok()
                    .is_some_and(|view| {
                        view.owner.as_deref() == Some("pulse-cap-a")
                            && view.journal.contains_key("pin")
                    })
                    && authority
                        .read_as(&second, "pulse-cap-a")
                        .ok()
                        .is_some_and(|view| {
                            view.owner.as_deref() == Some("pulse-cap-a")
                                && view.journal.contains_key("pin")
                        })
            },
            Duration::from_secs(20),
        ),
        "capacity setup {:?}",
        host.runtime.failures()
    );
    stop.store(true, Ordering::SeqCst);
    drop(adapter);
    drop(host);
    expire_lease();

    let hold = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&hold);
    let dir = scratch("cap-b");
    let mut config = leased(&dir, "pulse-cap-b", 30_000);
    config.cap = 1;
    let host = Host::open("cap-b", Arc::clone(authority), program, config);
    let adapter = Adapter::connect(&dir).unwrap();
    adapter.register(&hash, &["pin", "work"]).unwrap();
    adapter
        .serve(move |request| {
            if request.key == "pin" {
                return Reply::Value(one());
            }
            while !flag.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(20));
            }
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    assert!(
        host.runtime.wait_until(
            |_| {
                let owned = authority.read_as(&first, "pulse-cap-b").is_ok();
                let other = authority.read_as(&second, "pulse-cap-b").is_ok();
                owned ^ other
            },
            Duration::from_secs(30),
        ),
        "cap did not admit one {:?}",
        host.runtime.failures()
    );
    let first_owner = observe(authority, &first, &["pulse-cap-b", "pulse-cap-a"]).owner;
    let second_owner = observe(authority, &second, &["pulse-cap-b", "pulse-cap-a"]).owner;
    assert!(
        (first_owner.as_deref() == Some("pulse-cap-b"))
            ^ (second_owner.as_deref() == Some("pulse-cap-b"))
    );
    drop(adapter);
    thread::sleep(Duration::from_secs(6));
    assert_eq!(
        observe(authority, &first, &[first_owner.as_deref().unwrap_or("")]).owner,
        first_owner
    );
    assert_eq!(
        observe(authority, &second, &[second_owner.as_deref().unwrap_or("")]).owner,
        second_owner
    );
    hold.store(true, Ordering::SeqCst);
}

fn child_owned(authority: &Authority, id: &str, owner: &str) -> String {
    let bytes = observe(authority, id, &[owner]).continuation.unwrap();
    child_id(&bytes)
}

fn finished_as(authority: &Authority, host: &Host, id: &str, owner: &str) -> Value {
    assert!(
        host.runtime.wait_until(
            |_| {
                authority
                    .read_as(id, owner)
                    .ok()
                    .is_some_and(|view| view.status == ContinuationStatus::Completed)
            },
            std::time::Duration::from_secs(30),
        ),
        "{id} {:?}",
        host.runtime.failures()
    );
    let bytes = observe(authority, id, &[owner]).continuation.unwrap();
    let decoded = decode_continuation(&bytes).unwrap();
    let raw = decoded.result.clone().unwrap_or(Value::Undefined);
    export_value(&decoded.heap, &raw).unwrap()
}

fn child_of(authority: &Authority, id: &str) -> String {
    let bytes = authority.view(id).unwrap().continuation.unwrap();
    child_id(&bytes)
}

fn child_id(bytes: &[u8]) -> String {
    let decoded = decode_continuation(bytes).unwrap();
    decoded
        .join
        .expect("join")
        .branches
        .into_iter()
        .find_map(|branch| match branch.op {
            Some(BranchOp::Child {
                child_execution_id, ..
            }) => Some(child_execution_id),
            _ => None,
        })
        .expect("child")
}

fn finished(authority: &Authority, host: &Host, id: &str) -> Value {
    assert!(
        host.runtime.wait_until(
            |_| authority.view(id).unwrap().status == ContinuationStatus::Completed,
            std::time::Duration::from_secs(20),
        ),
        "{id} {:?}",
        host.runtime.failures()
    );
    let bytes = authority.view(id).unwrap().continuation.unwrap();
    let decoded = decode_continuation(&bytes).unwrap();
    let raw = decoded.result.clone().unwrap_or(Value::Undefined);
    export_value(&decoded.heap, &raw).unwrap()
}

fn join_program(name: &str) -> tcc_ir::Artifact {
    let mut instructions = vec![Instruction::Fork {
        count: 1,
        join_pc: Pc(0),
    }];
    instructions.push(Instruction::LoadConst {
        value: ConstValue::String("child".into()),
    });
    instructions.push(Instruction::Invoke { arg_count: 0 });
    let join_at = instructions.len() as u32;
    instructions.push(Instruction::JoinAll);
    instructions.push(Instruction::Return);
    if let Instruction::Fork { join_pc, .. } = &mut instructions[0] {
        *join_pc = Pc(join_at);
    }
    program(name, instructions)
}

fn program(name: &str, instructions: Vec<Instruction>) -> tcc_ir::Artifact {
    artifact(name, instructions)
}
