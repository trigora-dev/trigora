use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::authority::canonical_keys;
use crate::drive::{self, Answer, Step};
use crate::error::Result;
use crate::socket::{DispatchError, TransportSample};

use super::state::{Phase, Slot};
use super::{Dispatch, Inner, Span};

impl Inner {
    pub(super) fn finish(self: &Arc<Self>, id: &str, slot: &Arc<Slot>, step: Step) {
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

    pub(super) fn dispatch_effect(
        self: &Arc<Self>,
        id: &str,
        slot: &Arc<Slot>,
        from_timeout: bool,
    ) {
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

    pub(super) fn hub_dispatch(
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

    pub(super) fn deliver_answer(&self, id: String, attempt_id: String, answer: Answer) {
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

    pub(super) fn begin_commit(
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

    pub(super) fn note_cost(&self, slot: &Slot, cost: drive::Cost) {
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

    pub(super) fn note_transport(
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

    pub(super) fn record_span(&self, id: &str, slot: &Slot, commit: Duration) {
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

    pub(super) fn deliver_confirm(
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

    pub(super) fn settle(&self, slot: &Slot, phase: Phase, counter: &AtomicUsize) {
        slot.live.store(false, Ordering::SeqCst);
        *slot.phase.lock().expect("phase") = phase;
        *slot.engine.lock().expect("engine") = None;
        counter.fetch_add(1, Ordering::SeqCst);
        self.release_inflight();
    }
}
