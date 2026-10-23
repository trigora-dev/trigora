use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use tcc_state::{decode_continuation, Continuation, ContinuationStatus, PendingOp, WaitKind};

use crate::error::{Error, Result};

/// One recorded effect outcome. Part of the commit identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct JournalRow {
    pub key: String,
    pub idempotency_key: String,
    pub status: String,
    pub input: String,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimerRow {
    pub branch: String,
    pub due_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WaitRow {
    pub wait_id: String,
    pub event: String,
}

/// Full authoritative transaction. `timers` or `waits` set to `Some` must match
/// the continuation; `None` means derive them from it.
#[derive(Debug, Clone)]
pub struct Commit {
    pub execution_id: String,
    pub owner_token: String,
    pub expected_revision: u64,
    pub continuation: Vec<u8>,
    pub journal: BTreeMap<String, JournalRow>,
    pub timers: Option<Vec<TimerRow>>,
    pub waits: Option<Vec<WaitRow>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitResult {
    pub revision: u64,
    pub owner: Option<String>,
    pub idempotent: bool,
}

#[derive(Debug, Clone)]
pub struct View {
    pub revision: u64,
    pub owner: Option<String>,
    pub lease_until: u64,
    pub continuation: Option<Vec<u8>>,
    pub journal: BTreeMap<String, JournalRow>,
    pub timers: Vec<TimerRow>,
    pub waits: Vec<WaitRow>,
    pub status: ContinuationStatus,
}

#[derive(Debug, Clone)]
pub struct Claimed {
    pub revision: u64,
    pub continuation: Option<Vec<u8>>,
    pub journal: BTreeMap<String, JournalRow>,
    pub timers: Vec<TimerRow>,
    pub waits: Vec<WaitRow>,
    pub timer_due: bool,
    pub status: ContinuationStatus,
}

struct Record {
    owner: Option<String>,
    lease_until: u64,
    revision: u64,
    commit_id: Option<String>,
    continuation: Option<Vec<u8>>,
    journal: BTreeMap<String, JournalRow>,
    timers: Vec<TimerRow>,
    waits: Vec<WaitRow>,
    status: ContinuationStatus,
}

struct State {
    clock: u64,
    blobs: BTreeMap<String, Vec<u8>>,
    executions: BTreeMap<String, Record>,
}

/// In-memory stand-in for the coordination object. It outlives any one host.
#[derive(Clone)]
pub struct Authority {
    inner: Arc<Mutex<State>>,
}

impl Authority {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                clock: 0,
                blobs: BTreeMap::new(),
                executions: BTreeMap::new(),
            })),
        }
    }

    pub fn now(&self) -> u64 {
        self.lock().clock
    }

    pub fn advance_clock(&self, to: u64) -> Result<()> {
        let mut state = self.lock();
        if to < state.clock {
            return Err(Error::new("clock cannot move backwards"));
        }
        state.clock = to;
        Ok(())
    }

    pub fn open(&self, execution_id: &str, owner_token: &str, lease_ms: u64) -> Result<()> {
        let mut state = self.lock();
        if state.executions.contains_key(execution_id) {
            return Err(Error::new("execution already exists"));
        }
        let lease_until = state.clock.saturating_add(lease_ms);
        state.executions.insert(
            execution_id.to_string(),
            Record {
                owner: Some(owner_token.to_string()),
                lease_until,
                revision: 0,
                commit_id: None,
                continuation: None,
                journal: BTreeMap::new(),
                timers: Vec::new(),
                waits: Vec::new(),
                status: ContinuationStatus::Runnable,
            },
        );
        Ok(())
    }

    pub fn commit(&self, commit: &Commit) -> Result<CommitResult> {
        let next = commit
            .expected_revision
            .checked_add(1)
            .ok_or_else(|| Error::new("revision overflow"))?;
        let continuation = decode_continuation(&commit.continuation)
            .map_err(|err| Error::new(format!("continuation decode failed: {err}")))?;
        if continuation.execution_id != commit.execution_id {
            return Err(Error::new("continuation execution id mismatch"));
        }
        if continuation.revision != next {
            return Err(Error::new(
                "continuation revision does not match the commit",
            ));
        }
        let derived_timers = timers_of(&continuation);
        let derived_waits = waits_of(&continuation);
        if let Some(timers) = &commit.timers {
            if timers != &derived_timers {
                return Err(Error::new("inconsistent timer metadata"));
            }
        }
        if let Some(waits) = &commit.waits {
            if waits != &derived_waits {
                return Err(Error::new("inconsistent wait metadata"));
            }
        }
        let digest = sha256(&commit.continuation);
        let identity = commit_identity(
            next,
            &digest,
            &commit.journal,
            &derived_timers,
            &derived_waits,
        );
        let mut state = self.lock();
        state
            .blobs
            .entry(digest.clone())
            .or_insert_with(|| commit.continuation.clone());
        let clock = state.clock;
        let Some(record) = state.executions.get_mut(&commit.execution_id) else {
            return Err(Error::new("unknown execution"));
        };
        if record.revision == next && record.commit_id.as_deref() == Some(identity.as_str()) {
            return Ok(CommitResult {
                revision: next,
                owner: record.owner.clone(),
                idempotent: true,
            });
        }
        if record.revision == next {
            return Err(Error::new("commit conflict"));
        }
        let lease_open = record.lease_until > clock;
        if record.owner.as_deref() != Some(commit.owner_token.as_str()) || !lease_open {
            return Err(Error::new("stale owner"));
        }
        if record.revision != commit.expected_revision {
            return Err(Error::new("commit conflict"));
        }
        record.revision = next;
        record.commit_id = Some(identity);
        record.continuation = Some(commit.continuation.clone());
        record.journal = commit.journal.clone();
        record.timers = derived_timers;
        record.waits = derived_waits;
        record.status = continuation.status;
        Ok(CommitResult {
            revision: next,
            owner: record.owner.clone(),
            idempotent: false,
        })
    }

    pub fn view(&self, execution_id: &str) -> Result<View> {
        let state = self.lock();
        let record = state
            .executions
            .get(execution_id)
            .ok_or_else(|| Error::new("unknown execution"))?;
        Ok(snapshot(record))
    }

    /// Take or renew ownership. A live lease held by someone else is rejected.
    /// Does not read a continuation except by returning the committed one.
    pub fn claim(&self, execution_id: &str, owner_token: &str, lease_ms: u64) -> Result<Claimed> {
        let mut state = self.lock();
        let clock = state.clock;
        let record = state
            .executions
            .get_mut(execution_id)
            .ok_or_else(|| Error::new("unknown execution"))?;
        if let Some(owner) = record.owner.as_deref() {
            if owner != owner_token && record.lease_until > clock {
                return Err(Error::new("execution is owned"));
            }
        }
        record.owner = Some(owner_token.to_string());
        record.lease_until = clock.saturating_add(lease_ms);
        let timer_due = record.status == ContinuationStatus::Suspended
            && record.timers.iter().any(|timer| timer.due_at <= clock);
        Ok(Claimed {
            revision: record.revision,
            continuation: record.continuation.clone(),
            journal: record.journal.clone(),
            timers: record.timers.clone(),
            waits: record.waits.clone(),
            timer_due,
            status: record.status,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.lock().expect("authority lock")
    }
}

impl Default for Authority {
    fn default() -> Self {
        Self::new()
    }
}

fn snapshot(record: &Record) -> View {
    View {
        revision: record.revision,
        owner: record.owner.clone(),
        lease_until: record.lease_until,
        continuation: record.continuation.clone(),
        journal: record.journal.clone(),
        timers: record.timers.clone(),
        waits: record.waits.clone(),
        status: record.status,
    }
}

fn timers_of(continuation: &Continuation) -> Vec<TimerRow> {
    match &continuation.pending {
        Some(PendingOp::Wait {
            kind: WaitKind::Timer { resume_at_ms },
        }) => vec![TimerRow {
            branch: String::new(),
            due_at: *resume_at_ms,
        }],
        _ => Vec::new(),
    }
}

fn waits_of(continuation: &Continuation) -> Vec<WaitRow> {
    match &continuation.pending {
        Some(PendingOp::Wait {
            kind:
                WaitKind::Event {
                    wait_id,
                    event_name,
                    ..
                },
        }) => vec![WaitRow {
            wait_id: wait_id.clone(),
            event: event_name.clone(),
        }],
        _ => Vec::new(),
    }
}

pub(crate) fn commit_identity(
    revision: u64,
    digest: &str,
    journal: &BTreeMap<String, JournalRow>,
    timers: &[TimerRow],
    waits: &[WaitRow],
) -> String {
    let mut raw = format!("v1\n{revision}\n{digest}\n");
    for (key, row) in journal {
        raw.push_str(&format!(
            "j\t{key}\t{}\t{}\t{}\t{}\n",
            row.idempotency_key, row.status, row.input, row.result
        ));
    }
    let mut timers: Vec<&TimerRow> = timers.iter().collect();
    timers.sort();
    for timer in timers {
        raw.push_str(&format!("t\t{}\t{}\n", timer.branch, timer.due_at));
    }
    let mut waits: Vec<&WaitRow> = waits.iter().collect();
    waits.sort();
    for wait in waits {
        raw.push_str(&format!("w\t{}\t{}\n", wait.wait_id, wait.event));
    }
    sha256(raw.as_bytes())
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tcc_state::{encode_continuation, Continuation};

    fn continuation(execution_id: &str, revision: u64, due: Option<u64>) -> Vec<u8> {
        let mut continuation =
            Continuation::start(execution_id, "artifact", 1, "ts.subset.v1", 0, 0);
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
            expected_revision: 0,
            continuation: continuation("exec", 1, Some(5_000)),
            journal: journal("first", "1"),
            timers: None,
            waits: None,
        };
        let applied = authority.commit(&commit).unwrap();
        assert!(!applied.idempotent);
        assert_eq!(applied.owner.as_deref(), Some("A"));
        authority.advance_clock(1_000).unwrap();
        authority.claim("exec", "B", 1_000).unwrap();
        let lease = authority.view("exec").unwrap().lease_until;
        let retry = authority.commit(&commit).unwrap();
        assert!(retry.idempotent);
        assert_eq!(retry.owner.as_deref(), Some("B"));
        let view = authority.view("exec").unwrap();
        assert_eq!(view.owner.as_deref(), Some("B"));
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
            expected_revision: 0,
            continuation: bytes.clone(),
            journal: journal("first", "1"),
            timers: None,
            waits: None,
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
                expected_revision: 0,
                continuation: continuation("exec", 1, Some(5_000)),
                journal: BTreeMap::new(),
                timers: None,
                waits: None,
            })
            .unwrap();
        authority.advance_clock(1_000).unwrap();
        let early = authority.claim("exec", "B", 1_000).unwrap();
        assert!(!early.timer_due);
        authority.advance_clock(5_000).unwrap();
        let due = authority.claim("exec", "C", 1_000).unwrap();
        assert!(due.timer_due);
        assert_eq!(due.revision, 1);
        assert!(due.continuation.is_some());
    }
}
