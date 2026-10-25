mod dispatch;
mod lease;
mod recovery;
mod scheduler;
mod state;

pub(crate) use recovery::apply_child_outcome;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tcc_ir::{Artifact, EngineCaps};

use crate::authority::Authority;
use crate::drive;
use crate::error::{Error, Result};
use crate::socket::{Connection, HostCommand, Listener};

use state::{Inner, Phase, QueueState};

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
            cancelled: AtomicUsize::new(0),
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

    /// Store an artifact that `start_child` may run. The bytes are validated
    /// before they are kept, the same way `open` validates the first artifact.
    pub fn install(&self, artifact: Artifact) -> Result<()> {
        tcc_ir::validate(&artifact, &EngineCaps::current())
            .map_err(|err| Error::new(format!("artifact: {err}")))?;
        let hash = artifact.envelope.artifact_hash.clone();
        self.inner
            .artifacts
            .lock()
            .expect("artifacts")
            .insert(hash, artifact);
        Ok(())
    }

    /// Initialize the created child named by `artifact_hash`, which must already
    /// be installed. The first continuation is committed before any effect runs.
    pub fn start_child(&self, execution_id: &str, artifact_hash: &str) -> Result<()> {
        self.inner.start_created_child(execution_id, artifact_hash)
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

    /// Accept one payload for a committed wait, then drive. The engine sees the
    /// payload only after the authority has stored it.
    pub fn send(
        &self,
        execution_id: &str,
        event_name: &str,
        wait_id: &str,
        payload: &str,
    ) -> Result<()> {
        self.inner
            .deliver_event(execution_id, event_name, wait_id, payload)
    }

    /// Record a cancellation request, then finalize it when this process can
    /// observe the execution without taking someone else's lease.
    pub fn cancel(&self, execution_id: &str) -> Result<()> {
        self.inner.cancel_execution(execution_id)
    }

    pub fn waiting(&self) -> Vec<VersionWait> {
        let mut waiting = self.inner.waiting.lock().expect("waiting").clone();
        let slots = self.inner.slots.lock().expect("slots");
        for (id, slot) in slots.iter() {
            if matches!(*slot.phase.lock().expect("phase"), Phase::WaitingForAdapter)
                && !waiting.iter().any(|item| item.execution_id == *id)
            {
                waiting.push(state::version_wait(id, &slot.artifact_hash));
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

    pub fn cancelled(&self) -> usize {
        self.inner.cancelled.load(Ordering::SeqCst)
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
        let cancel = slot.cancel.load(Ordering::SeqCst);
        let child_ready = slot.child_ready.swap(false, Ordering::SeqCst);
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
                cancel,
                child_ready,
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
}
