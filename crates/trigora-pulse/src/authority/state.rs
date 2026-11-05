use std::collections::BTreeMap;

use tcc_state::ContinuationStatus;

use crate::error::{Error, Result};

use super::types::{JournalRow, TimerRow, View, WaitRow};

pub(super) struct StoredEvent {
    pub(super) wait_id: String,
    pub(super) event: String,
    pub(super) payload: String,
    pub(super) digest: String,
}

pub(super) struct StoredIntent {
    pub(super) invoke_id: String,
    pub(super) child_execution_id: String,
    pub(super) program_name: String,
    pub(super) arguments: String,
    pub(super) digest: String,
    pub(super) committed: bool,
}

#[derive(Clone)]
pub(super) struct CreatedLink {
    pub(super) parent_execution_id: String,
    pub(super) invoke_id: String,
    pub(super) program_name: String,
    pub(super) arguments: String,
    pub(super) digest: String,
}

pub(super) struct Record {
    pub(super) owner: Option<String>,
    pub(super) fence: u64,
    pub(super) lease_until: u64,
    pub(super) revision: u64,
    pub(super) commit_id: Option<String>,
    pub(super) continuation: Option<Vec<u8>>,
    pub(super) journal: BTreeMap<String, JournalRow>,
    pub(super) timers: Vec<TimerRow>,
    pub(super) waits: Vec<WaitRow>,
    pub(super) status: ContinuationStatus,
    pub(super) artifact_hash: String,
    pub(super) handler_keys: Vec<String>,
    pub(super) event: Option<StoredEvent>,
    pub(super) cancel_requested: bool,
    /// One intent per child id. The key is `child_execution_id`.
    pub(super) intents: BTreeMap<String, StoredIntent>,
    pub(super) created: Option<CreatedLink>,
}

impl Record {
    pub(super) fn new(owner: String, fence: u64, lease_until: u64) -> Self {
        Self {
            owner: Some(owner),
            fence,
            lease_until,
            revision: 0,
            commit_id: None,
            continuation: None,
            journal: BTreeMap::new(),
            timers: Vec::new(),
            waits: Vec::new(),
            status: ContinuationStatus::Runnable,
            artifact_hash: String::new(),
            handler_keys: Vec::new(),
            event: None,
            cancel_requested: false,
            intents: BTreeMap::new(),
            created: None,
        }
    }
}

pub(super) struct State {
    pub(super) clock: u64,
    pub(super) blobs: BTreeMap<String, Vec<u8>>,
    pub(super) executions: BTreeMap<String, Record>,
}

impl State {
    pub(super) fn new() -> Self {
        Self {
            clock: 0,
            blobs: BTreeMap::new(),
            executions: BTreeMap::new(),
        }
    }
}

pub(super) fn advance_clock(state: &mut State, to: u64) -> Result<()> {
    if to < state.clock {
        return Err(Error::new("clock cannot move backwards"));
    }
    state.clock = to;
    Ok(())
}

pub(super) fn insert(
    state: &mut State,
    execution_id: &str,
    owner_token: &str,
    fence: u64,
    lease_until: u64,
) -> Result<()> {
    if state.executions.contains_key(execution_id) {
        return Err(Error::new("execution already exists"));
    }
    state.executions.insert(
        execution_id.to_string(),
        Record::new(owner_token.to_string(), fence, lease_until),
    );
    Ok(())
}

pub(super) fn snapshot(record: &Record) -> View {
    View {
        revision: record.revision,
        owner: record.owner.clone(),
        fence: record.fence,
        lease_until: record.lease_until,
        continuation: record.continuation.clone(),
        journal: record.journal.clone(),
        timers: record.timers.clone(),
        waits: record.waits.clone(),
        status: record.status,
        artifact_hash: record.artifact_hash.clone(),
        handler_keys: record.handler_keys.clone(),
    }
}
