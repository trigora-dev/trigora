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
use crate::error::{Error, Result};

use protocol::{QueryResponse, RenewResponse};
use sqlite::CacheEntry;
use transport::{parse_origin, Endpoint, Route};

pub(super) const HARD_CONTINUATION_MAX: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub enum RemoteMode {
    Local,
    Gateway {
        base: String,
        token: String,
    },
    Authority {
        url: String,
        workspace: String,
        token: String,
    },
}

pub fn select_remote(
    token: Option<&str>,
    api_base: Option<&str>,
    authority_url: Option<&str>,
    workspace_id: Option<&str>,
    workspace_token: Option<&str>,
) -> Result<RemoteMode> {
    let token = nonempty(token);
    let authority_url = nonempty(authority_url);
    let workspace_id = nonempty(workspace_id);
    let workspace_token = nonempty(workspace_token);
    let legacy = [authority_url, workspace_id, workspace_token];
    let legacy_count = legacy.iter().filter(|value| value.is_some()).count();
    if token.is_some() && legacy_count > 0 {
        return Err(Error::new(
            "TRIGORA_TOKEN cannot be combined with TRIGORA_PULSE_AUTHORITY_URL, TRIGORA_PULSE_WORKSPACE_ID, or TRIGORA_PULSE_WORKSPACE_TOKEN",
        ));
    }
    if let Some(token) = token {
        let base = nonempty(api_base).unwrap_or("https://api.trigora.dev");
        return Ok(RemoteMode::Gateway {
            base: base.to_string(),
            token: token.to_string(),
        });
    }
    if legacy_count == 0 {
        return Ok(RemoteMode::Local);
    }
    if legacy_count != 3 {
        return Err(Error::new(
            "TRIGORA_PULSE_AUTHORITY_URL, TRIGORA_PULSE_WORKSPACE_ID, and TRIGORA_PULSE_WORKSPACE_TOKEN are required together",
        ));
    }
    Ok(RemoteMode::Authority {
        url: authority_url.unwrap().to_string(),
        workspace: workspace_id.unwrap().to_string(),
        token: workspace_token.unwrap().to_string(),
    })
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub struct RemoteClient {
    pub(super) endpoint: Endpoint,
    pub(super) max_continuation: usize,
    pub(super) cache: Mutex<HashMap<String, CacheEntry>>,
    pub(super) owner_hint: Mutex<Option<String>>,
}

impl RemoteClient {
    pub fn sqlite(
        base: impl Into<String>,
        workspace: impl Into<String>,
        token: impl Into<String>,
        max_continuation: usize,
    ) -> Result<Self> {
        Self::open(
            base.into(),
            Route::Authority {
                workspace: workspace.into(),
            },
            token.into(),
            max_continuation,
        )
    }

    pub fn gateway(
        base: impl Into<String>,
        token: impl Into<String>,
        max_continuation: usize,
    ) -> Result<Self> {
        Self::open(base.into(), Route::Gateway, token.into(), max_continuation)
    }

    fn open(base: String, route: Route, token: String, max_continuation: usize) -> Result<Self> {
        let origin = parse_origin(&base)?;
        Ok(Self {
            endpoint: Endpoint::from_parts(origin, route, token),
            max_continuation,
            cache: Mutex::new(HashMap::new()),
            owner_hint: Mutex::new(None),
        })
    }

    pub fn fresh(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            max_continuation: self.max_continuation,
            cache: Mutex::new(HashMap::new()),
            owner_hint: Mutex::new(None),
        }
    }

    pub fn commit(&self, commit: &Commit) -> Result<CommitResult> {
        self.commit_sqlite(commit)
    }

    pub fn request_cancel(&self, execution_id: &str) -> Result<crate::authority::CancelReceipt> {
        self.request_cancel_sqlite(execution_id)
    }

    pub fn record_invoke(
        &self,
        execution_id: &str,
        owner: &str,
        fence: u64,
        spec: &crate::authority::InvokeSpec,
    ) -> Result<crate::authority::InvokeReceipt> {
        self.record_invoke_sqlite(execution_id, owner, fence, spec)
    }

    pub fn ensure_child(
        &self,
        parent_id: &str,
        offered: Option<&crate::authority::InvokeSpec>,
    ) -> Result<crate::authority::InvokeReceipt> {
        self.ensure_child_sqlite(parent_id, offered)
    }

    pub fn start_created(
        &self,
        execution_id: &str,
        owner: &str,
        lease_ms: u64,
        artifact_hash: &str,
        handler_keys: &[String],
    ) -> Result<crate::authority::StartedChild> {
        self.start_created_sqlite(execution_id, owner, lease_ms, artifact_hash, handler_keys)
    }

    pub fn created_child(&self, execution_id: &str) -> Result<crate::authority::CreatedChild> {
        self.created_child_sqlite(execution_id)
    }

    pub fn accept_event(
        &self,
        execution_id: &str,
        owner: &str,
        fence: u64,
        event_name: &str,
        wait_id: &str,
        payload: &str,
    ) -> Result<crate::authority::EventReceipt> {
        self.accept_event_sqlite(execution_id, owner, fence, event_name, wait_id, payload)
    }

    pub(crate) fn set_clock(&self, at: u64) -> Result<()> {
        let body = serde_json::to_vec(&json!({ "at": at }))
            .map_err(|err| crate::error::Error::new(err.to_string()))?;
        self.endpoint.call(
            "POST",
            "/v1/test/clock",
            "A",
            Some(&body),
            "application/json",
        )?;
        Ok(())
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
            &self.endpoint.execution_path(execution_id, "sql-renew"),
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
        let path = self.endpoint.execution_path(execution_id, "sql-reconcile");
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
        self.view_entry(self.fetch_entry(execution_id, &owner)?)
    }

    pub fn connections_opened(&self) -> usize {
        self.endpoint.connections_opened()
    }

    pub fn view_child(&self, execution_id: &str, parent_id: &str) -> Result<View> {
        self.view_entry(self.fetch_child(execution_id, parent_id)?)
    }

    fn view_entry(&self, entry: CacheEntry) -> Result<View> {
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

#[cfg(test)]
mod remote_mode_tests {
    use super::{select_remote, RemoteMode};

    #[test]
    fn a_token_and_a_legacy_authority_are_rejected() {
        let err = select_remote(
            Some("customer"),
            None,
            Some("http://127.0.0.1:9"),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("cannot be combined"));
    }

    #[test]
    fn an_incomplete_legacy_authority_is_rejected() {
        let err = select_remote(None, None, Some("http://127.0.0.1:9"), None, None).unwrap_err();
        assert!(err.to_string().contains("required together"));
    }

    #[test]
    fn a_token_selects_the_gateway() {
        let mode = select_remote(
            Some(" customer "),
            Some(" https://edge.example/base/ "),
            None,
            None,
            None,
        )
        .unwrap();
        match mode {
            RemoteMode::Gateway { base, token } => {
                assert_eq!(base, "https://edge.example/base/");
                assert_eq!(token, "customer");
            }
            _ => panic!("gateway"),
        }
    }
}
