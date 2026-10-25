use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use tcc_core::{Engine, HostResponse};
use tcc_ir::Artifact;
use tcc_state::{decode_continuation, decode_value, ContinuationStatus, PendingOp, WaitKind};

use crate::authority::{canonical_keys, Claimed};
use crate::drive::Memory;
use crate::error::{Error, Result};

use super::state::{version_wait, Phase, Slot};
use super::Inner;

impl Inner {
    pub(super) fn has_adapter(
        &self,
        artifact_hash: &str,
        handler_keys: &[String],
        key: &str,
    ) -> bool {
        let connections = self.connections.lock().expect("connections");
        let pinned = canonical_keys(handler_keys);
        connections.iter().any(|connection| {
            connection.alive()
                && connection.artifact_hash == artifact_hash
                && canonical_keys(&connection.keys) == pinned
                && connection.keys.iter().any(|known| known == key)
        })
    }

    pub(super) fn has_registration(&self, artifact_hash: &str, handler_keys: &[String]) -> bool {
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

    pub(super) fn reconcile(self: &Arc<Self>) {
        let ids: Vec<String> = self.slots.lock().expect("slots").keys().cloned().collect();
        for id in ids {
            self.reconcile_one(&id);
        }
    }

    /// Resume only an execution that is waiting for this adapter's effect key.
    /// A suspended sleep or event stays suspended. A running engine is left in place.
    pub(super) fn reconcile_one(self: &Arc<Self>, id: &str) {
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

    pub(super) fn install_resumed(self: &Arc<Self>, id: &str, slot: &Slot) {
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

    pub(super) fn restore_engine(
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
            wake_claimed(&mut engine, &claimed)?;
            engine
        } else {
            let mut engine = Engine::start_with_args(artifact, execution_id, &self.caps, &[])
                .map_err(|err| Error::new(err.to_string()))?;
            if claimed.cancel_requested {
                wake_claimed(&mut engine, &claimed)?;
            }
            engine
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

    pub(super) fn artifact(&self, hash: &str) -> Result<Artifact> {
        self.artifacts
            .lock()
            .expect("artifacts")
            .get(hash)
            .cloned()
            .ok_or_else(|| Error::new("artifact is not loaded"))
    }

    pub(super) fn keys_for(&self, hash: &str) -> Vec<String> {
        self.connections
            .lock()
            .expect("connections")
            .iter()
            .find(|connection| connection.alive() && connection.artifact_hash == hash)
            .map(|connection| canonical_keys(&connection.keys))
            .unwrap_or_default()
    }

    pub(super) fn install_artifact(&self, hash: &str, artifact: Option<&str>) -> Result<()> {
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

    pub(super) fn resume_execution(&self, execution_id: &str) -> Result<()> {
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

    pub(super) fn cancel_execution(&self, execution_id: &str) -> Result<()> {
        self.authority.request_cancel(execution_id)?;
        let Some(slot) = self.slot(execution_id) else {
            return Ok(());
        };
        slot.cancel.store(true, Ordering::SeqCst);
        {
            let mut phase = slot.phase.lock().expect("phase");
            if !matches!(*phase, Phase::Suspended) {
                return Ok(());
            }
            *phase = Phase::Recovering;
        }
        let hash = slot.artifact_hash.clone();
        let keys = slot.handler_keys.clone();
        if let Err(err) = self.claim_and_admit(execution_id, &hash, &keys) {
            let mut phase = slot.phase.lock().expect("phase");
            if matches!(*phase, Phase::Recovering) {
                *phase = Phase::Suspended;
            }
            if err.to_string() == "execution is owned" {
                return Ok(());
            }
            return Err(err);
        }
        Ok(())
    }

    pub(super) fn deliver_event(
        &self,
        execution_id: &str,
        event_name: &str,
        wait_id: &str,
        payload: &str,
    ) -> Result<()> {
        let slot = self
            .slot(execution_id)
            .ok_or_else(|| Error::new("execution is not waiting"))?;
        {
            let phase = slot.phase.lock().expect("phase");
            if !matches!(*phase, Phase::Suspended) {
                return Err(Error::new("execution is not waiting"));
            }
        }
        let fence = slot.fence.load(Ordering::SeqCst);
        self.authority.accept_event(
            execution_id,
            &self.owner,
            fence,
            event_name,
            wait_id,
            payload,
        )?;
        {
            let mut phase = slot.phase.lock().expect("phase");
            if !matches!(*phase, Phase::Suspended) {
                return Ok(());
            }
            *phase = Phase::Recovering;
        }
        let hash = slot.artifact_hash.clone();
        let keys = slot.handler_keys.clone();
        if let Err(err) = self.claim_and_admit(execution_id, &hash, &keys) {
            let mut phase = slot.phase.lock().expect("phase");
            if matches!(*phase, Phase::Recovering) {
                *phase = Phase::Suspended;
            }
            return Err(err);
        }
        Ok(())
    }

    pub(super) fn claim_and_admit(
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
            wake_claimed(&mut engine, &claimed)?;
            engine
        } else {
            let mut engine = Engine::start_with_args(artifact, execution_id, &self.caps, &[])
                .map_err(|err| Error::new(err.to_string()))?;
            if claimed.cancel_requested {
                wake_claimed(&mut engine, &claimed)?;
            }
            engine
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

    pub(super) fn resume_waiting(&self) {
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

    pub(super) fn note_waiting(&self, execution_id: &str, artifact_hash: &str) {
        let mut waiting = self.waiting.lock().expect("waiting");
        if waiting.iter().any(|item| item.execution_id == execution_id) {
            return;
        }
        waiting.push(version_wait(execution_id, artifact_hash));
    }

    pub(super) fn clear_waiting(&self, execution_id: &str) {
        self.waiting
            .lock()
            .expect("waiting")
            .retain(|item| item.execution_id != execution_id);
    }

    pub(super) fn park_for_adapter(self: &Arc<Self>, id: &str, slot: &Arc<Slot>) {
        {
            let mut phase = slot.phase.lock().expect("phase");
            if matches!(
                *phase,
                Phase::Suspended
                    | Phase::Completed
                    | Phase::Cancelled
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

    pub(super) fn note_failure(&self, id: &str, message: String) {
        self.failures
            .lock()
            .expect("failures")
            .push((id.to_string(), message));
    }
}

fn wake_claimed(engine: &mut Engine, claimed: &Claimed) -> Result<()> {
    if claimed.cancel_requested && !terminal_status(engine.continuation().status) {
        engine
            .apply_host_response(HostResponse::Cancel)
            .map_err(|err| Error::new(err.to_string()))?;
        return Ok(());
    }
    if let Some(event) = claimed.event.as_ref().filter(|event| {
        matches!(
            &engine.continuation().pending,
            Some(PendingOp::Wait {
                kind: WaitKind::Event {
                    wait_id,
                    event_name,
                    ..
                },
            }) if wait_id == &event.wait_id && event_name == &event.event
        )
    }) {
        let value = decode_value(event.payload.as_bytes())
            .map_err(|err| Error::new(format!("event payload: {err}")))?;
        engine
            .apply_host_response(HostResponse::EventPayload {
                value,
                branch: None,
            })
            .map_err(|err| Error::new(err.to_string()))?;
    } else if claimed.timer_due {
        engine
            .apply_host_response(HostResponse::TimerFired { branch: None })
            .map_err(|err| Error::new(err.to_string()))?;
    }
    Ok(())
}

fn terminal_status(status: ContinuationStatus) -> bool {
    matches!(
        status,
        ContinuationStatus::Completed | ContinuationStatus::Failed | ContinuationStatus::Cancelled
    )
}
