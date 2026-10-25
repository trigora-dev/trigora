use tcc_state::ContinuationStatus;

use super::identity::canonical_keys;
use super::invoke;
use super::state::{snapshot, Record, State};
use super::types::{AcceptedEvent, Claimed, CommitQuery, Renewed, VersionPin, View};
use crate::error::{Error, Result};

pub(super) fn open(
    state: &mut State,
    execution_id: &str,
    owner_token: &str,
    lease_ms: u64,
) -> Result<Claimed> {
    let lease_until = state.clock.saturating_add(lease_ms);
    state.executions.insert(
        execution_id.to_string(),
        Record::new(owner_token.to_string(), 1, lease_until),
    );
    Ok(Claimed {
        revision: 0,
        fence: 1,
        lease_until,
        continuation: None,
        journal: Default::default(),
        timers: Vec::new(),
        waits: Vec::new(),
        timer_due: false,
        status: ContinuationStatus::Runnable,
        event: None,
        cancel_requested: false,
        intent: None,
    })
}

pub(super) fn renew(
    state: &mut State,
    execution_id: &str,
    owner_token: &str,
    fence: u64,
    lease_ms: u64,
) -> Result<Renewed> {
    let clock = state.clock;
    let record = state
        .executions
        .get_mut(execution_id)
        .ok_or_else(|| Error::new("unknown execution"))?;
    if record.owner.as_deref() != Some(owner_token)
        || record.fence != fence
        || record.lease_until <= clock
    {
        return Err(Error::new("stale owner"));
    }
    record.lease_until = clock.saturating_add(lease_ms);
    Ok(Renewed {
        owner: owner_token.to_string(),
        fence: record.fence,
        lease_until: record.lease_until,
    })
}

pub(super) fn query(
    state: &State,
    execution_id: &str,
    revision: u64,
    identity: &str,
) -> Result<CommitQuery> {
    let record = state
        .executions
        .get(execution_id)
        .ok_or_else(|| Error::new("unknown execution"))?;
    Ok(CommitQuery {
        accepted: record.revision == revision
            && record.commit_id.as_deref() == Some(identity)
            && !identity.is_empty(),
        revision: record.revision,
        identity: record.commit_id.clone().unwrap_or_default(),
        owner: record.owner.clone(),
        fence: record.fence,
        lease_until: record.lease_until,
    })
}

pub(super) fn view(state: &State, execution_id: &str) -> Result<View> {
    let record = state
        .executions
        .get(execution_id)
        .ok_or_else(|| Error::new("unknown execution"))?;
    Ok(snapshot(record))
}

pub(super) fn pin(state: &State, execution_id: &str) -> Result<VersionPin> {
    let record = state
        .executions
        .get(execution_id)
        .ok_or_else(|| Error::new("unknown execution"))?;
    Ok(VersionPin {
        revision: record.revision,
        artifact_hash: record.artifact_hash.clone(),
        handler_keys: record.handler_keys.clone(),
    })
}

pub(super) fn claim(
    state: &mut State,
    execution_id: &str,
    owner_token: &str,
    lease_ms: u64,
    artifact_hash: &str,
    handler_keys: &[String],
) -> Result<Claimed> {
    let clock = state.clock;
    let record = state
        .executions
        .get_mut(execution_id)
        .ok_or_else(|| Error::new("unknown execution"))?;
    if record.created.is_some() {
        return Err(Error::new("child is not initialized"));
    }
    if !record.artifact_hash.is_empty()
        && (record.artifact_hash != artifact_hash
            || record.handler_keys != canonical_keys(handler_keys))
    {
        return Err(Error::new("artifact version mismatch"));
    }
    let same = record.owner.as_deref() == Some(owner_token);
    let lease_open = record.lease_until > clock;
    if same && lease_open {
        record.lease_until = clock.saturating_add(lease_ms);
    } else if lease_open {
        return Err(Error::new("execution is owned"));
    } else {
        record.owner = Some(owner_token.to_string());
        record.fence = record.fence.saturating_add(1);
        record.lease_until = clock.saturating_add(lease_ms);
    }
    let timer_due = record.status == ContinuationStatus::Suspended
        && record.timers.iter().any(|timer| timer.due_at <= clock);
    Ok(Claimed {
        revision: record.revision,
        fence: record.fence,
        lease_until: record.lease_until,
        continuation: record.continuation.clone(),
        journal: record.journal.clone(),
        timers: record.timers.clone(),
        waits: record.waits.clone(),
        timer_due,
        status: record.status,
        event: record.event.as_ref().map(|event| AcceptedEvent {
            wait_id: event.wait_id.clone(),
            event: event.event.clone(),
            payload: event.payload.clone(),
        }),
        cancel_requested: record.cancel_requested,
        intent: invoke::pending(&record.intent),
    })
}
