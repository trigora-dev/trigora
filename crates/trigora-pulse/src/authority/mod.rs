mod commit;
mod identity;
mod invoke;
mod ownership;
mod signals;
mod state;
mod types;

use std::sync::{Arc, Mutex};

pub use types::{
    AcceptedEvent, CancelReceipt, Claimed, Commit, CommitQuery, CommitResult, CreatedChild,
    EventReceipt, InvokeIntent, InvokeReceipt, InvokeSpec, JournalRow, Renewed, StartedChild,
    TimerRow, VersionPin, View, WaitRow,
};

pub(crate) use identity::{canonical_keys, commit_identity, sha256, timers_of, waits_of};

use crate::error::{Error, Result};

/// In-memory stand-in for the coordination object. It outlives any one host.
/// A remote client, when set, is the source of truth and this map is only the
/// local bookkeeping `open` uses before the first commit.
#[derive(Clone)]
pub struct Authority {
    inner: Arc<Mutex<state::State>>,
    remote: Option<Arc<crate::remote::RemoteClient>>,
}

impl Authority {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(state::State::new())),
            remote: None,
        }
    }

    pub fn with_remote(mut self, remote: crate::remote::RemoteClient) -> Self {
        self.remote = Some(Arc::new(remote));
        self
    }

    pub fn now(&self) -> u64 {
        self.lock().clock
    }

    pub fn advance_clock(&self, to: u64) -> Result<()> {
        state::advance_clock(&mut self.lock(), to)?;
        if let Some(remote) = &self.remote {
            remote.set_clock(to)?;
        }
        Ok(())
    }

    pub fn has_remote(&self) -> bool {
        self.remote.is_some()
    }

    pub fn open(&self, execution_id: &str, owner_token: &str, lease_ms: u64) -> Result<Claimed> {
        if let Some(record) = self.lock().executions.get(execution_id) {
            if record.created.is_some() && record.continuation.is_none() {
                return Err(Error::new("child is not initialized"));
            }
            return Err(Error::new("execution already exists"));
        }
        if let Some(remote) = &self.remote {
            let claimed = remote.claim(execution_id, owner_token, lease_ms, "", &[])?;
            state::insert(
                &mut self.lock(),
                execution_id,
                owner_token,
                claimed.fence,
                claimed.lease_until,
            )?;
            return Ok(claimed);
        }
        ownership::open(&mut self.lock(), execution_id, owner_token, lease_ms)
    }

    pub fn commit(&self, commit: &Commit) -> Result<CommitResult> {
        if let Some(remote) = &self.remote {
            return remote.commit(commit);
        }
        let prepared = commit::prepare(commit)?;
        commit::apply(&mut self.lock(), commit, prepared)
    }

    /// Store one payload for the committed wait. The revision does not move.
    /// The same digest is an idempotent retry. A different digest conflicts.
    pub fn accept_event(
        &self,
        execution_id: &str,
        owner_token: &str,
        fence: u64,
        event_name: &str,
        wait_id: &str,
        payload: &str,
    ) -> Result<EventReceipt> {
        if event_name.is_empty() || wait_id.is_empty() || payload.is_empty() {
            return Err(Error::new("event payload is invalid"));
        }
        if let Some(remote) = &self.remote {
            return remote.accept_event(
                execution_id,
                owner_token,
                fence,
                event_name,
                wait_id,
                payload,
            );
        }
        let digest = sha256(payload.as_bytes());
        signals::accept(
            &mut self.lock(),
            execution_id,
            owner_token,
            fence,
            event_name,
            wait_id,
            payload,
            digest,
        )
    }

    /// Store one invoke intent for a child id. The revision does not move. The
    /// same child id, program, and arguments are idempotent. A different program
    /// or argument vector for that child conflicts and leaves the first intent.
    pub fn record_invoke(
        &self,
        execution_id: &str,
        owner_token: &str,
        fence: u64,
        spec: &InvokeSpec,
    ) -> Result<InvokeReceipt> {
        if let Some(remote) = &self.remote {
            return remote.record_invoke(execution_id, owner_token, fence, spec);
        }
        invoke::record(&mut self.lock(), execution_id, owner_token, fence, spec)
    }

    /// Create the child from the stored intent. This does not require the
    /// parent's current lease. `offered`, when set, must match that intent.
    pub fn ensure_child(
        &self,
        parent_id: &str,
        offered: Option<&InvokeSpec>,
    ) -> Result<InvokeReceipt> {
        if let Some(remote) = &self.remote {
            return remote.ensure_child(parent_id, offered);
        }
        invoke::ensure(&mut self.lock(), parent_id, offered)
    }

    /// Admit a created child. The stored arguments are the only ones returned.
    /// This does not write a continuation and does not delete the child link.
    pub fn start_created(
        &self,
        execution_id: &str,
        owner_token: &str,
        lease_ms: u64,
        artifact_hash: &str,
        handler_keys: &[String],
    ) -> Result<StartedChild> {
        if let Some(remote) = &self.remote {
            return remote.start_created(
                execution_id,
                owner_token,
                lease_ms,
                artifact_hash,
                handler_keys,
            );
        }
        invoke::start(
            &mut self.lock(),
            execution_id,
            owner_token,
            lease_ms,
            artifact_hash,
            handler_keys,
        )
    }

    pub fn created_child(&self, execution_id: &str) -> Result<CreatedChild> {
        if let Some(remote) = &self.remote {
            return remote.created_child(execution_id);
        }
        invoke::created(&self.lock(), execution_id)
    }

    /// Record a cancellation request. The caller does not need the owner fence,
    /// and the revision does not move.
    pub fn request_cancel(&self, execution_id: &str) -> Result<CancelReceipt> {
        if let Some(remote) = &self.remote {
            return remote.request_cancel(execution_id);
        }
        signals::request_cancel(&mut self.lock(), execution_id)
    }

    pub fn renew(
        &self,
        execution_id: &str,
        owner_token: &str,
        fence: u64,
        lease_ms: u64,
    ) -> Result<Renewed> {
        if let Some(remote) = &self.remote {
            return remote.renew(execution_id, owner_token, fence, lease_ms);
        }
        ownership::renew(&mut self.lock(), execution_id, owner_token, fence, lease_ms)
    }

    /// Whether this logical commit was accepted. Does not grant authority and
    /// does not change the record.
    pub fn query(&self, execution_id: &str, revision: u64, identity: &str) -> Result<CommitQuery> {
        if let Some(remote) = &self.remote {
            return remote.reconcile(execution_id, revision, identity);
        }
        ownership::query(&self.lock(), execution_id, revision, identity)
    }

    pub fn view(&self, execution_id: &str) -> Result<View> {
        if let Some(remote) = &self.remote {
            return remote.view(execution_id);
        }
        ownership::view(&self.lock(), execution_id)
    }

    /// Read a child's committed continuation. The caller is the parent execution,
    /// not the child's owner, so this does not require the child's lease.
    pub fn view_child(&self, child_id: &str, parent_id: &str) -> Result<View> {
        if let Some(remote) = &self.remote {
            return remote.view_child(child_id, parent_id);
        }
        let link = invoke::created(&self.lock(), child_id)?;
        if link.parent_execution_id != parent_id {
            return Err(Error::new("invalid owner"));
        }
        ownership::view(&self.lock(), child_id)
    }

    /// Take or renew ownership. A live lease held by someone else is rejected.
    /// Does not read a continuation except by returning the committed one.
    pub fn pin(&self, execution_id: &str) -> Result<VersionPin> {
        if let Some(remote) = &self.remote {
            return remote.pin(execution_id);
        }
        ownership::pin(&self.lock(), execution_id)
    }

    /// Take or renew ownership when the offered artifact and handler keys match
    /// the pin. A mismatch leaves owner, fence, and lease unchanged.
    pub fn claim(
        &self,
        execution_id: &str,
        owner_token: &str,
        lease_ms: u64,
        artifact_hash: &str,
        handler_keys: &[String],
    ) -> Result<Claimed> {
        if let Some(remote) = &self.remote {
            return remote.claim(
                execution_id,
                owner_token,
                lease_ms,
                artifact_hash,
                handler_keys,
            );
        }
        ownership::claim(
            &mut self.lock(),
            execution_id,
            owner_token,
            lease_ms,
            artifact_hash,
            handler_keys,
        )
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, state::State> {
        self.inner.lock().expect("authority lock")
    }
}

impl Default for Authority {
    fn default() -> Self {
        Self::new()
    }
}
