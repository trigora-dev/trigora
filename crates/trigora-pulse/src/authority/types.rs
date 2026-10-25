use std::collections::BTreeMap;

use tcc_state::ContinuationStatus;

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
    pub fence: u64,
    pub expected_revision: u64,
    pub continuation: Vec<u8>,
    pub journal: BTreeMap<String, JournalRow>,
    pub timers: Option<Vec<TimerRow>>,
    pub waits: Option<Vec<WaitRow>>,
    /// Empty means the continuation's artifact hash is the pin.
    pub artifact_hash: String,
    pub handler_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitResult {
    pub revision: u64,
    pub owner: Option<String>,
    pub fence: u64,
    pub lease_until: u64,
    pub idempotent: bool,
}

/// One payload accepted for one committed wait. A claim returns it. A view does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedEvent {
    pub wait_id: String,
    pub event: String,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventReceipt {
    pub revision: u64,
    pub fence: u64,
    pub idempotent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelReceipt {
    pub revision: u64,
    pub idempotent: bool,
}

/// One child the parent has asked to create. The argument vector is stored in
/// full so a later owner can create that child without the original process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvokeIntent {
    pub invoke_id: String,
    pub child_execution_id: String,
    pub program_name: String,
    pub arguments: String,
    pub digest: String,
}

/// The invoke a host is asking to record or to match against the stored intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvokeSpec {
    pub invoke_id: String,
    pub child_execution_id: String,
    pub program_name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvokeReceipt {
    pub revision: u64,
    pub idempotent: bool,
}

/// Ownership of a created child, plus the arguments its first continuation must bind.
/// `initialized` is true only after that continuation has been committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedChild {
    pub parent_execution_id: String,
    pub invoke_id: String,
    pub program_name: String,
    pub arguments: String,
    pub revision: u64,
    pub fence: u64,
    pub lease_until: u64,
    pub initialized: bool,
}

/// A child row that exists and has not been initialized with a continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedChild {
    pub parent_execution_id: String,
    pub invoke_id: String,
    pub child_execution_id: String,
    pub program_name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renewed {
    pub owner: String,
    pub fence: u64,
    pub lease_until: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitQuery {
    pub accepted: bool,
    pub revision: u64,
    pub identity: String,
    pub owner: Option<String>,
    pub fence: u64,
    pub lease_until: u64,
}

#[derive(Debug, Clone)]
pub struct View {
    pub revision: u64,
    pub owner: Option<String>,
    pub fence: u64,
    pub lease_until: u64,
    pub continuation: Option<Vec<u8>>,
    pub journal: BTreeMap<String, JournalRow>,
    pub timers: Vec<TimerRow>,
    pub waits: Vec<WaitRow>,
    pub status: ContinuationStatus,
    pub artifact_hash: String,
    pub handler_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionPin {
    pub revision: u64,
    pub artifact_hash: String,
    pub handler_keys: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Claimed {
    pub revision: u64,
    pub fence: u64,
    pub lease_until: u64,
    pub continuation: Option<Vec<u8>>,
    pub journal: BTreeMap<String, JournalRow>,
    pub timers: Vec<TimerRow>,
    pub waits: Vec<WaitRow>,
    pub timer_due: bool,
    pub status: ContinuationStatus,
    pub event: Option<AcceptedEvent>,
    pub cancel_requested: bool,
    /// Present only while the parent has not committed the wait for this child.
    pub intent: Option<InvokeIntent>,
}
