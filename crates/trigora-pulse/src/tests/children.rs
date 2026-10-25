use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use tcc_core::{Engine, HostResponse};
use tcc_ir::{Artifact, ConstValue, EngineCaps, Instruction, Pc};
use tcc_state::{
    decode_continuation, encode_continuation, export_value, BranchOp, BranchPhase,
    ContinuationStatus, PendingOp, Value, WaitKind,
};

use crate::{Adapter, Authority, Commit, Reply};

use super::common::*;

#[test]
fn child_completion_on_the_memory_authority() {
    child_completion(Arc::new(Authority::new()));
}

#[test]
fn child_completion_on_the_sql_authority() {
    let server = crate::remote::serve();
    let authority = Arc::new(Authority::new().with_remote(server.client()));
    child_completion(authority);
}

fn child_completion(authority: Arc<Authority>) {
    delivers_a_string(&authority);
    delivers_null_and_a_structured_value(&authority);
    retries_initialization_without_an_effect(&authority);
    commits_the_initial_continuation_before_an_effect(&authority);
    rejects_an_uninstalled_artifact_and_a_second_hash(&authority);
    recovers_the_same_value_after_the_parent_crashes(&authority);
    a_stale_parent_fence_cannot_commit_the_result(&authority);
    a_failed_child_is_catchable(&authority);
    a_wrong_child_id_does_not_change_the_continuation(&authority);
    an_observed_cancel_beats_the_child_and_an_unobserved_one_does_not(&authority);
    a_local_cancel_beats_a_later_child(&authority);
    a_claim_that_sees_cancel_does_not_take_the_result(&authority);
    a_cancelled_child_does_not_resume_the_parent(&authority);
    three_join_children_stay_one_each(&authority);
    a_lost_parent_commit_redelivers_one_join_branch(&authority);
    a_detached_race_result_is_ignored(&authority);
}

fn delivers_a_string(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "string", None);
    let child_program = program(
        "string-child",
        0,
        vec![
            Instruction::LoadConst {
                value: ConstValue::String("hi".into()),
            },
            Instruction::Return,
        ],
    );
    finish_child(&host, &child, child_program);
    assert_eq!(
        finished(authority, &host, "string"),
        Value::String("hi".into())
    );
}

fn delivers_null_and_a_structured_value(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "null", None);
    finish_child(
        &host,
        &child,
        program(
            "null-child",
            0,
            vec![
                Instruction::LoadConst {
                    value: ConstValue::Null,
                },
                Instruction::Return,
            ],
        ),
    );
    assert_eq!(finished(authority, &host, "null"), Value::Null);

    let (host, child) = suspend_parent(authority, "object", None);
    finish_child(
        &host,
        &child,
        program(
            "object-child",
            0,
            vec![
                Instruction::NewObject,
                Instruction::LoadConst {
                    value: ConstValue::String("v".into()),
                },
                Instruction::SetProp { key: "k".into() },
                Instruction::Return,
            ],
        ),
    );
    assert_eq!(
        finished(authority, &host, "object"),
        Value::Object(BTreeMap::from([("k".into(), Value::String("v".into()))]))
    );

    let (host, child) = suspend_parent(authority, "array", None);
    finish_child(
        &host,
        &child,
        program(
            "array-child",
            0,
            vec![
                Instruction::NewArray,
                Instruction::LoadConst {
                    value: ConstValue::Number(1.0),
                },
                Instruction::ArrayPush,
                Instruction::LoadConst {
                    value: ConstValue::Number(2.0),
                },
                Instruction::ArrayPush,
                Instruction::Return,
            ],
        ),
    );
    assert_eq!(
        finished(authority, &host, "array"),
        Value::Array(vec![Value::Number(1.0), Value::Number(2.0)])
    );
}

fn retries_initialization_without_an_effect(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "init", Some(ConstValue::Number(7.0)));
    let child_program = program(
        "init-child",
        1,
        vec![
            Instruction::LoadLocal {
                local: tcc_ir::LocalId(0),
            },
            Instruction::Return,
        ],
    );
    host.runtime.install(child_program.clone()).unwrap();
    let started = authority
        .start_created(&child, "A", 60_000, "init-child", &[])
        .unwrap();
    assert!(!started.initialized);
    assert!(started.arguments.contains('7'));
    assert!(authority.view(&child).unwrap().continuation.is_none());
    let uninitialized = authority
        .claim(&child, "A", 60_000, "init-child", &[])
        .unwrap_err();
    assert_eq!(uninitialized.to_string(), "child is not initialized");
    assert!(host.runtime.dispatches().is_empty());
    drop(host);
    expire(authority, &child);
    let dir = scratch("init-again");
    let host = Host::open(
        "init-again",
        Arc::clone(authority),
        program("init-again-parent", 0, vec![Instruction::Return]),
        settings(&dir, "B"),
    );
    host.runtime.install(child_program).unwrap();
    host.runtime.start_child(&child, "init-child").unwrap();
    assert_eq!(
        finished(authority, &host, &child),
        Value::Number(7.0),
        "{:?}",
        host.runtime.failures()
    );
}

fn commits_the_initial_continuation_before_an_effect(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "effect", None);
    let child_program = program(
        "effect-child",
        0,
        vec![
            Instruction::LoadConst {
                value: ConstValue::String("work".into()),
            },
            Instruction::Effect { has_input: false },
            Instruction::Pop,
            Instruction::LoadConst {
                value: ConstValue::Number(1.0),
            },
            Instruction::Return,
        ],
    );
    host.runtime.install(child_program).unwrap();
    let release = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&release);
    let (entered_tx, entered_rx) = mpsc::channel();
    let adapter = Adapter::connect(&host.dir).unwrap();
    adapter.register("effect-child", &["work"]).unwrap();
    adapter
        .serve(move |_| {
            let _ = entered_tx.send(());
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Reply::Value(one())
        })
        .unwrap();
    wait_adapters(&host.runtime, 1);
    host.runtime.start_child(&child, "effect-child").unwrap();
    assert!(
        entered_rx.recv_timeout(Duration::from_secs(5)).is_ok(),
        "{:?}",
        host.runtime.failures()
    );
    let view = authority.view(&child).unwrap();
    assert!(view.revision >= 1);
    let decoded = decode_continuation(view.continuation.as_ref().unwrap()).unwrap();
    assert!(decoded.pending.is_none());
    assert_eq!(decoded.frames[0].pc, 0);
    release.store(true, Ordering::SeqCst);
    assert_eq!(
        finished(authority, &host, &child),
        Value::Number(1.0),
        "{:?}",
        host.runtime.failures()
    );
}

fn rejects_an_uninstalled_artifact_and_a_second_hash(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "hash", None);
    let missing = host
        .runtime
        .start_child(&child, "not-installed")
        .unwrap_err();
    assert_eq!(
        missing.to_string(),
        "artifact bytes do not match the declared hash"
    );
    assert_eq!(
        authority
            .claim(&child, "A", 60_000, "hash-child", &[])
            .unwrap_err()
            .to_string(),
        "child is not initialized"
    );
    let child_program = program(
        "hash-child",
        0,
        vec![
            Instruction::LoadConst {
                value: ConstValue::Null,
            },
            Instruction::Return,
        ],
    );
    host.runtime.install(child_program).unwrap();
    authority
        .start_created(&child, "A", 60_000, "hash-child", &[])
        .unwrap();
    let mismatch = authority
        .start_created(&child, "A", 60_000, "other-child", &[])
        .unwrap_err();
    assert_eq!(mismatch.to_string(), "artifact version mismatch");
    assert!(authority.view(&child).unwrap().continuation.is_none());
}

fn recovers_the_same_value_after_the_parent_crashes(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "crash", None);
    drop(host);
    let child_program = program(
        "crash-child",
        0,
        vec![
            Instruction::LoadConst {
                value: ConstValue::String("same".into()),
            },
            Instruction::Return,
        ],
    );
    let dir = scratch("crash-child");
    let child_host = Host::open(
        "crash-child",
        Arc::clone(authority),
        program("crash-unused", 0, vec![Instruction::Return]),
        settings(&dir, "A"),
    );
    child_host.runtime.install(child_program).unwrap();
    child_host
        .runtime
        .start_child(&child, "crash-child")
        .unwrap();
    assert_eq!(
        finished(authority, &child_host, &child),
        Value::String("same".into())
    );
    assert_eq!(
        authority.view("crash").unwrap().status,
        ContinuationStatus::Suspended
    );
    drop(child_host);
    let dir = scratch("crash-parent");
    let parent = Host::open(
        "crash-parent",
        Arc::clone(authority),
        parent_program("crash", None),
        settings(&dir, "A"),
    );
    let _adapter = serve(&parent.dir, "crash", &[]);
    wait_adapters(&parent.runtime, 1);
    parent.runtime.resume("crash").unwrap();
    assert_eq!(
        finished(authority, &parent, "crash"),
        Value::String("same".into()),
        "{:?}",
        parent.runtime.failures()
    );
}

fn a_stale_parent_fence_cannot_commit_the_result(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "fence", None);
    let view = authority.view("fence").unwrap();
    drop(host);
    let dir = scratch("fence-child");
    let child_host = Host::open(
        "fence-child",
        Arc::clone(authority),
        program("fence-unused", 0, vec![Instruction::Return]),
        settings(&dir, "A"),
    );
    child_host
        .runtime
        .install(program(
            "fence-child",
            0,
            vec![
                Instruction::LoadConst {
                    value: ConstValue::Number(3.0),
                },
                Instruction::Return,
            ],
        ))
        .unwrap();
    child_host
        .runtime
        .start_child(&child, "fence-child")
        .unwrap();
    assert_eq!(finished(authority, &child_host, &child), Value::Number(3.0));
    drop(child_host);
    let mut continuation = decode_continuation(view.continuation.as_ref().unwrap()).unwrap();
    continuation.revision = view.revision + 1;
    let bytes = encode_continuation(&continuation).unwrap();
    expire(authority, "fence");
    let rejected = authority
        .commit(&Commit {
            execution_id: "fence".into(),
            owner_token: "A".into(),
            fence: view.fence,
            expected_revision: view.revision,
            continuation: bytes,
            journal: view.journal.clone(),
            timers: None,
            waits: None,
            artifact_hash: continuation.artifact.hash.clone(),
            handler_keys: Vec::new(),
        })
        .unwrap_err();
    assert_eq!(rejected.to_string(), "stale owner");
    let dir = scratch("fence-parent");
    let parent = Host::open(
        "fence-parent",
        Arc::clone(authority),
        parent_program("fence", None),
        settings(&dir, "B"),
    );
    let _adapter = serve(&parent.dir, "fence", &[]);
    wait_adapters(&parent.runtime, 1);
    parent.runtime.resume("fence").unwrap();
    assert_eq!(
        finished(authority, &parent, "fence"),
        Value::Number(3.0),
        "{:?}",
        parent.runtime.failures()
    );
}

fn a_failed_child_is_catchable(authority: &Arc<Authority>) {
    let parent_program = program(
        "catch",
        0,
        vec![
            Instruction::PushTry {
                catch: Pc(4),
                finally: None,
            },
            Instruction::LoadConst {
                value: ConstValue::String("child".into()),
            },
            Instruction::Invoke { arg_count: 0 },
            Instruction::Return,
            Instruction::Return,
        ],
    );
    let host = Host::open(
        "catch",
        Arc::clone(authority),
        parent_program,
        settings(&scratch("catch"), "A"),
    );
    host.runtime.start("catch").unwrap();
    assert!(host.runtime.wait_until(
        |_| authority.view("catch").unwrap().status == ContinuationStatus::Suspended,
        Duration::from_secs(5)
    ));
    let child = waiting_child(authority, "catch");
    finish_child(
        &host,
        &child,
        program(
            "catch-child",
            0,
            vec![
                Instruction::LoadConst {
                    value: ConstValue::String("broke".into()),
                },
                Instruction::Throw,
            ],
        ),
    );
    assert_eq!(
        finished(authority, &host, "catch"),
        Value::String("broke".into()),
        "{:?}",
        host.runtime.failures()
    );
}

fn a_wrong_child_id_does_not_change_the_continuation(authority: &Arc<Authority>) {
    let (host, _) = suspend_parent(authority, "wrong", None);
    let view = authority.view("wrong").unwrap();
    let continuation = decode_continuation(view.continuation.as_ref().unwrap()).unwrap();
    let mut engine = Engine::resume(
        parent_program("wrong", None),
        continuation.clone(),
        &EngineCaps::current(),
    )
    .unwrap();
    let rejected = engine
        .apply_host_response(HostResponse::ChildResult {
            value: Value::Number(1.0),
            child_execution_id: "other-child".into(),
            branch: None,
        })
        .unwrap_err();
    assert!(rejected.to_string().contains("does not match"));
    assert_eq!(engine.continuation(), &continuation);
    drop(host);
}

fn an_observed_cancel_beats_the_child_and_an_unobserved_one_does_not(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "observe", None);
    drop(host);
    let dir = scratch("observe-child");
    let child_host = Host::open(
        "observe-child",
        Arc::clone(authority),
        program("observe-unused", 0, vec![Instruction::Return]),
        settings(&dir, "A"),
    );
    child_host
        .runtime
        .install(program(
            "observe-child",
            0,
            vec![
                Instruction::LoadConst {
                    value: ConstValue::Number(9.0),
                },
                Instruction::Return,
            ],
        ))
        .unwrap();
    child_host
        .runtime
        .start_child(&child, "observe-child")
        .unwrap();
    assert_eq!(finished(authority, &child_host, &child), Value::Number(9.0));
    let view = authority.view("observe").unwrap();
    let continuation = decode_continuation(view.continuation.as_ref().unwrap()).unwrap();
    authority.request_cancel("observe").unwrap();
    let mut ignored = Engine::resume(
        parent_program("observe", None),
        continuation.clone(),
        &EngineCaps::current(),
    )
    .unwrap();
    crate::supervisor::apply_child_outcome(authority, &mut ignored, false).unwrap();
    assert_ne!(ignored.continuation().status, ContinuationStatus::Cancelled);
    assert!(ignored.continuation().pending.is_none());
    let mut observed = Engine::resume(
        parent_program("observe", None),
        continuation,
        &EngineCaps::current(),
    )
    .unwrap();
    crate::supervisor::apply_child_outcome(authority, &mut observed, true).unwrap();
    assert_eq!(
        observed.continuation().status,
        ContinuationStatus::Cancelled
    );
}

fn a_local_cancel_beats_a_later_child(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "local-cancel", None);
    host.runtime.cancel("local-cancel").unwrap();
    assert!(host.runtime.wait_until(
        |_| authority.view("local-cancel").unwrap().status == ContinuationStatus::Cancelled,
        Duration::from_secs(5)
    ));
    let child_program = program(
        "local-cancel-child",
        0,
        vec![
            Instruction::LoadConst {
                value: ConstValue::Number(1.0),
            },
            Instruction::Return,
        ],
    );
    host.runtime.install(child_program).unwrap();
    host.runtime
        .start_child(&child, "local-cancel-child")
        .unwrap();
    assert_eq!(
        finished(authority, &host, &child),
        Value::Number(1.0),
        "{:?}",
        host.runtime.failures()
    );
    assert_eq!(
        authority.view("local-cancel").unwrap().status,
        ContinuationStatus::Cancelled
    );
    assert!(authority.created_child(&child).is_ok());
}

fn a_claim_that_sees_cancel_does_not_take_the_result(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "claim-cancel", None);
    drop(host);
    let dir = scratch("claim-cancel-child");
    let child_host = Host::open(
        "claim-cancel-child",
        Arc::clone(authority),
        program("claim-cancel-unused", 0, vec![Instruction::Return]),
        settings(&dir, "A"),
    );
    child_host
        .runtime
        .install(program(
            "claim-cancel-child",
            0,
            vec![
                Instruction::LoadConst {
                    value: ConstValue::Number(8.0),
                },
                Instruction::Return,
            ],
        ))
        .unwrap();
    child_host
        .runtime
        .start_child(&child, "claim-cancel-child")
        .unwrap();
    assert_eq!(finished(authority, &child_host, &child), Value::Number(8.0));
    drop(child_host);
    authority.request_cancel("claim-cancel").unwrap();
    let dir = scratch("claim-cancel-parent");
    let parent = Host::open(
        "claim-cancel-parent",
        Arc::clone(authority),
        parent_program("claim-cancel", None),
        settings(&dir, "B"),
    );
    let _adapter = serve(&parent.dir, "claim-cancel", &[]);
    wait_adapters(&parent.runtime, 1);
    expire(authority, "claim-cancel");
    parent.runtime.resume("claim-cancel").unwrap();
    assert!(parent.runtime.wait_until(
        |_| authority.view("claim-cancel").unwrap().status == ContinuationStatus::Cancelled,
        Duration::from_secs(5)
    ));
    assert!(
        parent.runtime.failures().is_empty(),
        "{:?}",
        parent.runtime.failures()
    );
    assert!(authority.created_child(&child).is_ok());
}

fn a_cancelled_child_does_not_resume_the_parent(authority: &Arc<Authority>) {
    let (host, child) = suspend_parent(authority, "child-cancel", None);
    let child_program = program(
        "child-cancel-child",
        0,
        vec![
            Instruction::LoadConst {
                value: ConstValue::Number(60_000.0),
            },
            Instruction::Sleep,
            Instruction::Pop,
            Instruction::LoadConst {
                value: ConstValue::Number(1.0),
            },
            Instruction::Return,
        ],
    );
    host.runtime.install(child_program).unwrap();
    host.runtime
        .start_child(&child, "child-cancel-child")
        .unwrap();
    assert!(host.runtime.wait_until(
        |_| authority.view(&child).unwrap().status == ContinuationStatus::Suspended,
        Duration::from_secs(5)
    ));
    host.runtime.cancel(&child).unwrap();
    assert!(host.runtime.wait_until(
        |_| authority.view(&child).unwrap().status == ContinuationStatus::Cancelled,
        Duration::from_secs(5)
    ));
    assert!(host.runtime.wait_until(
        |_| {
            authority.view("child-cancel").unwrap().status == ContinuationStatus::Suspended
                && host.runtime.failures().is_empty()
        },
        Duration::from_secs(5)
    ));
    let parent = decode_continuation(
        authority
            .view("child-cancel")
            .unwrap()
            .continuation
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    assert!(matches!(
        parent.pending,
        Some(PendingOp::Wait {
            kind: WaitKind::Child { .. },
        })
    ));
}

fn three_join_children_stay_one_each(authority: &Arc<Authority>) {
    let (host, children) = suspend_join(authority, "join-all", false);
    assert_eq!(children.len(), 3);
    for (_, child) in &children {
        assert_eq!(
            authority.created_child(child).unwrap().program_name,
            "child"
        );
    }
    assert!(authority
        .claim("join-all", "A", 60_000, "join-all", &[])
        .unwrap()
        .intent
        .is_none());
    drop(host);
    let dir = scratch("join-all-again");
    let again = Host::open(
        "join-all-again",
        Arc::clone(authority),
        join_program("join-all", 3, false),
        settings(&dir, "A"),
    );
    let _adapter = serve(&again.dir, "join-all", &[]);
    wait_adapters(&again.runtime, 1);
    again.runtime.resume("join-all").unwrap();
    assert!(
        again.runtime.wait_until(
            |_| {
                child_branches(authority, "join-all").len() == 3
                    && again.runtime.failures().is_empty()
            },
            Duration::from_secs(5)
        ),
        "{:?}",
        again.runtime.failures()
    );
    assert_eq!(child_branches(authority, "join-all"), children);
}

fn a_lost_parent_commit_redelivers_one_join_branch(authority: &Arc<Authority>) {
    let (host, children) = suspend_join(authority, "join-redo", false);
    drop(host);
    let child_id = children[0].1.clone();
    let dir = scratch("join-redo-child");
    let child_host = Host::open(
        "join-redo-child",
        Arc::clone(authority),
        program("unused-redo", 0, vec![Instruction::Return]),
        settings(&dir, "A"),
    );
    child_host
        .runtime
        .install(program(
            "redo-value",
            0,
            vec![
                Instruction::LoadConst {
                    value: ConstValue::Number(4.0),
                },
                Instruction::Return,
            ],
        ))
        .unwrap();
    child_host
        .runtime
        .start_child(&child_id, "redo-value")
        .unwrap();
    assert_eq!(
        finished(authority, &child_host, &child_id),
        Value::Number(4.0)
    );
    drop(child_host);

    let dir = scratch("join-redo-fault");
    let mut config = settings(&dir, "A");
    config.fault_after_commits = Some(0);
    let fault = Host::open(
        "join-redo-fault",
        Arc::clone(authority),
        join_program("join-redo", 3, false),
        config,
    );
    let _adapter = serve(&fault.dir, "join-redo", &[]);
    wait_adapters(&fault.runtime, 1);
    fault.runtime.resume("join-redo").unwrap();
    assert!(
        fault
            .runtime
            .wait_until(|runtime| runtime.abandoned() == 1, Duration::from_secs(5)),
        "{:?}",
        fault.runtime.failures()
    );
    assert!(
        child_branches(authority, "join-redo")
            .iter()
            .all(
                |(_, child)| branch_phase(authority, "join-redo", child) == BranchPhase::Registered
            ),
        "the rejected commit must leave every branch registered"
    );
    drop(fault);

    let dir = scratch("join-redo-parent");
    let parent = Host::open(
        "join-redo-parent",
        Arc::clone(authority),
        join_program("join-redo", 3, false),
        settings(&dir, "A"),
    );
    let _again = serve(&parent.dir, "join-redo", &[]);
    wait_adapters(&parent.runtime, 1);
    parent.runtime.resume("join-redo").unwrap();
    assert!(
        parent.runtime.wait_until(
            |_| {
                branch_phase(authority, "join-redo", &child_id) == BranchPhase::Completed
                    && child_branches(authority, "join-redo")
                        .iter()
                        .filter(|(_, child)| *child != child_id)
                        .all(|(_, child)| {
                            branch_phase(authority, "join-redo", child) == BranchPhase::Registered
                        })
            },
            Duration::from_secs(5)
        ),
        "{:?}",
        parent.runtime.failures()
    );
    for (_, child) in &children {
        assert_eq!(
            authority.created_child(child).unwrap().program_name,
            "child"
        );
    }
}

fn a_detached_race_result_is_ignored(authority: &Arc<Authority>) {
    let (host, children) = suspend_join(authority, "join-race", true);
    drop(host);
    let dir = scratch("join-race-children");
    let child_host = Host::open(
        "join-race-children",
        Arc::clone(authority),
        program("unused-race", 0, vec![Instruction::Return]),
        settings(&dir, "A"),
    );
    for (index, (_, child)) in children.iter().enumerate() {
        let hash = format!("race-{index}");
        child_host
            .runtime
            .install(program(
                &hash,
                0,
                vec![
                    Instruction::LoadConst {
                        value: ConstValue::Number((index + 1) as f64),
                    },
                    Instruction::Return,
                ],
            ))
            .unwrap();
        child_host.runtime.start_child(child, &hash).unwrap();
    }
    for (_, child) in &children {
        let _ = finished(authority, &child_host, child);
    }
    drop(child_host);
    let dir = scratch("join-race-fault");
    let mut config = settings(&dir, "A");
    config.fault_after_commits = Some(1);
    let fault = Host::open(
        "join-race-fault",
        Arc::clone(authority),
        join_program("join-race", 2, true),
        config,
    );
    let _fault_adapter = serve(&fault.dir, "join-race", &[]);
    wait_adapters(&fault.runtime, 1);
    fault.runtime.resume("join-race").unwrap();
    assert!(
        fault
            .runtime
            .wait_until(|runtime| runtime.abandoned() == 1, Duration::from_secs(5)),
        "{:?}",
        fault.runtime.failures()
    );
    assert_eq!(
        branch_phase(authority, "join-race", &children[1].1),
        BranchPhase::Detached
    );
    drop(fault);
    let dir = scratch("join-race-parent");
    let parent = Host::open(
        "join-race-parent",
        Arc::clone(authority),
        join_program("join-race", 2, true),
        settings(&dir, "A"),
    );
    let _adapter = serve(&parent.dir, "join-race", &[]);
    wait_adapters(&parent.runtime, 1);
    parent.runtime.resume("join-race").unwrap();
    assert_eq!(
        finished(authority, &parent, "join-race"),
        Value::Number(1.0),
        "{:?}",
        parent.runtime.failures()
    );
    for (_, child) in &children {
        assert_eq!(
            authority.created_child(child).unwrap().program_name,
            "child"
        );
    }
}

fn suspend_join(authority: &Arc<Authority>, id: &str, race: bool) -> (Host, Vec<(String, String)>) {
    let count = if race { 2 } else { 3 };
    let host = Host::open(
        id,
        Arc::clone(authority),
        join_program(id, count, race),
        settings(&scratch(id), "A"),
    );
    host.runtime.start(id).unwrap();
    assert!(
        host.runtime.wait_until(
            |_| {
                authority.view(id).unwrap().status == ContinuationStatus::Suspended
                    && child_branches(authority, id).len() == count as usize
            },
            Duration::from_secs(5)
        ),
        "{id} did not suspend {:?}",
        host.runtime.failures()
    );
    (host, child_branches(authority, id))
}

fn child_branches(authority: &Authority, id: &str) -> Vec<(String, String)> {
    let Some(bytes) = authority.view(id).unwrap().continuation else {
        return Vec::new();
    };
    let decoded = decode_continuation(&bytes).unwrap();
    let Some(join) = decoded.join else {
        return Vec::new();
    };
    join.branches
        .iter()
        .filter_map(|branch| match &branch.op {
            Some(BranchOp::Child {
                child_execution_id, ..
            }) => Some((branch.branch_id.clone(), child_execution_id.clone())),
            _ => None,
        })
        .collect()
}

fn branch_phase(authority: &Authority, id: &str, child: &str) -> BranchPhase {
    let bytes = authority.view(id).unwrap().continuation.unwrap();
    let decoded = decode_continuation(&bytes).unwrap();
    decoded
        .join
        .expect("join")
        .branches
        .iter()
        .find(|branch| {
            matches!(
                &branch.op,
                Some(BranchOp::Child {
                    child_execution_id, ..
                }) if child_execution_id == child
            )
        })
        .expect("branch")
        .phase
}

fn join_program(name: &str, count: u32, race: bool) -> Artifact {
    let mut instructions = vec![Instruction::Fork {
        count,
        join_pc: Pc(0),
    }];
    for _ in 0..count {
        instructions.push(Instruction::LoadConst {
            value: ConstValue::String("child".into()),
        });
        instructions.push(Instruction::Invoke { arg_count: 0 });
    }
    let join_at = instructions.len() as u32;
    instructions.push(if race {
        Instruction::JoinAny
    } else {
        Instruction::JoinAll
    });
    instructions.push(Instruction::Return);
    if let Instruction::Fork { join_pc, .. } = &mut instructions[0] {
        *join_pc = Pc(join_at);
    }
    program(name, 0, instructions)
}

fn suspend_parent(authority: &Arc<Authority>, id: &str, arg: Option<ConstValue>) -> (Host, String) {
    let host = Host::open(
        id,
        Arc::clone(authority),
        parent_program(id, arg),
        settings(&scratch(id), "A"),
    );
    host.runtime.start(id).unwrap();
    assert!(
        host.runtime.wait_until(
            |_| authority.view(id).unwrap().status == ContinuationStatus::Suspended,
            Duration::from_secs(5)
        ),
        "{id} did not suspend {:?}",
        host.runtime.failures()
    );
    let child = waiting_child(authority, id);
    (host, child)
}

fn finish_child(host: &Host, child: &str, program: Artifact) {
    let hash = program.envelope.artifact_hash.clone();
    host.runtime.install(program).unwrap();
    host.runtime.start_child(child, &hash).unwrap();
}

fn finished(authority: &Authority, host: &Host, id: &str) -> Value {
    assert!(
        host.runtime.wait_until(
            |_| authority.view(id).unwrap().status == ContinuationStatus::Completed,
            Duration::from_secs(5)
        ),
        "{id} did not complete {:?}",
        host.runtime.failures()
    );
    let bytes = authority.view(id).unwrap().continuation.unwrap();
    let decoded = decode_continuation(&bytes).unwrap();
    let raw = decoded.result.clone().unwrap_or(Value::Undefined);
    export_value(&decoded.heap, &raw).unwrap()
}

fn waiting_child(authority: &Authority, id: &str) -> String {
    let bytes = authority.view(id).unwrap().continuation.unwrap();
    let decoded = decode_continuation(&bytes).unwrap();
    match decoded.pending {
        Some(PendingOp::Wait {
            kind: WaitKind::Child {
                child_execution_id, ..
            },
        }) => child_execution_id,
        other => panic!("child wait missing: {other:?}"),
    }
}

fn parent_program(name: &str, arg: Option<ConstValue>) -> Artifact {
    let mut instructions = Vec::new();
    let arg_count = if let Some(value) = arg {
        instructions.push(Instruction::LoadConst { value });
        1
    } else {
        0
    };
    instructions.push(Instruction::LoadConst {
        value: ConstValue::String("child".into()),
    });
    instructions.push(Instruction::Invoke { arg_count });
    instructions.push(Instruction::Return);
    program(name, 0, instructions)
}

fn program(name: &str, params: u32, instructions: Vec<Instruction>) -> Artifact {
    let mut artifact = artifact(name, instructions);
    artifact.program.functions[0].param_count = params;
    artifact.program.functions[0].local_count = params;
    artifact
}

fn expire(authority: &Authority, id: &str) {
    let until = authority.view(id).unwrap().lease_until;
    authority.advance_clock(until).unwrap();
}
