use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex};

use tcc_core::Engine;

use crate::authority::canonical_keys;
use crate::drive::Memory;
use crate::error::{Error, Result};

use super::state::{Backlog, Partial, Phase, Slot};
use super::Inner;

impl Inner {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn admit(
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
    pub(super) fn launch(
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
    pub(super) fn insert_slot(
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
            cancel: AtomicBool::new(false),
            artifact_hash,
            handler_keys,
        });
        self.spawn_renewal(&id, &slot);
        self.slots.lock().expect("slots").insert(id, slot);
    }

    pub(super) fn enqueue(&self, id: String) {
        let mut queue = self.queue.lock().expect("queue");
        if queue.stop {
            return;
        }
        queue.runnable.push_back(id);
        self.queue_cv.notify_one();
    }

    pub(super) fn pop(&self) -> Option<String> {
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

    pub(super) fn slot(&self, id: &str) -> Option<Arc<Slot>> {
        self.slots.lock().expect("slots").get(id).cloned()
    }

    pub(super) fn accept_execution(
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

    pub(super) fn in_backlog(&self, id: &str) -> bool {
        self.queue
            .lock()
            .expect("queue")
            .backlog
            .iter()
            .any(|item| item.id == id)
    }
    pub(super) fn release_inflight(&self) {
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
}
