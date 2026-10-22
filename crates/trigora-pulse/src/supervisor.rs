use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tcc_core::{Engine, HostResponse};
use tcc_ir::{Artifact, EngineCaps};
use tcc_state::decode_continuation;

use crate::authority::Authority;
use crate::drive::{self, Answer, Memory, Step};
use crate::error::{Error, Result};
use crate::socket::{Connection, DispatchError, HostCommand, Listener};

#[derive(Debug, Clone)]
pub struct Dispatch {
    pub execution_id: String,
    pub attempt_id: String,
    pub key: String,
}

#[derive(Debug, Clone, Default)]
pub struct Timings {
    pub socket: Vec<Duration>,
    pub commit: Vec<Duration>,
    pub restore: Vec<Duration>,
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

struct Slot {
    phase: Mutex<Phase>,
    engine: Mutex<Option<Engine>>,
    memory: Mutex<Memory>,
    answer: Mutex<Option<Answer>>,
    confirm: Mutex<Option<std::result::Result<u64, String>>>,
    attempts: AtomicU32,
    timeouts: AtomicU32,
}

struct Backlog {
    id: String,
    engine: Engine,
    memory: Memory,
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
    artifact: Artifact,
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
    timings: Mutex<Timings>,
    workers: Mutex<Vec<JoinHandle<()>>>,
    accept: Mutex<Option<JoinHandle<()>>>,
}

pub struct Runtime {
    inner: Arc<Inner>,
}

impl Runtime {
    pub fn open(authority: Arc<Authority>, artifact: Artifact, config: Config) -> Result<Self> {
        tcc_ir::validate(&artifact, &EngineCaps::current())
            .map_err(|err| Error::new(format!("artifact: {err}")))?;
        let workers = config.workers.max(1);
        let listener = Listener::bind(&config.dir)?;
        let inner = Arc::new(Inner {
            authority,
            artifact,
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
            timings: Mutex::new(Timings::default()),
            workers: Mutex::new(Vec::new()),
            accept: Mutex::new(None),
        });
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
        if self.inner.accept_execution(execution_id)? {
            self.inner.enqueue(execution_id.to_string());
        }
        Ok(())
    }

    pub fn resume(&self, execution_id: &str) -> Result<()> {
        let started = Instant::now();
        let claimed =
            self.inner
                .authority
                .claim(execution_id, &self.inner.owner, self.inner.lease_ms)?;
        let engine = if let Some(bytes) = &claimed.continuation {
            let continuation =
                decode_continuation(bytes).map_err(|err| Error::new(format!("restore: {err}")))?;
            if continuation.artifact.hash != self.inner.artifact.envelope.artifact_hash {
                return Err(Error::new("artifact hash does not match the continuation"));
            }
            let mut engine =
                Engine::resume(self.inner.artifact.clone(), continuation, &self.inner.caps)
                    .map_err(|err| Error::new(err.to_string()))?;
            if claimed.timer_due {
                engine
                    .apply_host_response(HostResponse::TimerFired { branch: None })
                    .map_err(|err| Error::new(err.to_string()))?;
            }
            engine
        } else {
            Engine::start_with_args(
                self.inner.artifact.clone(),
                execution_id,
                &self.inner.caps,
                &[],
            )
            .map_err(|err| Error::new(err.to_string()))?
        };
        self.inner
            .timings
            .lock()
            .expect("timings")
            .restore
            .push(started.elapsed());
        self.inner.admit(
            execution_id.to_string(),
            engine,
            Memory {
                journal: claimed.journal,
                revision: claimed.revision,
                last_effect: None,
            },
        );
        Ok(())
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
                    let Ok((connection, reader)) = Connection::handshake(stream, &bearer) else {
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
                            match commands.accept_execution(&id) {
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
            drive::turn(drive::Turn {
                engine,
                memory: &mut memory,
                execution_id: id.clone(),
                owner_token: inner.owner.clone(),
                answer,
                confirm,
            })
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
    fn admit(&self, id: String, engine: Engine, memory: Memory) {
        {
            let mut queue = self.queue.lock().expect("queue");
            if queue.inflight >= self.cap {
                queue.backlog.push_back(Backlog { id, engine, memory });
                queue.backlogged = true;
                return;
            }
            queue.inflight += 1;
            queue.max_inflight = queue.max_inflight.max(queue.inflight);
        }
        self.launch(id, engine, memory);
    }

    fn launch(&self, id: String, engine: Engine, memory: Memory) {
        self.insert_slot(id.clone(), engine, memory);
        self.enqueue(id);
    }

    fn insert_slot(&self, id: String, engine: Engine, memory: Memory) {
        let slot = Arc::new(Slot {
            phase: Mutex::new(Phase::Runnable),
            engine: Mutex::new(Some(engine)),
            memory: Mutex::new(memory),
            answer: Mutex::new(None),
            confirm: Mutex::new(None),
            attempts: AtomicU32::new(0),
            timeouts: AtomicU32::new(0),
        });
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

    fn accept_execution(&self, execution_id: &str) -> Result<bool> {
        if self.slot(execution_id).is_some() || self.in_backlog(execution_id) {
            return Err(Error::new("execution id is already in use"));
        }
        self.authority
            .open(execution_id, &self.owner, self.lease_ms)
            .map_err(|err| {
                if err.to_string() == "execution already exists" {
                    Error::new("execution id is already in use")
                } else {
                    err
                }
            })?;
        let engine = Engine::start_with_args(self.artifact.clone(), execution_id, &self.caps, &[])
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
                });
                queue.backlogged = true;
                return Ok(false);
            }
            queue.inflight += 1;
            queue.max_inflight = queue.max_inflight.max(queue.inflight);
        }
        self.insert_slot(execution_id.to_string(), engine, memory);
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
        if !self.has_adapter(&last.key) {
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
        std::thread::spawn(move || {
            let started = Instant::now();
            let dispatched = inner.hub_dispatch(&id, &attempt_id, &last.key, &last.input);
            inner
                .timings
                .lock()
                .expect("timings")
                .socket
                .push(started.elapsed());
            let answer = match dispatched {
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
    ) -> std::result::Result<String, DispatchError> {
        let connections = self.connections.lock().expect("connections").clone();
        let hash = &self.artifact.envelope.artifact_hash;
        for connection in connections {
            if connection.alive()
                && connection.artifact_hash == *hash
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
        Err(DispatchError::Unavailable)
    }

    fn deliver_answer(&self, id: String, attempt_id: String, answer: Answer) {
        let Some(slot) = self.slot(&id) else {
            return;
        };
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
        let done = self.commits_done.load(Ordering::SeqCst);
        if self.fault_after_commits.is_some_and(|limit| done >= limit) {
            self.abandon(id, slot);
            return;
        }
        *slot.phase.lock().expect("phase") = Phase::WaitingForCommit;
        let inner = Arc::clone(self);
        let id = id.to_string();
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = inner.authority.commit(&commit);
            inner
                .timings
                .lock()
                .expect("timings")
                .commit
                .push(started.elapsed());
            inner.deliver_confirm(id, result);
        });
    }

    fn deliver_confirm(&self, id: String, result: Result<crate::authority::CommitResult>) {
        let Some(slot) = self.slot(&id) else {
            return;
        };
        let mut phase = slot.phase.lock().expect("phase");
        if !matches!(*phase, Phase::WaitingForCommit) {
            return;
        }
        match result {
            Ok(done) if done.owner.as_deref() == Some(self.owner.as_str()) => {
                self.commits_done.fetch_add(1, Ordering::SeqCst);
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
        *slot.phase.lock().expect("phase") = phase;
        *slot.engine.lock().expect("engine") = None;
        counter.fetch_add(1, Ordering::SeqCst);
        self.release_inflight();
    }

    fn has_adapter(&self, key: &str) -> bool {
        let connections = self.connections.lock().expect("connections");
        let hash = &self.artifact.envelope.artifact_hash;
        connections.iter().any(|connection| {
            connection.alive()
                && connection.artifact_hash == *hash
                && connection.keys.iter().any(|known| known == key)
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
        let key = {
            let memory = slot.memory.lock().expect("memory");
            memory.last_effect.as_ref().map(|effect| effect.key.clone())
        };
        let Some(key) = key else {
            return;
        };
        if !self.has_adapter(&key) {
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
        match self.restore_engine(id) {
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

    fn restore_engine(&self, execution_id: &str) -> Result<(Engine, Memory)> {
        let started = Instant::now();
        let claimed = self
            .authority
            .claim(execution_id, &self.owner, self.lease_ms)?;
        let engine = if let Some(bytes) = &claimed.continuation {
            let continuation =
                decode_continuation(bytes).map_err(|err| Error::new(format!("restore: {err}")))?;
            if continuation.artifact.hash != self.artifact.envelope.artifact_hash {
                return Err(Error::new("artifact hash does not match the continuation"));
            }
            let mut engine = Engine::resume(self.artifact.clone(), continuation, &self.caps)
                .map_err(|err| Error::new(err.to_string()))?;
            if claimed.timer_due {
                engine
                    .apply_host_response(HostResponse::TimerFired { branch: None })
                    .map_err(|err| Error::new(err.to_string()))?;
            }
            engine
        } else {
            Engine::start_with_args(self.artifact.clone(), execution_id, &self.caps, &[])
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
            self.launch(item.id, item.engine, item.memory);
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
    }
}
