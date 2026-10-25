use tcc_state::{decode_continuation, Continuation};

use super::identity::{canonical_keys, commit_identity, is_terminal, sha256, timers_of, waits_of};
use super::invoke;
use super::signals::release_event;
use super::state::State;
use super::types::{Commit, CommitResult, TimerRow, WaitRow};
use crate::error::{Error, Result};

pub(super) struct Prepared {
    next: u64,
    continuation: Continuation,
    derived_timers: Vec<TimerRow>,
    derived_waits: Vec<WaitRow>,
    digest: String,
    identity: String,
}

pub(super) fn prepare(commit: &Commit) -> Result<Prepared> {
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
    Ok(Prepared {
        next,
        continuation,
        derived_timers,
        derived_waits,
        digest,
        identity,
    })
}

pub(super) fn apply(
    state: &mut State,
    commit: &Commit,
    prepared: Prepared,
) -> Result<CommitResult> {
    let Prepared {
        next,
        continuation,
        derived_timers,
        derived_waits,
        digest,
        identity,
    } = prepared;
    state
        .blobs
        .entry(digest)
        .or_insert_with(|| commit.continuation.clone());
    let clock = state.clock;
    let Some(record) = state.executions.get_mut(&commit.execution_id) else {
        return Err(Error::new("unknown execution"));
    };
    let lease_open = record.lease_until > clock;
    if record.owner.as_deref() != Some(commit.owner_token.as_str())
        || record.fence != commit.fence
        || !lease_open
    {
        return Err(Error::new("stale owner"));
    }
    let hash = if commit.artifact_hash.is_empty() {
        continuation.artifact.hash.clone()
    } else if commit.artifact_hash == continuation.artifact.hash {
        commit.artifact_hash.clone()
    } else {
        return Err(Error::new("commit conflict"));
    };
    let keys = canonical_keys(&commit.handler_keys);
    if !record.artifact_hash.is_empty()
        && (record.artifact_hash != hash || record.handler_keys != keys)
    {
        return Err(Error::new("commit conflict"));
    }
    if record.revision == next && record.commit_id.as_deref() == Some(identity.as_str()) {
        return Ok(CommitResult {
            revision: next,
            owner: record.owner.clone(),
            fence: record.fence,
            lease_until: record.lease_until,
            idempotent: true,
        });
    }
    if record.revision == next || record.revision != commit.expected_revision {
        return Err(Error::new("commit conflict"));
    }
    if is_terminal(record.status) {
        return Err(Error::new("terminal state"));
    }
    let previous_waits = record.waits.clone();
    record.revision = next;
    record.commit_id = Some(identity);
    record.continuation = Some(commit.continuation.clone());
    record.journal = commit.journal.clone();
    record.timers = derived_timers;
    record.waits = derived_waits;
    release_event(&mut record.event, &previous_waits, &record.waits);
    record.status = continuation.status;
    if is_terminal(continuation.status) {
        record.cancel_requested = false;
    }
    invoke::consume(&mut record.intents, &continuation);
    if record.artifact_hash.is_empty() {
        record.artifact_hash = hash;
        record.handler_keys = keys;
    }
    Ok(CommitResult {
        revision: next,
        owner: record.owner.clone(),
        fence: record.fence,
        lease_until: record.lease_until,
        idempotent: false,
    })
}
