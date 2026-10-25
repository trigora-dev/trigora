use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::sync::Weak;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tcc_core::{Engine, HostResponse};
use tcc_ir::{Artifact, EngineCaps};
use tcc_state::decode_continuation;

use crate::authority::{canonical_keys, Authority};
use crate::drive::{self, Answer, Memory, Step};
use crate::error::{Error, Result};
use crate::socket::{Connection, DispatchError, HostCommand, Listener, TransportSample};

#[derive(Debug, Clone)]
pub struct Dispatch {
    pub execution_id: String,
    pub attempt_id: String,
    pub key: String,
}

#[derive(Debug, Clone, Default)]
pub struct Span {
    pub execution_id: String,
    pub attempt_id: String,
    pub drive: Duration,
    pub encode_input: Duration,
    pub schedule: Duration,
    pub socket_write: Duration,
    pub socket_read: Duration,
    pub round_trip: Duration,
    pub decode_result: Duration,
    pub journal: Duration,
    pub continuation: Duration,
    pub commit: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionWait {
    pub execution_id: String,
    pub artifact_hash: String,
    pub reason: String,
    pub message: String,
}

fn version_wait(execution_id: &str, artifact_hash: &str) -> VersionWait {
    VersionWait {
        execution_id: execution_id.to_string(),
        artifact_hash: artifact_hash.to_string(),
        reason: "waiting-for-version".to_string(),
        message: format!(
            "Execution requires artifact {artifact_hash}, but no compatible worker is connected. Deploy a worker with the required version to resume."
        ),
    }
}

#[derive(Debug, Clone, Default)]
pub struct Timings {
    pub socket: Vec<Duration>,
    pub commit: Vec<Duration>,
    pub restore: Vec<Duration>,
    pub spans: Vec<Span>,
}

pub struct Config {
    pub dir: PathBuf,
    pub owner_token: String,
    pub workers: usize,
    pub cap: usize,
    pub effect_timeout: Duration,
    pub lease_ms: u64,
    pub max_effect_attempts: u32,
    pub fault_after_commits: Option<u32>,
}

impl Config {
    pub fn new(dir: impl Into<PathBuf>, owner_token: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            owner_token: owner_token.into(),
            workers: 4,
            cap: 64,
            effect_timeout: Duration::from_secs(2),
            lease_ms: 60_000,
            max_effect_attempts: 2,
            fault_after_commits: None,
        }
    }
}

enum Phase {
    Runnable,
    Driving,
    WaitingForEffect,
    WaitingForCommit,
    WaitingForAdapter,
    Recovering,
    Suspended,
    Completed,
    Abandoned,
}

#[derive(Default)]
struct Partial {
    drive: Duration,
    encode_input: Duration,
    schedule: Duration,
    socket_write: Duration,
    socket_read: Duration,
    round_trip: Duration,
    decode_result: Duration,
    journal: Duration,
    continuation: Duration,
    attempt_id: String,
}

struct Slot {
    phase: Mutex<Phase>,
    engine: Mutex<Option<Engine>>,
    memory: Mutex<Memory>,
    answer: Mutex<Option<Answer>>,
    confirm: Mutex<Option<std::result::Result<u64, String>>>,
    partial: Mutex<Partial>,
    attempts: AtomicU32,
    timeouts: AtomicU32,
    fence: AtomicU64,
    lease_until: AtomicU64,
    live: AtomicBool,
    renewing: AtomicBool,
    artifact_hash: String,
    handler_keys: Vec<String>,
}

struct Backlog {
    id: String,
    engine: Engine,
    memory: Memory,
    fence: u64,
    lease_until: u64,
    artifact_hash: String,
    handler_keys: Vec<String>,
}

struct QueueState {
    runnable: VecDeque<String>,
    backlog: VecDeque<Backlog>,
    stop: bool,
    inflight: usize,
    max_inflight: usize,
    backlogged: bool,
}

struct Inner {
    authority: Arc<Authority>,
    artifacts: Mutex<HashMap<String, Artifact>>,
    initial_hash: String,
    caps: EngineCaps,
    owner: String,
    cap: usize,
    effect_timeout: Duration,
    lease_ms: u64,
    max_effect_attempts: u32,
    fault_after_commits: Option<u32>,
    listener: Listener,
    stop: AtomicBool,
    stopped: AtomicBool,
    connections: Mutex<Vec<Arc<Connection>>>,
    slots: Mutex<HashMap<String, Arc<Slot>>>,
    queue: Mutex<QueueState>,
    queue_cv: Condvar,
    completed: AtomicUsize,
    suspended: AtomicUsize,
    abandoned: AtomicUsize,
    commits_done: AtomicU32,
    dispatches: Mutex<Vec<Dispatch>>,
    failures: Mutex<Vec<(String, String)>>,
    waiting: Mutex<Vec<VersionWait>>,
    timings: Mutex<Timings>,
    log_timings: bool,
    workers: Mutex<Vec<JoinHandle<()>>>,
    accept: Mutex<Option<JoinHandle<()>>>,
    renewers: Mutex<Vec<JoinHandle<()>>>,
    me: OnceLock<Weak<Inner>>,
}

pub struct Runtime {
    inner: Arc<Inner>,
}

impl Runtime {
    pub fn open(authority: Arc<Authority>, artifact: Artifact, config: Config) -> Result<Self> {
        tcc_ir::validate(&artifact, &EngineCaps::current())
            .map_err(|err| Error::new(format!("artifact: {err}")))?;
        let initial_hash = artifact.envelope.artifact_hash.clone();
        let mut artifacts = HashMap::new();
        artifacts.insert(initial_hash.clone(), artifact);
        let workers = config.workers.max(1);
        let listener = Listener::bind(&config.dir)?;
        let inner = Arc::new(Inner {
            authority,
            artifacts: Mutex::new(artifacts),
            initial_hash,
            caps: EngineCaps::current(),
            owner: config.owner_token,
            cap: config.cap.max(1),
            effect_timeout: config.effect_timeout,
            lease_ms: config.lease_ms,
            max_effect_attempts: config.max_effect_attempts.max(1),
            fault_after_commits: config.fault_after_commits,
            listener,
            stop: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            connections: Mutex::new(Vec::new()),
            slots: Mutex::new(HashMap::new()),
            queue: Mutex::new(QueueState {
                runnable: VecDeque::new(),
                backlog: VecDeque::new(),
                stop: false,
                inflight: 0,
                max_inflight: 0,
                backlogged: false,
            }),
            queue_cv: Condvar::new(),
            completed: AtomicUsize::new(0),
            suspended: AtomicUsize::new(0),
            abandoned: AtomicUsize::new(0),
            commits_done: AtomicU32::new(0),
            dispatches: Mutex::new(Vec::new()),
            failures: Mutex::new(Vec::new()),
            waiting: Mutex::new(Vec::new()),
            timings: Mutex::new(Timings::default()),
            log_timings: std::env::var("TRIGORA_PULSE_TIMINGS").ok().as_deref() == Some("1"),
            workers: Mutex::new(Vec::new()),
            accept: Mutex::new(None),
            renewers: Mutex::new(Vec::new()),
            me: OnceLock::new(),
        });
        let _ = inner.me.set(Arc::downgrade(&inner));
        let accept_inner = Arc::clone(&inner);
        let accept = std::thread::spawn(move || accept_loop(accept_inner));
        *inner.accept.lock().expect("accept") = Some(accept);
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let worker = Arc::clone(&inner);
            handles.push(std::thread::spawn(move || worker_loop(worker)));
        }
        *inner.workers.lock().expect("workers") = handles;
        Ok(Self { inner })
    }

    pub fn dir(&self) -> &Path {
        self.inner.listener.dir()
    }

    pub fn start(&self, execution_id: &str) -> Result<()> {
        let hash = self.inner.initial_hash.clone();
        let keys = self.inner.keys_for(&hash);
        if self.inner.accept_execution(execution_id, &hash, &keys)? {
            self.inner.enqueue(execution_id.to_string());
        }
        Ok(())
    }

    pub fn resume(&self, execution_id: &str) -> Result<()> {
        self.inner.resume_execution(execution_id)
    }

    pub fn waiting(&self) -> Vec<VersionWait> {
        let mut waiting = self.inner.waiting.lock().expect("waiting").clone();
        let slots = self.inner.slots.lock().expect("slots");
        for (id, slot) in slots.iter() {
            if matches!(*slot.phase.lock().expect("phase"), Phase::WaitingForAdapter)
                && !waiting.iter().any(|item| item.execution_id == *id)
            {
                waiting.push(version_wait(id, &slot.artifact_hash));
            }
        }
        waiting
    }

    pub fn wait_until(&self, mut pred: impl FnMut(&Runtime) -> bool, timeout: Duration) -> bool {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if pred(self) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        pred(self)
    }

    pub fn completed(&self) -> usize {
        self.inner.completed.load(Ordering::SeqCst)
    }

    pub fn suspended(&self) -> usize {
        self.inner.suspended.load(Ordering::SeqCst)
    }

    pub fn abandoned(&self) -> usize {
        self.inner.abandoned.load(Ordering::SeqCst)
    }

    pub fn waiting_for_adapter(&self) -> usize {
        self.inner
            .slots
            .lock()
            .expect("slots")
            .values()
            .filter(|slot| matches!(*slot.phase.lock().expect("phase"), Phase::WaitingForAdapter))
            .count()
    }

    pub fn inflight(&self) -> usize {
        self.inner.queue.lock().expect("queue").inflight
    }

    pub fn max_inflight(&self) -> usize {
        self.inner.queue.lock().expect("queue").max_inflight
    }

    pub fn backlogged(&self) -> bool {
        self.inner.queue.lock().expect("queue").backlogged
    }

    pub fn dispatches(&self) -> Vec<Dispatch> {
        self.inner.dispatches.lock().expect("dispatches").clone()
    }

    pub fn failures(&self) -> Vec<(String, String)> {
        self.inner.failures.lock().expect("failures").clone()
    }

    pub fn adapter_count(&self) -> usize {
        self.inner.connections.lock().expect("connections").len()
    }

    pub fn timings(&self) -> Timings {
        self.inner.timings.lock().expect("timings").clone()
    }

    pub fn clear_timings(&self) {
        *self.inner.timings.lock().expect("timings") = Timings::default();
    }

    pub fn shutdown(&self) {
        self.inner.shutdown();
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(inner: Arc<Inner>) {
    loop {
        if inner.stop.load(Ordering::SeqCst) {
            break;
        }
        match inner.listener.accept() {
            Ok((stream, _)) => {
                if inner.stop.load(Ordering::SeqCst) {
                    break;
                }
                let bearer = inner.listener.bearer().to_string();
                let inner = Arc::clone(&inner);
                std::thread::spawn(move || {
                    let installed = Arc::clone(&inner);
                    let Ok((connection, reader)) =
                        Connection::handshake(stream, &bearer, |hash, _keys, artifact| {
                            installed.install_artifact(hash, artifact)
                        })
                    else {
                        return;
                    };
                    let (tx, rx) = std::sync::mpsc::channel();
                    connection.set_commands(tx);
                    inner
                        .connections
                        .lock()
                        .expect("connections")
                        .push(Arc::clone(&connection));
                    let commands = Arc::clone(&inner);
                    let writer = Arc::clone(&connection);
                    std::thread::spawn(move || {
                        while let Ok(command) = rx.recv() {
                            let HostCommand::Start(id) = command;
                            let hash = writer.artifact_hash.clone();
                            let keys = writer.keys.clone();
                            match commands.accept_execution(&id, &hash, &keys) {
                                Ok(true) => {
                                    writer.reply_start(&id, Ok(()));
                                    commands.enqueue(id);
                                }
                                Ok(false) => writer.reply_start(&id, Ok(())),
                                Err(err) => writer.reply_start(&id, Err(err)),
                            }
                        }
                    });
                    connection.spawn_reader(reader);
                    inner.reconcile();
                    inner.resume_waiting();
                });
            }
            Err(_) => break,
        }
    }
}

fn worker_loop(inner: Arc<Inner>) {
    while let Some(id) = inner.pop() {
        if inner.stop.load(Ordering::SeqCst) {
            break;
        }
        let Some(slot) = inner.slot(&id) else {
            continue;
        };
        if !slot.live.load(Ordering::SeqCst) {
            inner.abandon(&id, &slot);
            continue;
        }
        {
            let mut phase = slot.phase.lock().expect("phase");
            if !matches!(*phase, Phase::Runnable) {
                continue;
            }
            *phase = Phase::Driving;
        }
        let answer = slot.answer.lock().expect("answer").take();
        let confirm = slot.confirm.lock().expect("confirm").take();
        let step = {
            let mut engine_guard = slot.engine.lock().expect("engine");
            let Some(engine) = engine_guard.as_mut() else {
                drop(engine_guard);
                inner.abandon(&id, &slot);
                continue;
            };
            let mut memory = slot.memory.lock().expect("memory");
            let mut cost = drive::Cost::default();
            let step = drive::turn(drive::Turn {
                engine,
                memory: &mut memory,
                execution_id: id.clone(),
                owner_token: inner.owner.clone(),
                answer,
                confirm,
                cost: &mut cost,
            });
            inner.note_cost(&slot, cost);
            step
        };
        match step {
            Ok(step) => inner.finish(&id, &slot, step),
            Err(err) => {
                inner.note_failure(&id, err.to_string());
                inner.abandon(&id, &slot);
            }
        }
    }
}

impl Inner {
    #[allow(clippy::too_many_arguments)]
    fn admit(
        &self,
        id: String,
        engine: Engine,
        memory: Memory,
        fence: u64,
        lease_until: u64,
        artifact_hash: String,
        handler_keys: Vec<String>,
    ) {
        {
            let mut queue = self.queue.lock().expect("queue");
            if queue.inflight >= self.cap {
                queue.backlog.push_back(Backlog {
                    id,
                    engine,
                    memory,
                    fence,
                    lease_until,
                    artifact_hash,
                    handler_keys,
                });
                queue.backlogged = true;
                return;
            }
            queue.inflight += 1;
            queue.max_inflight = queue.max_inflight.max(queue.inflight);
        }
        self.launch(
            id,
            engine,
            memory,
            fence,
            lease_until,
            artifact_hash,
            handler_keys,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn launch(
        &self,
        id: String,
        engine: Engine,
        memory: Memory,
        fence: u64,
        lease_until: u64,
        artifact_hash: String,
        handler_keys: Vec<String>,
    ) {
        self.insert_slot(
            id.clone(),
            engine,
            memory,
            fence,
            lease_until,
            artifact_hash,
            handler_keys,
        );
        self.enqueue(id);
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_slot(
        &self,
        id: String,
        engine: Engine,
        memory: Memory,
        fence: u64,
        lease_until: u64,
        artifact_hash: String,
        handler_keys: Vec<String>,
    ) {
        let slot = Arc::new(Slot {
            phase: Mutex::new(Phase::Runnable),
            engine: Mutex::new(Some(engine)),
            memory: Mutex::new(memory),
            answer: Mutex::new(None),
            confirm: Mutex::new(None),
            partial: Mutex::new(Partial::default()),
            attempts: AtomicU32::new(0),
            timeouts: AtomicU32::new(0),
            fence: AtomicU64::new(fence),
            lease_until: AtomicU64::new(lease_until),
            live: AtomicBool::new(true),
            renewing: AtomicBool::new(false),
            artifact_hash,
            handler_keys,
        });
        self.spawn_renewal(&id, &slot);
        self.slots.lock().expect("slots").insert(id, slot);
    }

    fn enqueue(&self, id: String) {
        let mut queue = self.queue.lock().expect("queue");
        if queue.stop {
            return;
        }
        queue.runnable.push_back(id);
        self.queue_cv.notify_one();
    }

    fn pop(&self) -> Option<String> {
        let mut queue = self.queue.lock().expect("queue");
        loop {
            if queue.stop {
                return None;
            }
            if let Some(id) = queue.runnable.pop_front() {
                return Some(id);
            }
            queue = self.queue_cv.wait(queue).expect("queue wait");
        }
    }

    fn slot(&self, id: &str) -> Option<Arc<Slot>> {
        self.slots.lock().expect("slots").get(id).cloned()
    }

    fn accept_execution(
        &self,
        execution_id: &str,
        artifact_hash: &str,
        handler_keys: &[String],
    ) -> Result<bool> {
        if self.slot(execution_id).is_some() || self.in_backlog(execution_id) {
            return Err(Error::new("execution id is already in use"));
        }
        let artifact = self.artifact(artifact_hash)?;
        let handler_keys = canonical_keys(handler_keys);
        let opened = self
            .authority
            .open(execution_id, &self.owner, self.lease_ms)
            .map_err(|err| {
                if err.to_string() == "execution already exists" {
                    Error::new("execution id is already in use")
                } else {
                    err
                }
            })?;
        let engine = Engine::start_with_args(artifact, execution_id, &self.caps, &[])
            .map_err(|err| Error::new(err.to_string()))?;
        let memory = Memory {
            journal: BTreeMap::new(),
            revision: 0,
            last_effect: None,
        };
        {
            let mut queue = self.queue.lock().expect("queue");
            if queue.inflight >= self.cap {
                queue.backlog.push_back(Backlog {
                    id: execution_id.to_string(),
                    engine,
                    memory,
                    fence: opened.fence,
                    lease_until: opened.lease_until,
                    artifact_hash: artifact_hash.to_string(),
                    handler_keys,
                });
                queue.backlogged = true;
                return Ok(false);
            }
            queue.inflight += 1;
            queue.max_inflight = queue.max_inflight.max(queue.inflight);
        }
        self.insert_slot(
            execution_id.to_string(),
            engine,
            memory,
            opened.fence,
            opened.lease_until,
            artifact_hash.to_string(),
            handler_keys,
        );
        Ok(true)
    }

    fn in_backlog(&self, id: &str) -> bool {
        self.queue
            .lock()
            .expect("queue")
            .backlog
            .iter()
            .any(|item| item.id == id)
    }

    fn finish(self: &Arc<Self>, id: &str, slot: &Arc<Slot>, step: Step) {
        match step {
            Step::NeedEffect => self.dispatch_effect(id, slot, false),
            Step::TimedOut => self.dispatch_effect(id, slot, true),
            Step::Disconnected => self.park_for_adapter(id, slot),
            Step::Commit(commit) => self.begin_commit(id, slot, commit),
            Step::Suspended => self.settle(slot, Phase::Suspended, &self.suspended),
            Step::Finished => self.settle(slot, Phase::Completed, &self.completed),
            Step::Failed(message) => {
                self.note_failure(id, message);
                self.abandon(id, slot);
            }
        }
    }

    fn dispatch_effect(self: &Arc<Self>, id: &str, slot: &Arc<Slot>, from_timeout: bool) {
        let Some(last) = slot.memory.lock().expect("memory").last_effect.clone() else {
            self.abandon(id, slot);
            return;
        };
        if !self.has_adapter(&slot.artifact_hash, &slot.handler_keys, &last.key) {
            self.note_failure(
                id,
                format!("no adapter registered for effect `{}`", last.key),
            );
            *slot.phase.lock().expect("phase") = Phase::WaitingForAdapter;
            return;
        }
        if from_timeout {
            let timeouts = slot.timeouts.fetch_add(1, Ordering::SeqCst) + 1;
            if timeouts >= self.max_effect_attempts {
                self.abandon(id, slot);
                return;
            }
        } else {
            slot.timeouts.store(0, Ordering::SeqCst);
        }
        let attempt = slot.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        let attempt_id = format!("{id}:{attempt}");
        *slot.phase.lock().expect("phase") = Phase::WaitingForEffect;
        self.dispatches.lock().expect("dispatches").push(Dispatch {
            execution_id: id.to_string(),
            attempt_id: attempt_id.clone(),
            key: last.key.clone(),
        });
        let inner = Arc::clone(self);
        let id = id.to_string();
        let decided = Instant::now();
        let slot = Arc::clone(slot);
        std::thread::spawn(move || {
            let hash = slot.artifact_hash.clone();
            let keys = slot.handler_keys.clone();
            let sample = inner.hub_dispatch(&id, &attempt_id, &last.key, &last.input, &hash, &keys);
            inner.note_transport(&slot, &attempt_id, decided, &sample);
            let answer = match sample.outcome {
                Ok(value) => Answer::Value(value),
                Err(DispatchError::Timeout) => Answer::TimedOut,
                Err(DispatchError::Failed(message)) => Answer::Failed(message),
                Err(DispatchError::Disconnected | DispatchError::Unavailable) => {
                    Answer::Disconnected
                }
            };
            inner.deliver_answer(id, attempt_id, answer);
        });
    }

    fn hub_dispatch(
        &self,
        execution_id: &str,
        attempt_id: &str,
        key: &str,
        input: &str,
        artifact_hash: &str,
        handler_keys: &[String],
    ) -> TransportSample {
        let connections = self.connections.lock().expect("connections").clone();
        let pinned = canonical_keys(handler_keys);
        for connection in connections {
            if connection.alive()
                && connection.artifact_hash == artifact_hash
                && canonical_keys(&connection.keys) == pinned
                && connection.keys.iter().any(|known| known == key)
            {
                return connection.dispatch(
                    execution_id,
                    attempt_id,
                    key,
                    input,
                    self.effect_timeout,
                );
            }
        }
        TransportSample::failed(DispatchError::Unavailable)
    }

    fn deliver_answer(&self, id: String, attempt_id: String, answer: Answer) {
        let Some(slot) = self.slot(&id) else {
            return;
        };
        if !slot.live.load(Ordering::SeqCst) {
            return;
        }
        let mut phase = slot.phase.lock().expect("phase");
        if !matches!(*phase, Phase::WaitingForEffect) {
            return;
        }
        let current = slot.attempts.load(Ordering::SeqCst);
        if attempt_id != format!("{id}:{current}") {
            return;
        }
        *slot.answer.lock().expect("answer") = Some(answer);
        *phase = Phase::Runnable;
        drop(phase);
        self.enqueue(id);
    }

    fn begin_commit(
        self: &Arc<Self>,
        id: &str,
        slot: &Arc<Slot>,
        commit: crate::authority::Commit,
    ) {
        if !slot.live.load(Ordering::SeqCst) {
            self.abandon(id, slot);
            return;
        }
        let done = self.commits_done.load(Ordering::SeqCst);
        if self.fault_after_commits.is_some_and(|limit| done >= limit) {
            self.abandon(id, slot);
            return;
        }
        let mut commit = commit;
        commit.fence = slot.fence.load(Ordering::SeqCst);
        commit.artifact_hash = slot.artifact_hash.clone();
        commit.handler_keys = slot.handler_keys.clone();
        *slot.phase.lock().expect("phase") = Phase::WaitingForCommit;
        let inner = Arc::clone(self);
        let id = id.to_string();
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = inner.authority.commit(&commit);
            let commit = started.elapsed();
            inner.timings.lock().expect("timings").commit.push(commit);
            inner.deliver_confirm(id, result, commit);
        });
    }

    fn note_cost(&self, slot: &Slot, cost: drive::Cost) {
        let mut partial = slot.partial.lock().expect("partial");
        if let Some(drive) = cost.drive {
            partial.drive = drive;
        }
        if let Some(encode_input) = cost.encode_input {
            partial.encode_input = encode_input;
        }
        if let Some(decode_result) = cost.decode_result {
            partial.decode_result = decode_result;
        }
        if let Some(journal) = cost.journal {
            partial.journal = journal;
        }
        if let Some(continuation) = cost.continuation {
            partial.continuation = continuation;
        }
    }

    fn note_transport(
        &self,
        slot: &Slot,
        attempt_id: &str,
        decided: Instant,
        sample: &TransportSample,
    ) {
        let mut partial = slot.partial.lock().expect("partial");
        partial.attempt_id = attempt_id.to_string();
        partial.schedule = decided
            .elapsed()
            .saturating_sub(sample.socket_write + sample.round_trip);
        partial.socket_write = sample.socket_write;
        partial.socket_read = sample.socket_read;
        partial.round_trip = sample.round_trip;
    }

    fn record_span(&self, id: &str, slot: &Slot, commit: Duration) {
        let partial = std::mem::take(&mut *slot.partial.lock().expect("partial"));
        if partial.attempt_id.is_empty() {
            return;
        }
        let span = Span {
            execution_id: id.to_string(),
            attempt_id: partial.attempt_id,
            drive: partial.drive,
            encode_input: partial.encode_input,
            schedule: partial.schedule,
            socket_write: partial.socket_write,
            socket_read: partial.socket_read,
            round_trip: partial.round_trip,
            decode_result: partial.decode_result,
            journal: partial.journal,
            continuation: partial.continuation,
            commit,
        };
        if self.log_timings {
            eprintln!(
                "pulse-span {}",
                serde_json::json!({
                    "executionId": span.execution_id,
                    "attemptId": span.attempt_id,
                    "driveUs": span.drive.as_micros(),
                    "encodeInputUs": span.encode_input.as_micros(),
                    "scheduleUs": span.schedule.as_micros(),
                    "socketWriteUs": span.socket_write.as_micros(),
                    "socketReadUs": span.socket_read.as_micros(),
                    "roundTripUs": span.round_trip.as_micros(),
                    "decodeResultUs": span.decode_result.as_micros(),
                    "journalUs": span.journal.as_micros(),
                    "continuationUs": span.continuation.as_micros(),
                    "commitUs": span.commit.as_micros(),
                })
            );
        }
        let mut timings = self.timings.lock().expect("timings");
        timings.socket.push(span.round_trip);
        timings.spans.push(span);
    }

    fn deliver_confirm(
        &self,
        id: String,
        result: Result<crate::authority::CommitResult>,
        commit: Duration,
    ) {
        let Some(slot) = self.slot(&id) else {
            return;
        };
        let mut phase = slot.phase.lock().expect("phase");
        if !matches!(*phase, Phase::WaitingForCommit) {
            return;
        }
        match result {
            Ok(done)
                if done.owner.as_deref() == Some(self.owner.as_str())
                    && slot.live.load(Ordering::SeqCst)
                    && done.fence == slot.fence.load(Ordering::SeqCst) =>
            {
                if done.lease_until > 0 {
                    slot.lease_until.store(done.lease_until, Ordering::SeqCst);
                }
                self.commits_done.fetch_add(1, Ordering::SeqCst);
                self.record_span(&id, &slot, commit);
                *slot.confirm.lock().expect("confirm") = Some(Ok(done.revision));
                *phase = Phase::Runnable;
                drop(phase);
                self.enqueue(id);
            }
            _ => {
                drop(phase);
                self.abandon(&id, &slot);
            }
        }
    }

    fn settle(&self, slot: &Slot, phase: Phase, counter: &AtomicUsize) {
        slot.live.store(false, Ordering::SeqCst);
        *slot.phase.lock().expect("phase") = phase;
        *slot.engine.lock().expect("engine") = None;
        counter.fetch_add(1, Ordering::SeqCst);
        self.release_inflight();
    }

    fn has_adapter(&self, artifact_hash: &str, handler_keys: &[String], key: &str) -> bool {
        let connections = self.connections.lock().expect("connections");
        let pinned = canonical_keys(handler_keys);
        connections.iter().any(|connection| {
            connection.alive()
                && connection.artifact_hash == artifact_hash
                && canonical_keys(&connection.keys) == pinned
                && connection.keys.iter().any(|known| known == key)
        })
    }

    fn has_registration(&self, artifact_hash: &str, handler_keys: &[String]) -> bool {
        if !self
            .artifacts
            .lock()
            .expect("artifacts")
            .contains_key(artifact_hash)
        {
            return false;
        }
        let pinned = canonical_keys(handler_keys);
        self.connections
            .lock()
            .expect("connections")
            .iter()
            .any(|connection| {
                connection.alive()
                    && connection.artifact_hash == artifact_hash
                    && canonical_keys(&connection.keys) == pinned
            })
    }

    fn reconcile(self: &Arc<Self>) {
        let ids: Vec<String> = self.slots.lock().expect("slots").keys().cloned().collect();
        for id in ids {
            self.reconcile_one(&id);
        }
    }

    /// Resume only an execution that is waiting for this adapter's effect key.
    /// A suspended sleep or event stays suspended. A running engine is left in place.
    fn reconcile_one(self: &Arc<Self>, id: &str) {
        let Some(slot) = self.slot(id) else {
            return;
        };
        if !slot.live.load(Ordering::SeqCst) {
            return;
        }
        let key = {
            let memory = slot.memory.lock().expect("memory");
            memory.last_effect.as_ref().map(|effect| effect.key.clone())
        };
        let Some(key) = key else {
            return;
        };
        if !self.has_adapter(&slot.artifact_hash, &slot.handler_keys, &key) {
            return;
        }
        let restore = {
            let mut phase = slot.phase.lock().expect("phase");
            if !matches!(*phase, Phase::WaitingForAdapter) {
                return;
            }
            if slot.engine.lock().expect("engine").is_some() {
                *phase = Phase::Runnable;
                false
            } else {
                *phase = Phase::Recovering;
                true
            }
        };
        if restore {
            self.install_resumed(id, &slot);
        } else {
            self.enqueue(id.to_string());
        }
    }

    fn install_resumed(self: &Arc<Self>, id: &str, slot: &Slot) {
        match self.restore_engine(id, &slot.artifact_hash, &slot.handler_keys) {
            Ok((engine, memory)) => {
                *slot.engine.lock().expect("engine") = Some(engine);
                *slot.memory.lock().expect("memory") = memory;
                *slot.phase.lock().expect("phase") = Phase::Runnable;
                self.enqueue(id.to_string());
            }
            Err(err) => {
                self.note_failure(id, err.to_string());
                *slot.phase.lock().expect("phase") = Phase::WaitingForAdapter;
            }
        }
    }

    fn restore_engine(
        &self,
        execution_id: &str,
        artifact_hash: &str,
        handler_keys: &[String],
    ) -> Result<(Engine, Memory)> {
        let started = Instant::now();
        let artifact = self.artifact(artifact_hash)?;
        let claimed = self.authority.claim(
            execution_id,
            &self.owner,
            self.lease_ms,
            artifact_hash,
            handler_keys,
        )?;
        let engine = if let Some(bytes) = &claimed.continuation {
            let continuation =
                decode_continuation(bytes).map_err(|err| Error::new(format!("restore: {err}")))?;
            if continuation.artifact.hash != artifact_hash {
                return Err(Error::new("artifact hash does not match the continuation"));
            }
            let mut engine = Engine::resume(artifact, continuation, &self.caps)
                .map_err(|err| Error::new(err.to_string()))?;
            if claimed.timer_due {
                engine
                    .apply_host_response(HostResponse::TimerFired { branch: None })
                    .map_err(|err| Error::new(err.to_string()))?;
            }
            engine
        } else {
            Engine::start_with_args(artifact, execution_id, &self.caps, &[])
                .map_err(|err| Error::new(err.to_string()))?
        };
        self.timings
            .lock()
            .expect("timings")
            .restore
            .push(started.elapsed());
        Ok((
            engine,
            Memory {
                journal: claimed.journal,
                revision: claimed.revision,
                last_effect: None,
            },
        ))
    }

    fn artifact(&self, hash: &str) -> Result<Artifact> {
        self.artifacts
            .lock()
            .expect("artifacts")
            .get(hash)
            .cloned()
            .ok_or_else(|| Error::new("artifact is not loaded"))
    }

    fn keys_for(&self, hash: &str) -> Vec<String> {
        self.connections
            .lock()
            .expect("connections")
            .iter()
            .find(|connection| connection.alive() && connection.artifact_hash == hash)
            .map(|connection| canonical_keys(&connection.keys))
            .unwrap_or_default()
    }

    fn install_artifact(&self, hash: &str, artifact: Option<&str>) -> Result<()> {
        let Some(encoded) = artifact else {
            return Ok(());
        };
        let decoded = tcc_ir::decode_artifact(encoded)
            .map_err(|err| Error::new(format!("artifact: {err}")))?;
        if decoded.envelope.artifact_hash != hash {
            return Err(Error::new("artifact bytes do not match the declared hash"));
        }
        tcc_ir::validate(&decoded, &self.caps)
            .map_err(|err| Error::new(format!("artifact: {err}")))?;
        self.artifacts
            .lock()
            .expect("artifacts")
            .insert(hash.to_string(), decoded);
        Ok(())
    }

    fn resume_execution(&self, execution_id: &str) -> Result<()> {
        let pin = self.authority.pin(execution_id)?;
        let (hash, keys) = if pin.artifact_hash.is_empty() {
            let hash = self.initial_hash.clone();
            (hash.clone(), self.keys_for(&hash))
        } else if self.has_registration(&pin.artifact_hash, &pin.handler_keys) {
            (pin.artifact_hash, pin.handler_keys)
        } else {
            self.note_waiting(execution_id, &pin.artifact_hash);
            return Ok(());
        };
        match self.claim_and_admit(execution_id, &hash, &keys) {
            Ok(()) => {
                self.clear_waiting(execution_id);
                Ok(())
            }
            Err(err) if err.to_string() == "execution is owned" => {
                self.note_waiting(execution_id, &hash);
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn claim_and_admit(
        &self,
        execution_id: &str,
        artifact_hash: &str,
        handler_keys: &[String],
    ) -> Result<()> {
        let started = Instant::now();
        let artifact = self.artifact(artifact_hash)?;
        let claimed = self.authority.claim(
            execution_id,
            &self.owner,
            self.lease_ms,
            artifact_hash,
            handler_keys,
        )?;
        let engine = if let Some(bytes) = &claimed.continuation {
            let continuation =
                decode_continuation(bytes).map_err(|err| Error::new(format!("restore: {err}")))?;
            if continuation.artifact.hash != artifact_hash {
                return Err(Error::new("artifact hash does not match the continuation"));
            }
            let mut engine = Engine::resume(artifact, continuation, &self.caps)
                .map_err(|err| Error::new(err.to_string()))?;
            if claimed.timer_due {
                engine
                    .apply_host_response(HostResponse::TimerFired { branch: None })
                    .map_err(|err| Error::new(err.to_string()))?;
            }
            engine
        } else {
            Engine::start_with_args(artifact, execution_id, &self.caps, &[])
                .map_err(|err| Error::new(err.to_string()))?
        };
        self.timings
            .lock()
            .expect("timings")
            .restore
            .push(started.elapsed());
        self.admit(
            execution_id.to_string(),
            engine,
            Memory {
                journal: claimed.journal,
                revision: claimed.revision,
                last_effect: None,
            },
            claimed.fence,
            claimed.lease_until,
            artifact_hash.to_string(),
            canonical_keys(handler_keys),
        );
        Ok(())
    }

    fn resume_waiting(&self) {
        let pending: Vec<String> = self
            .waiting
            .lock()
            .expect("waiting")
            .iter()
            .map(|item| item.execution_id.clone())
            .collect();
        for id in pending {
            if let Err(err) = self.resume_execution(&id) {
                self.note_failure(&id, err.to_string());
            }
        }
    }

    fn note_waiting(&self, execution_id: &str, artifact_hash: &str) {
        let mut waiting = self.waiting.lock().expect("waiting");
        if waiting.iter().any(|item| item.execution_id == execution_id) {
            return;
        }
        waiting.push(version_wait(execution_id, artifact_hash));
    }

    fn clear_waiting(&self, execution_id: &str) {
        self.waiting
            .lock()
            .expect("waiting")
            .retain(|item| item.execution_id != execution_id);
    }

    fn park_for_adapter(self: &Arc<Self>, id: &str, slot: &Arc<Slot>) {
        {
            let mut phase = slot.phase.lock().expect("phase");
            if matches!(
                *phase,
                Phase::Suspended
                    | Phase::Completed
                    | Phase::Abandoned
                    | Phase::WaitingForAdapter
                    | Phase::Recovering
            ) {
                return;
            }
            // The engine is still at the outstanding effect, which is the last
            // committed revision. A late result is ignored because this attempt
            // is no longer `WaitingForEffect`.
            *phase = Phase::WaitingForAdapter;
        }
        self.reconcile_one(id);
    }

    fn note_failure(&self, id: &str, message: String) {
        self.failures
            .lock()
            .expect("failures")
            .push((id.to_string(), message));
    }

    fn abandon(&self, _id: &str, slot: &Slot) {
        {
            let mut phase = slot.phase.lock().expect("phase");
            if matches!(
                *phase,
                Phase::Suspended | Phase::Completed | Phase::Abandoned
            ) {
                return;
            }
            *phase = Phase::Abandoned;
        }
        *slot.engine.lock().expect("engine") = None;
        self.abandoned.fetch_add(1, Ordering::SeqCst);
        self.release_inflight();
    }

    fn release_inflight(&self) {
        let next = {
            let mut queue = self.queue.lock().expect("queue");
            queue.inflight = queue.inflight.saturating_sub(1);
            if queue.stop {
                None
            } else if let Some(item) = queue.backlog.pop_front() {
                queue.inflight += 1;
                queue.max_inflight = queue.max_inflight.max(queue.inflight);
                Some(item)
            } else {
                None
            }
        };
        if let Some(item) = next {
            self.launch(
                item.id,
                item.engine,
                item.memory,
                item.fence,
                item.lease_until,
                item.artifact_hash,
                item.handler_keys,
            );
        }
    }

    fn shutdown(&self) {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        self.stop.store(true, Ordering::SeqCst);
        {
            let mut queue = self.queue.lock().expect("queue");
            queue.stop = true;
            self.queue_cv.notify_all();
        }
        let _ = std::os::unix::net::UnixStream::connect(self.listener.socket_path());
        if let Some(handle) = self.accept.lock().expect("accept").take() {
            let _ = handle.join();
        }
        for connection in self.connections.lock().expect("connections").drain(..) {
            connection.close();
        }
        let workers = std::mem::take(&mut *self.workers.lock().expect("workers"));
        for handle in workers {
            let _ = handle.join();
        }
        let renewers = std::mem::take(&mut *self.renewers.lock().expect("renewers"));
        for handle in renewers {
            let _ = handle.join();
        }
    }

    fn spawn_renewal(&self, id: &str, slot: &Arc<Slot>) {
        if !self.authority.has_remote() {
            return;
        }
        if slot.renewing.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(inner) = self.me.get().and_then(Weak::upgrade) else {
            return;
        };
        let id = id.to_string();
        let slot = Arc::clone(slot);
        let handle = std::thread::spawn(move || inner.renew_loop(id, slot));
        self.renewers.lock().expect("renewers").push(handle);
    }

    fn renew_loop(self: &Arc<Self>, id: String, slot: Arc<Slot>) {
        let mut failures = 0u32;
        while !self.stop.load(Ordering::SeqCst) && slot.live.load(Ordering::SeqCst) {
            let until = slot.lease_until.load(Ordering::SeqCst);
            let margin = (self.lease_ms / 3).max(50);
            let now = unix_ms();
            let wake = until.saturating_sub(margin);
            if until == 0 || now < wake {
                let pause = if until == 0 {
                    50
                } else {
                    (wake - now).min(200)
                };
                std::thread::sleep(Duration::from_millis(pause));
                continue;
            }
            let fence = slot.fence.load(Ordering::SeqCst);
            match self.authority.renew(&id, &self.owner, fence, self.lease_ms) {
                Ok(renewed) if renewed.fence == fence => {
                    failures = 0;
                    slot.lease_until
                        .store(renewed.lease_until, Ordering::SeqCst);
                }
                Ok(_) => {
                    slot.live.store(false, Ordering::SeqCst);
                    self.abandon(&id, &slot);
                    break;
                }
                Err(err) => {
                    let message = err.to_string();
                    let fatal = message == "stale owner" || message == "execution is owned";
                    failures += 1;
                    if fatal || failures >= 3 {
                        slot.live.store(false, Ordering::SeqCst);
                        self.abandon(&id, &slot);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50 * u64::from(failures)));
                }
            }
        }
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
