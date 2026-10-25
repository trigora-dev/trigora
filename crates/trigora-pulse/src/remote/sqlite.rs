use std::collections::BTreeMap;

use serde_json::json;
use tcc_state::{decode_continuation, ContinuationStatus};

use super::protocol::{
    checkpoint_frame, manifest_timers, manifest_waits, split_frame, SqliteClaimResponse,
    SqliteCommitResponse, SqliteRestoreHeader, VersionBody,
};
use super::{RemoteClient, RemoteReport};
use crate::authority::{
    self, timers_of, waits_of, Claimed, Commit, CommitResult, JournalRow, TimerRow, WaitRow,
};
use crate::error::{Error, Result};

#[derive(Clone)]
pub(crate) struct CacheEntry {
    pub(super) journal: BTreeMap<String, JournalRow>,
    pub(super) revision: u64,
    pub(super) continuation: Vec<u8>,
    pub(super) timers: Vec<TimerRow>,
    pub(super) waits: Vec<WaitRow>,
    pub(super) status: ContinuationStatus,
    pub(super) owner: String,
    pub(super) fence: u64,
    pub(super) lease_until: u64,
}

impl RemoteClient {
    pub fn pin(&self, execution_id: &str) -> Result<crate::authority::VersionPin> {
        let found: VersionBody = self.endpoint.call_json(
            "GET",
            &format!(
                "/v1/workspaces/{}/executions/{execution_id}/sql-version",
                self.endpoint.workspace
            ),
        )?;
        Ok(crate::authority::VersionPin {
            revision: found.revision,
            artifact_hash: found.artifact_hash,
            handler_keys: found.handler_keys,
        })
    }

    pub(super) fn commit_sqlite(&self, commit: &Commit) -> Result<CommitResult> {
        self.remember(&commit.owner_token);
        if commit.continuation.len() > self.max_continuation {
            return Err(checkpoint_too_large(self.max_continuation));
        }
        let continuation = decode_continuation(&commit.continuation)
            .map_err(|err| Error::new(format!("continuation decode failed: {err}")))?;
        if continuation.execution_id != commit.execution_id {
            return Err(Error::new("continuation execution id mismatch"));
        }
        let next = commit
            .expected_revision
            .checked_add(1)
            .ok_or_else(|| Error::new("revision overflow"))?;
        if continuation.revision != next {
            return Err(Error::new(
                "continuation revision does not match the commit",
            ));
        }
        let timers = timers_of(&continuation);
        let waits = waits_of(&continuation);
        if let Some(given) = &commit.timers {
            if given != &timers {
                return Err(Error::new("inconsistent timer metadata"));
            }
        }
        if let Some(given) = &commit.waits {
            if given != &waits {
                return Err(Error::new("inconsistent wait metadata"));
            }
        }
        let digest = authority::sha256(&commit.continuation);
        let identity = authority::commit_identity(next, &digest, &commit.journal, &timers, &waits);
        let cached = self.prepare(&commit.execution_id, &commit.owner_token)?;
        let changed = changed_rows(&cached.journal, &commit.journal)?;
        let unchanged =
            !cached.continuation.is_empty() && authority::sha256(&cached.continuation) == digest;
        let raw: &[u8] = if unchanged { &[] } else { &commit.continuation };
        let header = json!({
            "ownerToken": commit.owner_token,
            "fence": commit.fence,
            "expectedRevision": commit.expected_revision,
            "continuationDigest": digest,
            "continuationBytes": raw.len(),
            "identity": identity,
            "journal": changed.iter().map(|row| json!({
                "key": row.key,
                "idempotencyKey": row.idempotency_key,
                "status": row.status,
                "input": row.input,
                "result": row.result,
            })).collect::<Vec<_>>(),
            "timers": timers.iter().map(|timer| json!({"branch": timer.branch, "dueAt": timer.due_at})).collect::<Vec<_>>(),
            "waits": waits.iter().map(|wait| json!({"waitId": wait.wait_id, "event": wait.event})).collect::<Vec<_>>(),
            "artifactHash": commit.artifact_hash,
            "handlerKeys": commit.handler_keys,
        });
        let body = checkpoint_frame(&header, raw)?;
        let posted: Result<SqliteCommitResponse> = self.endpoint.post_bytes(
            &format!(
                "/v1/workspaces/{}/executions/{}/sql-commits",
                self.endpoint.workspace, commit.execution_id
            ),
            &commit.owner_token,
            &body,
        );
        let response = match posted {
            Ok(response) => response,
            Err(err) if err.to_string().starts_with("authority ") => {
                if let Ok(found) = self.reconcile(&commit.execution_id, next, &identity) {
                    if found.accepted
                        && found.owner.as_deref() == Some(commit.owner_token.as_str())
                        && found.fence == commit.fence
                    {
                        return Ok(CommitResult {
                            revision: found.revision,
                            owner: found.owner,
                            fence: found.fence,
                            lease_until: found.lease_until,
                            idempotent: true,
                        });
                    }
                }
                return Err(err);
            }
            Err(err) => return Err(err),
        };
        if response.identity != identity || response.revision != next {
            return Err(Error::new("commit conflict"));
        }
        *self.report.lock().expect("report") = Some(RemoteReport {
            validation_us: response.validation_us,
            continuation_bytes: response.continuation_bytes,
            chunks: response.chunk_count,
            revision: response.revision,
        });
        self.cache.lock().expect("cache").insert(
            commit.execution_id.clone(),
            CacheEntry {
                journal: commit.journal.clone(),
                revision: response.revision,
                continuation: commit.continuation.clone(),
                timers,
                waits,
                status: continuation.status,
                owner: response.owner.clone(),
                fence: response.fence,
                lease_until: response.lease_until,
            },
        );
        Ok(CommitResult {
            revision: response.revision,
            owner: Some(response.owner),
            fence: response.fence,
            lease_until: response.lease_until,
            idempotent: response.idempotent,
        })
    }

    pub(super) fn claim_sqlite(
        &self,
        execution_id: &str,
        owner: &str,
        lease_ms: u64,
        artifact_hash: &str,
        handler_keys: &[String],
    ) -> Result<Claimed> {
        self.remember(owner);
        let claimed: SqliteClaimResponse = self.endpoint.post_json(
            &format!(
                "/v1/workspaces/{}/executions/{execution_id}/sql-claim",
                self.endpoint.workspace
            ),
            owner,
            &json!({
                "leaseMs": lease_ms,
                "artifactHash": artifact_hash,
                "handlerKeys": handler_keys,
            }),
        )?;
        if claimed.revision == 0 || claimed.continuation_digest.is_empty() {
            return Ok(Claimed {
                revision: claimed.revision,
                fence: claimed.fence,
                lease_until: claimed.lease_until,
                continuation: None,
                journal: BTreeMap::new(),
                timers: Vec::new(),
                waits: Vec::new(),
                timer_due: false,
                status: ContinuationStatus::Runnable,
            });
        }
        let entry = self.fetch_sqlite(execution_id, owner)?;
        let status = entry.status;
        self.cache
            .lock()
            .expect("cache")
            .insert(execution_id.to_string(), entry.clone());
        Ok(Claimed {
            revision: entry.revision,
            fence: claimed.fence,
            lease_until: claimed.lease_until,
            continuation: Some(entry.continuation),
            journal: entry.journal,
            timers: entry.timers,
            waits: entry.waits,
            timer_due: false,
            status,
        })
    }

    pub(super) fn fetch_sqlite(&self, execution_id: &str, owner: &str) -> Result<CacheEntry> {
        let response = self.endpoint.call(
            "GET",
            &format!(
                "/v1/workspaces/{}/executions/{execution_id}/sql-restore",
                self.endpoint.workspace
            ),
            owner,
            None,
            "application/trigora-checkpoint",
        )?;
        let (header, continuation) = split_frame(&response.body)?;
        let header: SqliteRestoreHeader =
            serde_json::from_slice(header).map_err(|err| Error::new(err.to_string()))?;
        if header.revision == 0 || header.continuation_digest.is_empty() {
            return Ok(CacheEntry {
                journal: BTreeMap::new(),
                revision: header.revision,
                continuation: Vec::new(),
                timers: Vec::new(),
                waits: Vec::new(),
                status: ContinuationStatus::Runnable,
                owner: header.owner,
                fence: header.fence,
                lease_until: header.lease_until,
            });
        }
        if continuation.len() as u64 != header.continuation_bytes {
            return Err(Error::new("corrupt blob"));
        }
        if authority::sha256(continuation) != header.continuation_digest {
            return Err(Error::new("corrupt blob"));
        }
        let decoded = decode_continuation(continuation)
            .map_err(|err| Error::new(format!("restore: {err}")))?;
        if decoded.execution_id != execution_id {
            return Err(Error::new("continuation execution id mismatch"));
        }
        let timers = timers_of(&decoded);
        let waits = waits_of(&decoded);
        if manifest_timers(&header.timers) != timers || manifest_waits(&header.waits) != waits {
            return Err(Error::new("inconsistent timer metadata"));
        }
        let mut journal = BTreeMap::new();
        for row in header.journal {
            journal.insert(
                row.key.clone(),
                JournalRow {
                    key: row.key,
                    idempotency_key: row.idempotency_key,
                    status: row.status,
                    input: row.input,
                    result: row.result,
                },
            );
        }
        let identity = authority::commit_identity(
            header.revision,
            &header.continuation_digest,
            &journal,
            &timers,
            &waits,
        );
        if identity != header.identity {
            return Err(Error::new("commit conflict"));
        }
        Ok(CacheEntry {
            journal,
            revision: header.revision,
            continuation: continuation.to_vec(),
            timers,
            waits,
            status: decoded.status,
            owner: header.owner,
            fence: header.fence,
            lease_until: header.lease_until,
        })
    }

    pub(super) fn remember(&self, owner: &str) {
        *self.owner_hint.lock().expect("owner") = Some(owner.to_string());
    }

    pub(super) fn prepare(&self, execution_id: &str, owner: &str) -> Result<CacheEntry> {
        if let Some(entry) = self.cache.lock().expect("cache").get(execution_id).cloned() {
            return Ok(entry);
        }
        match self.fetch_entry(execution_id, owner) {
            Ok(entry) => Ok(entry),
            Err(err) if err.to_string() == "unknown execution" => Ok(CacheEntry {
                journal: BTreeMap::new(),
                revision: 0,
                continuation: Vec::new(),
                timers: Vec::new(),
                waits: Vec::new(),
                status: ContinuationStatus::Runnable,
                owner: owner.to_string(),
                fence: 1,
                lease_until: 0,
            }),
            Err(err) => Err(err),
        }
    }

    pub(super) fn fetch_entry(&self, execution_id: &str, owner: &str) -> Result<CacheEntry> {
        self.fetch_sqlite(execution_id, owner)
    }
}

fn changed_rows(
    old: &BTreeMap<String, JournalRow>,
    new: &BTreeMap<String, JournalRow>,
) -> Result<Vec<JournalRow>> {
    for key in old.keys() {
        if !new.contains_key(key) {
            return Err(Error::new("deleted journal key"));
        }
    }
    Ok(new
        .iter()
        .filter(|(key, row)| old.get(*key) != Some(*row))
        .map(|(_, row)| row.clone())
        .collect())
}

fn checkpoint_too_large(limit: usize) -> Error {
    let label = if limit.is_multiple_of(1024 * 1024) {
        format!("{} MiB", limit / (1024 * 1024))
    } else {
        format!("{limit} bytes")
    };
    Error::new(format!(
        "TRIGORA_CHECKPOINT_TOO_LARGE\nContinuation exceeds the configured {label} limit. Store large data externally and retain a reference in the durable program state."
    ))
}
