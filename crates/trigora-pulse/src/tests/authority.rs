use std::collections::BTreeMap;

use tcc_state::{
    decode_continuation, encode_continuation, Continuation, ContinuationStatus, PendingOp, WaitKind,
};

use crate::authority::*;

fn continuation(execution_id: &str, revision: u64, due: Option<u64>) -> Vec<u8> {
    let mut continuation = Continuation::start(execution_id, "artifact", 1, "ts.subset.v1", 0, 0);
    continuation.revision = revision;
    if let Some(due_at) = due {
        continuation.status = ContinuationStatus::Suspended;
        continuation.pending = Some(PendingOp::Wait {
            kind: WaitKind::Timer {
                resume_at_ms: due_at,
            },
        });
    } else {
        continuation.status = ContinuationStatus::Runnable;
    }
    encode_continuation(&continuation).expect("encode")
}

fn journal(key: &str, result: &str) -> BTreeMap<String, JournalRow> {
    let mut map = BTreeMap::new();
    map.insert(
        key.to_string(),
        JournalRow {
            key: key.to_string(),
            idempotency_key: format!("exec:{key}"),
            status: "completed".to_string(),
            input: "{}".to_string(),
            result: result.to_string(),
        },
    );
    map
}

#[test]
fn retry_after_claim_does_not_restore_ownership() {
    let authority = Authority::new();
    authority.open("exec", "A", 1_000).unwrap();
    let commit = Commit {
        execution_id: "exec".to_string(),
        owner_token: "A".to_string(),
        fence: 1,
        expected_revision: 0,
        continuation: continuation("exec", 1, Some(5_000)),
        journal: journal("first", "1"),
        timers: None,
        waits: None,
        artifact_hash: String::new(),
        handler_keys: Vec::new(),
    };
    let applied = authority.commit(&commit).unwrap();
    assert!(!applied.idempotent);
    assert_eq!(applied.fence, 1);
    let again = authority.commit(&commit).unwrap();
    assert!(again.idempotent);
    assert_eq!(again.fence, 1);
    assert_eq!(
        authority.view("exec").unwrap().lease_until,
        applied.lease_until
    );
    authority.advance_clock(1_000).unwrap();
    let claimed = authority
        .claim("exec", "B", 1_000, "artifact", &[])
        .unwrap();
    assert_eq!(claimed.fence, 2);
    let lease = authority.view("exec").unwrap().lease_until;
    let err = authority.commit(&commit).unwrap_err();
    assert_eq!(err.to_string(), "stale owner");
    let view = authority.view("exec").unwrap();
    assert_eq!(view.owner.as_deref(), Some("B"));
    assert_eq!(view.fence, 2);
    assert_eq!(view.revision, 1);
    assert_eq!(view.lease_until, lease);
    assert_eq!(view.journal["first"].result, "1");
    let next = Commit {
        owner_token: "A".to_string(),
        expected_revision: 1,
        continuation: continuation("exec", 2, Some(5_000)),
        ..commit.clone()
    };
    let err = authority.commit(&next).unwrap_err();
    assert_eq!(err.to_string(), "stale owner");
    assert_eq!(authority.view("exec").unwrap().revision, 1);
}

#[test]
fn same_bytes_with_different_journal_or_timers_conflict() {
    let authority = Authority::new();
    authority.open("exec", "A", 5_000).unwrap();
    let bytes = continuation("exec", 1, Some(5_000));
    let first = Commit {
        execution_id: "exec".to_string(),
        owner_token: "A".to_string(),
        fence: 1,
        expected_revision: 0,
        continuation: bytes.clone(),
        journal: journal("first", "1"),
        timers: None,
        waits: None,
        artifact_hash: String::new(),
        handler_keys: Vec::new(),
    };
    authority.commit(&first).unwrap();
    let other_journal = Commit {
        journal: journal("first", "2"),
        ..first.clone()
    };
    let err = authority.commit(&other_journal).unwrap_err();
    assert_eq!(err.to_string(), "commit conflict");
    assert_eq!(authority.view("exec").unwrap().journal["first"].result, "1");
    let other_timer = Commit {
        timers: Some(vec![TimerRow {
            branch: String::new(),
            due_at: 9,
        }]),
        ..first
    };
    let err = authority.commit(&other_timer).unwrap_err();
    assert_eq!(err.to_string(), "inconsistent timer metadata");
    assert_eq!(authority.view("exec").unwrap().timers[0].due_at, 5_000);
    assert_eq!(authority.view("exec").unwrap().revision, 1);
}

#[test]
fn claim_reports_a_due_timer_only_after_the_clock() {
    let authority = Authority::new();
    authority.open("exec", "A", 1_000).unwrap();
    authority
        .commit(&Commit {
            execution_id: "exec".to_string(),
            owner_token: "A".to_string(),
            fence: 1,
            expected_revision: 0,
            continuation: continuation("exec", 1, Some(5_000)),
            journal: BTreeMap::new(),
            timers: None,
            waits: None,
            artifact_hash: String::new(),
            handler_keys: Vec::new(),
        })
        .unwrap();
    authority.advance_clock(1_000).unwrap();
    let early = authority
        .claim("exec", "B", 1_000, "artifact", &[])
        .unwrap();
    assert!(!early.timer_due);
    authority.advance_clock(5_000).unwrap();
    let due = authority
        .claim("exec", "C", 1_000, "artifact", &[])
        .unwrap();
    assert!(due.timer_due);
    assert_eq!(due.revision, 1);
    assert_eq!(due.fence, 3);
    assert!(due.continuation.is_some());
}

#[test]
fn an_expired_owner_cannot_commit_before_anyone_else_claims() {
    let authority = Authority::new();
    authority.open("exec", "A", 1_000).unwrap();
    let commit = Commit {
        execution_id: "exec".to_string(),
        owner_token: "A".to_string(),
        fence: 1,
        expected_revision: 0,
        continuation: continuation("exec", 1, Some(5_000)),
        journal: journal("first", "1"),
        timers: None,
        waits: None,
        artifact_hash: String::new(),
        handler_keys: Vec::new(),
    };
    authority.commit(&commit).unwrap();
    let digest = sha256(&commit.continuation);
    let decoded = decode_continuation(&commit.continuation).unwrap();
    let identity = commit_identity(
        1,
        &digest,
        &commit.journal,
        &timers_of(&decoded),
        &waits_of(&decoded),
    );
    authority.advance_clock(1_000).unwrap();
    let err = authority.commit(&commit).unwrap_err();
    assert_eq!(err.to_string(), "stale owner");
    let err = authority.renew("exec", "A", 1, 1_000).unwrap_err();
    assert_eq!(err.to_string(), "stale owner");
    let found = authority.query("exec", 1, &identity).unwrap();
    assert!(found.accepted);
    assert_eq!(found.fence, 1);
    assert_eq!(found.owner.as_deref(), Some("A"));
    let view = authority.view("exec").unwrap();
    assert_eq!(view.revision, 1);
    assert_eq!(view.fence, 1);
    assert_eq!(view.journal["first"].result, "1");
    let reclaimed = authority
        .claim("exec", "A", 1_000, "artifact", &[])
        .unwrap();
    assert_eq!(reclaimed.fence, 2);
    assert_eq!(reclaimed.revision, 1);
    let err = authority.commit(&commit).unwrap_err();
    assert_eq!(err.to_string(), "stale owner");
}

#[test]
fn a_due_timer_does_not_change_the_owner() {
    let authority = Authority::new();
    authority.open("exec", "A", 5_000).unwrap();
    authority
        .commit(&Commit {
            execution_id: "exec".to_string(),
            owner_token: "A".to_string(),
            fence: 1,
            expected_revision: 0,
            continuation: continuation("exec", 1, Some(1_000)),
            journal: BTreeMap::new(),
            timers: None,
            waits: None,
            artifact_hash: String::new(),
            handler_keys: Vec::new(),
        })
        .unwrap();
    authority.advance_clock(1_000).unwrap();
    let view = authority.view("exec").unwrap();
    assert_eq!(view.owner.as_deref(), Some("A"));
    assert_eq!(view.fence, 1);
    assert_eq!(view.revision, 1);
    assert_eq!(view.timers[0].due_at, 1_000);
}

#[test]
fn a_mismatched_claim_does_not_move_the_lease() {
    let authority = Authority::new();
    authority.open("exec", "A", 1_000).unwrap();
    authority
        .commit(&Commit {
            execution_id: "exec".to_string(),
            owner_token: "A".to_string(),
            fence: 1,
            expected_revision: 0,
            continuation: continuation("exec", 1, None),
            journal: BTreeMap::new(),
            timers: None,
            waits: None,
            artifact_hash: String::new(),
            handler_keys: vec!["work".to_string()],
        })
        .unwrap();
    let before = authority.view("exec").unwrap();
    authority.advance_clock(1_000).unwrap();
    let err = authority
        .claim("exec", "B", 1_000, "other", &["work".to_string()])
        .unwrap_err();
    assert_eq!(err.to_string(), "artifact version mismatch");
    let err = authority
        .claim("exec", "B", 1_000, "artifact", &["other".to_string()])
        .unwrap_err();
    assert_eq!(err.to_string(), "artifact version mismatch");
    let after = authority.view("exec").unwrap();
    assert_eq!(after.owner, before.owner);
    assert_eq!(after.fence, before.fence);
    assert_eq!(after.lease_until, before.lease_until);
    assert_eq!(after.revision, 1);
    let claimed = authority
        .claim("exec", "B", 1_000, "artifact", &["work".to_string()])
        .unwrap();
    assert_eq!(claimed.fence, 2);
    let err = authority
        .commit(&Commit {
            execution_id: "exec".to_string(),
            owner_token: "B".to_string(),
            fence: 2,
            expected_revision: 1,
            continuation: continuation("exec", 2, None),
            journal: BTreeMap::new(),
            timers: None,
            waits: None,
            artifact_hash: String::new(),
            handler_keys: vec!["other".to_string()],
        })
        .unwrap_err();
    assert_eq!(err.to_string(), "commit conflict");
    assert_eq!(authority.view("exec").unwrap().revision, 1);
    assert_eq!(
        authority.view("exec").unwrap().handler_keys,
        vec!["work".to_string()]
    );
}
