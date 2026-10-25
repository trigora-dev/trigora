use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

use tcc_core::Engine;
use tcc_ir::{Artifact, EngineCaps};

use crate::authority::Authority;
use crate::drive::{Answer, Memory};
use crate::socket::{Connection, Listener};

use super::{Dispatch, Timings, VersionWait};

pub(super) enum Phase {
    Runnable,
    Driving,
    WaitingForEffect,
    WaitingForCommit,
    WaitingForAdapter,
    Recovering,
    Suspended,
    Completed,
    Cancelled,
    Abandoned,
}

#[derive(Default)]
pub(super) struct Partial {
    pub(super) drive: Duration,
    pub(super) encode_input: Duration,
    pub(super) schedule: Duration,
    pub(super) socket_write: Duration,
    pub(super) socket_read: Duration,
    pub(super) round_trip: Duration,
    pub(super) decode_result: Duration,
    pub(super) journal: Duration,
    pub(super) continuation: Duration,
    pub(super) attempt_id: String,
}

pub(super) struct Slot {
    pub(super) phase: Mutex<Phase>,
    pub(super) engine: Mutex<Option<Engine>>,
    pub(super) memory: Mutex<Memory>,
    pub(super) answer: Mutex<Option<Answer>>,
    pub(super) confirm: Mutex<Option<std::result::Result<u64, String>>>,
    pub(super) partial: Mutex<Partial>,
    pub(super) attempts: AtomicU32,
    pub(super) timeouts: AtomicU32,
    pub(super) fence: AtomicU64,
    pub(super) lease_until: AtomicU64,
    pub(super) live: AtomicBool,
    pub(super) renewing: AtomicBool,
    pub(super) cancel: AtomicBool,
    pub(super) child_ready: AtomicBool,
    pub(super) artifact_hash: String,
    pub(super) handler_keys: Vec<String>,
}

pub(super) struct Backlog {
    pub(super) id: String,
    pub(super) engine: Engine,
    pub(super) memory: Memory,
    pub(super) fence: u64,
    pub(super) lease_until: u64,
    pub(super) artifact_hash: String,
    pub(super) handler_keys: Vec<String>,
}

pub(super) struct QueueState {
    pub(super) runnable: VecDeque<String>,
    pub(super) backlog: VecDeque<Backlog>,
    pub(super) stop: bool,
    pub(super) inflight: usize,
    pub(super) max_inflight: usize,
    pub(super) backlogged: bool,
}

pub(super) struct Inner {
    pub(super) authority: Arc<Authority>,
    pub(super) artifacts: Mutex<HashMap<String, Artifact>>,
    pub(super) initial_hash: String,
    pub(super) caps: EngineCaps,
    pub(super) owner: String,
    pub(super) cap: usize,
    pub(super) effect_timeout: Duration,
    pub(super) lease_ms: u64,
    pub(super) max_effect_attempts: u32,
    pub(super) fault_after_commits: Option<u32>,
    pub(super) listener: Listener,
    pub(super) stop: AtomicBool,
    pub(super) stopped: AtomicBool,
    pub(super) connections: Mutex<Vec<Arc<Connection>>>,
    pub(super) slots: Mutex<HashMap<String, Arc<Slot>>>,
    pub(super) queue: Mutex<QueueState>,
    pub(super) queue_cv: Condvar,
    pub(super) completed: AtomicUsize,
    pub(super) suspended: AtomicUsize,
    pub(super) cancelled: AtomicUsize,
    pub(super) abandoned: AtomicUsize,
    pub(super) commits_done: AtomicU32,
    pub(super) dispatches: Mutex<Vec<Dispatch>>,
    pub(super) failures: Mutex<Vec<(String, String)>>,
    pub(super) waiting: Mutex<Vec<VersionWait>>,
    pub(super) timings: Mutex<Timings>,
    pub(super) log_timings: bool,
    pub(super) workers: Mutex<Vec<JoinHandle<()>>>,
    pub(super) accept: Mutex<Option<JoinHandle<()>>>,
    pub(super) renewers: Mutex<Vec<JoinHandle<()>>>,
    pub(super) me: OnceLock<Weak<Inner>>,
}

pub(super) fn version_wait(execution_id: &str, artifact_hash: &str) -> VersionWait {
    VersionWait {
        execution_id: execution_id.to_string(),
        artifact_hash: artifact_hash.to_string(),
        reason: "waiting-for-version".to_string(),
        message: format!(
            "Execution requires artifact {artifact_hash}, but no compatible worker is connected. Deploy a worker with the required version to resume."
        ),
    }
}
