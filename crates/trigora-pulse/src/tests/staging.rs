use std::collections::BTreeMap;
use std::env;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tcc_ir::{ConstValue, Instruction, Pc};
use tcc_state::{
    decode_continuation, encode_continuation, export_value, BranchOp, Continuation,
    ContinuationStatus, Value,
};

use crate::authority::{commit_identity, sha256, timers_of, waits_of, Authority, Commit};
use crate::RemoteClient;

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

fn child_of(authority: &Authority, id: &str) -> String {
    let bytes = authority.view(id).unwrap().continuation.unwrap();
    let decoded = decode_continuation(&bytes).unwrap();
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
