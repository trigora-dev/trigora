mod measure;
mod protocol;
mod sqlite;
mod transport;

#[cfg(test)]
mod harness;

#[cfg(test)]
pub(crate) use harness::serve;

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::json;

use crate::authority::{Claimed, Commit, CommitResult, View};
use crate::error::Result;

use protocol::{QueryResponse, RenewResponse};
use sqlite::CacheEntry;
use transport::{split_base, Endpoint};

pub use measure::run_remote_measure;

pub(super) const HARD_CONTINUATION_MAX: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RemoteReport {
    pub validation_us: u64,
    pub continuation_bytes: u64,
    pub chunks: u64,
    pub revision: u64,
}

pub struct RemoteClient {
    pub(super) endpoint: Endpoint,
    pub(super) max_continuation: usize,
    pub(super) cache: Mutex<HashMap<String, CacheEntry>>,
    pub(super) report: Mutex<Option<RemoteReport>>,
    pub(super) owner_hint: Mutex<Option<String>>,
}

impl RemoteClient {
    pub fn sqlite(
        base: impl Into<String>,
        workspace: impl Into<String>,
        token: impl Into<String>,
        max_continuation: usize,
    ) -> Self {
        let base = base.into();
        let (host, port) = split_base(&base).unwrap_or_else(|_| ("invalid".into(), 0));
        Self::build(host, port, workspace.into(), token.into(), max_continuation)
    }

    fn build(
        host: String,
        port: u16,
        workspace: String,
        token: String,
        max_continuation: usize,
    ) -> Self {
        Self {
            endpoint: Endpoint::open(host, port, workspace, token),
            max_continuation,
            cache: Mutex::new(HashMap::new()),
            report: Mutex::new(None),
            owner_hint: Mutex::new(None),
        }
    }

    pub fn last_report(&self) -> Option<RemoteReport> {
        self.report.lock().expect("report").clone()
    }

    pub fn fresh(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            max_continuation: self.max_continuation,
            cache: Mutex::new(HashMap::new()),
            report: Mutex::new(None),
            owner_hint: Mutex::new(None),
        }
    }

    pub fn commit(&self, commit: &Commit) -> Result<CommitResult> {
        self.commit_sqlite(commit)
    }

    pub fn claim(
        &self,
        execution_id: &str,
        owner: &str,
        lease_ms: u64,
        artifact_hash: &str,
        handler_keys: &[String],
    ) -> Result<Claimed> {
        self.claim_sqlite(execution_id, owner, lease_ms, artifact_hash, handler_keys)
    }

    pub fn renew(
        &self,
        execution_id: &str,
        owner: &str,
        fence: u64,
        lease_ms: u64,
    ) -> Result<crate::authority::Renewed> {
        let renewed: RenewResponse = self.endpoint.post_json(
            &format!(
                "/v1/workspaces/{}/executions/{execution_id}/sql-renew",
                self.endpoint.workspace
            ),
            owner,
            &json!({ "fence": fence, "leaseMs": lease_ms }),
        )?;
        Ok(crate::authority::Renewed {
            owner: renewed.owner,
            fence: renewed.fence,
            lease_until: renewed.lease_until,
        })
    }

    pub fn reconcile(
        &self,
        execution_id: &str,
        revision: u64,
        identity: &str,
    ) -> Result<crate::authority::CommitQuery> {
        let owner = self
            .owner_hint
            .lock()
            .expect("owner")
            .clone()
            .unwrap_or_default();
        let path = format!(
            "/v1/workspaces/{}/executions/{execution_id}/sql-reconcile",
            self.endpoint.workspace
        );
        let found: QueryResponse = self.endpoint.post_json(
            &path,
            &owner,
            &json!({ "revision": revision, "identity": identity }),
        )?;
        Ok(crate::authority::CommitQuery {
            accepted: found.accepted,
            revision: found.revision,
            identity: found.identity,
            owner: Some(found.owner),
            fence: found.fence,
            lease_until: found.lease_until,
        })
    }

    pub fn view(&self, execution_id: &str) -> Result<View> {
        let owner = self
            .owner_hint
            .lock()
            .expect("owner")
            .clone()
            .ok_or_else(|| crate::error::Error::new("unknown owner"))?;
        let entry = self.fetch_entry(execution_id, &owner)?;
        Ok(View {
            revision: entry.revision,
            owner: Some(entry.owner),
            fence: entry.fence,
            lease_until: entry.lease_until,
            continuation: if entry.revision == 0 {
                None
            } else {
                Some(entry.continuation)
            },
            journal: entry.journal,
            timers: entry.timers,
            waits: entry.waits,
            status: entry.status,
            artifact_hash: String::new(),
            handler_keys: Vec::new(),
        })
    }
}
