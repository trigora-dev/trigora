use std::collections::{BTreeMap, HashMap};
use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::SystemTime;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;
use tcc_state::{decode_continuation, ContinuationStatus};

use crate::authority::{
    self, timers_of, waits_of, Claimed, Commit, CommitResult, JournalRow, TimerRow, View, WaitRow,
};
use crate::error::{Error, Result};

#[derive(Clone)]
struct Endpoint {
    host: String,
    port: u16,
    workspace: String,
    token: String,
}

#[derive(Clone)]
struct CacheEntry {
    journal: BTreeMap<String, JournalRow>,
    revision: u64,
    continuation: Vec<u8>,
    timers: Vec<TimerRow>,
    waits: Vec<WaitRow>,
    status: ContinuationStatus,
    owner: String,
    fence: u64,
    lease_until: u64,
}

#[derive(Debug, Clone)]
pub struct RemoteReport {
    pub validation_us: u64,
    pub continuation_bytes: u64,
    pub chunks: u64,
    pub revision: u64,
}

const HARD_CONTINUATION_MAX: usize = 16 * 1024 * 1024;

pub struct RemoteClient {
    endpoint: Endpoint,
    max_continuation: usize,
    cache: Mutex<HashMap<String, CacheEntry>>,
    report: Mutex<Option<RemoteReport>>,
    owner_hint: Mutex<Option<String>>,
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
            endpoint: Endpoint {
                host,
                port,
                workspace,
                token,
            },
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

    fn commit_sqlite(&self, commit: &Commit) -> Result<CommitResult> {
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

    fn claim_sqlite(
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

    fn fetch_sqlite(&self, execution_id: &str, owner: &str) -> Result<CacheEntry> {
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

    fn measure_pair(&self, execution: &str, size: usize, keys: usize) -> Result<serde_json::Value> {
        let continuation = vec![b'x'; size];
        let digest = authority::sha256(&continuation);
        let journal = measure_journal(keys);
        let identity = authority::commit_identity(1, &digest, &journal, &[], &[]);
        let rows: Vec<JournalRow> = journal.values().cloned().collect();
        let rss_before = rss_kb();
        let (first, rss_peak) = sample_rss(|| {
            let started = Instant::now();
            let first =
                self.measure_post(execution, 0, &digest, &identity, &rows, Some(&continuation))?;
            Ok::<_, Error>((first, started.elapsed().as_secs_f64() * 1000.0))
        });
        let (first, first_ms) = first?;
        let rss_after = rss_kb();
        let mut later_journal = journal.clone();
        let extra = JournalRow {
            key: "extra".into(),
            idempotency_key: "exec:extra".into(),
            status: "completed".into(),
            input: String::new(),
            result: "1".into(),
        };
        later_journal.insert(extra.key.clone(), extra.clone());
        let later_identity = authority::commit_identity(2, &digest, &later_journal, &[], &[]);
        let started = Instant::now();
        let later = self.measure_post(execution, 1, &digest, &later_identity, &[extra], None)?;
        let later_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let restored = self.fresh().measure_restore(execution)?;
        let restore_ms = started.elapsed().as_secs_f64() * 1000.0;
        if restored != digest {
            return Err(Error::new("corrupt blob"));
        }
        Ok(json!({
            "execution": execution,
            "size": size,
            "keys": keys,
            "firstMs": first_ms,
            "laterMs": later_ms,
            "restoreMs": restore_ms,
            "chunkCount": first.chunk_count,
            "chunksWritten": first.chunks_written,
            "laterChunksWritten": later.chunks_written,
            "rowsRead": first.rows_read,
            "rowsWritten": first.rows_written,
            "laterRowsWritten": later.rows_written,
            "bytesRewritten": first.bytes_rewritten,
            "laterBytesRewritten": later.bytes_rewritten,
            "bufferedBytes": first.buffered_bytes,
            "validationUs": first.validation_us,
            "laterValidationUs": later.validation_us,
            "rssBeforeKb": rss_before,
            "rssPeakKb": rss_peak,
            "rssAfterKb": rss_after,
        }))
    }

    fn measure_post(
        &self,
        execution: &str,
        expected: u64,
        digest: &str,
        identity: &str,
        journal: &[JournalRow],
        continuation: Option<&[u8]>,
    ) -> Result<SqliteCommitResponse> {
        let raw = continuation.unwrap_or(&[]);
        let header = json!({
            "ownerToken": "measure",
            "fence": 1,
            "expectedRevision": expected,
            "continuationDigest": digest,
            "continuationBytes": raw.len(),
            "identity": identity,
            "journal": journal.iter().map(|row| json!({
                "key": row.key,
                "idempotencyKey": row.idempotency_key,
                "status": row.status,
                "input": row.input,
                "result": row.result,
            })).collect::<Vec<_>>(),
            "timers": [],
            "waits": [],
        });
        let body = checkpoint_frame(&header, raw)?;
        self.endpoint.post_bytes(
            &format!(
                "/v1/workspaces/{}/executions/{execution}/sql-commits",
                self.endpoint.workspace
            ),
            "measure",
            &body,
        )
    }

    fn measure_restore(&self, execution: &str) -> Result<String> {
        Ok(self.measure_restore_report(execution)?.continuation_digest)
    }

    fn measure_restore_report(&self, execution: &str) -> Result<SqliteRestoreHeader> {
        let response = self.endpoint.call(
            "GET",
            &format!(
                "/v1/workspaces/{}/executions/{execution}/sql-restore",
                self.endpoint.workspace
            ),
            "measure",
            None,
            "application/trigora-checkpoint",
        )?;
        let (header, continuation) = split_frame(&response.body)?;
        let header: SqliteRestoreHeader =
            serde_json::from_slice(header).map_err(|err| Error::new(err.to_string()))?;
        if continuation.len() as u64 != header.continuation_bytes
            || authority::sha256(continuation) != header.continuation_digest
        {
            return Err(Error::new("corrupt blob"));
        }
        Ok(header)
    }

    pub fn view(&self, execution_id: &str) -> Result<View> {
        let owner = self
            .owner_hint
            .lock()
            .expect("owner")
            .clone()
            .ok_or_else(|| Error::new("unknown owner"))?;
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

    fn remember(&self, owner: &str) {
        *self.owner_hint.lock().expect("owner") = Some(owner.to_string());
    }

    fn prepare(&self, execution_id: &str, owner: &str) -> Result<CacheEntry> {
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

    fn fetch_entry(&self, execution_id: &str, owner: &str) -> Result<CacheEntry> {
        self.fetch_sqlite(execution_id, owner)
    }
}

impl Endpoint {
    fn call_json<T: for<'de> Deserialize<'de>>(&self, method: &str, path: &str) -> Result<T> {
        let response = self.call(method, path, "", None, "application/json")?;
        serde_json::from_slice(&response.body).map_err(|err| Error::new(err.to_string()))
    }

    fn post_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        owner: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let bytes = serde_json::to_vec(body).map_err(|err| Error::new(err.to_string()))?;
        let response = self.call("POST", path, owner, Some(&bytes), "application/json")?;
        serde_json::from_slice(&response.body).map_err(|err| Error::new(err.to_string()))
    }

    fn post_bytes<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        owner: &str,
        body: &[u8],
    ) -> Result<T> {
        let response = self.call(
            "POST",
            path,
            owner,
            Some(body),
            "application/trigora-checkpoint",
        )?;
        serde_json::from_slice(&response.body).map_err(|err| Error::new(err.to_string()))
    }

    fn call(
        &self,
        method: &str,
        path: &str,
        owner: &str,
        body: Option<&[u8]>,
        content_type: &str,
    ) -> Result<RawHttp> {
        let response = self.exchange(method, path, owner, body, content_type)?;
        if !(200..300).contains(&response.status) {
            let message = String::from_utf8_lossy(&response.body);
            let message = message.trim();
            if message.is_empty() {
                return Err(Error::new(format!("authority status {}", response.status)));
            }
            return Err(Error::new(message.to_string()));
        }
        Ok(response)
    }

    fn exchange(
        &self,
        method: &str,
        path: &str,
        owner: &str,
        body: Option<&[u8]>,
        content_type: &str,
    ) -> Result<RawHttp> {
        if self.port == 0 {
            return Err(Error::new(
                "the Pulse authority client speaks HTTP/1.1; terminate TLS at the deployment edge",
            ));
        }
        let mut stream = TcpStream::connect((self.host.as_str(), self.port))
            .map_err(|err| Error::new(format!("authority connect: {err}")))?;
        let timeout = if body.is_some_and(|bytes| bytes.len() > 1024 * 1024) {
            Duration::from_secs(120)
        } else {
            Duration::from_secs(30)
        };
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|err| Error::new(err.to_string()))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|err| Error::new(err.to_string()))?;
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: Bearer {}\r\nX-Pulse-Owner: {owner}\r\nConnection: close\r\n",
            self.host, self.port, self.token
        );
        if let Some(body) = body {
            request.push_str(&format!(
                "Content-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            ));
            stream
                .write_all(request.as_bytes())
                .and_then(|_| stream.write_all(body))
                .and_then(|_| stream.flush())
                .map_err(|err| Error::new(format!("authority write: {err}")))?;
        } else {
            request.push_str("\r\n");
            stream
                .write_all(request.as_bytes())
                .and_then(|_| stream.flush())
                .map_err(|err| Error::new(format!("authority write: {err}")))?;
        }
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .map_err(|err| Error::new(format!("authority read: {err}")))?;
        let (status, payload) = parse_http(&response)?;
        Ok(RawHttp {
            status,
            body: payload,
        })
    }
}

struct RawHttp {
    status: u16,
    body: Vec<u8>,
}

pub fn run_remote_measure() -> Result<()> {
    run_sqlite_measure()
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

fn manifest_timers(timers: &[TimerJson]) -> Vec<TimerRow> {
    timers
        .iter()
        .map(|timer| TimerRow {
            branch: timer.branch.clone(),
            due_at: timer.due_at,
        })
        .collect()
}

fn manifest_waits(waits: &[WaitJson]) -> Vec<WaitRow> {
    waits
        .iter()
        .map(|wait| WaitRow {
            wait_id: wait.wait_id.clone(),
            event: wait.event.clone(),
        })
        .collect()
}

fn run_sqlite_measure() -> Result<()> {
    let url = env::var("TRIGORA_PULSE_AUTHORITY_URL")
        .map_err(|_| Error::new("TRIGORA_PULSE_AUTHORITY_URL is required"))?;
    let workspace = env::var("TRIGORA_PULSE_WORKSPACE_ID")
        .map_err(|_| Error::new("TRIGORA_PULSE_WORKSPACE_ID is required"))?;
    let token = env::var("TRIGORA_PULSE_WORKSPACE_TOKEN")
        .map_err(|_| Error::new("TRIGORA_PULSE_WORKSPACE_TOKEN is required"))?;
    let max = env::var("TRIGORA_PULSE_MAX_CONTINUATION_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(HARD_CONTINUATION_MAX);
    let client = RemoteClient::sqlite(&url, &workspace, &token, max);
    if let Ok(execution) = env::var("TRIGORA_PULSE_RESTORE_EXECUTION") {
        let (restored, rss_peak) = sample_rss(|| {
            let started = Instant::now();
            let restored = client.measure_restore_report(&execution)?;
            Ok::<_, Error>((restored, started.elapsed().as_secs_f64() * 1000.0))
        });
        let (restored, restore_ms) = restored?;
        println!(
            "{}",
            serde_json::json!({
                "restoreExecution": execution,
                "restoreMs": restore_ms,
                "rssPeakKb": rss_peak,
                "rssKb": rss_kb(),
                "revision": restored.revision,
                "owner": restored.owner,
                "fence": restored.fence,
                "digest": restored.continuation_digest,
                "bytes": restored.continuation_bytes,
                "journal": restored.journal.len(),
                "completed": restored.journal.iter().filter(|row| row.status == "completed").count(),
            })
        );
        return Ok(());
    }
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|err| Error::new(err.to_string()))?
        .as_millis();
    let mut small = Vec::new();
    for index in 0..8 {
        let execution = format!("p3e-{stamp}-s-{index}");
        let report = client.measure_pair(&execution, 4096, 1)?;
        emit_measure("small", &report);
        small.push(report);
    }
    let mut overlap = Vec::new();
    let mut handles = Vec::new();
    for index in 0..8 {
        let execution = format!("p3e-{stamp}-o-{index}");
        let worker = client.fresh();
        handles.push(thread::spawn(move || {
            worker.measure_pair(&execution, 4096, 1)
        }));
    }
    for handle in handles {
        let report = handle.join().map_err(|_| Error::new("measure thread"))??;
        emit_measure("overlap", &report);
        overlap.push(report);
    }
    let mut sized = Vec::new();
    let mut restart_execution = String::new();
    for (label, size, samples) in [
        ("1MiB", 1024 * 1024, 3usize),
        ("4MiB", 4 * 1024 * 1024, 3),
        ("16MiB", HARD_CONTINUATION_MAX, 1),
    ] {
        for index in 0..samples {
            let execution = format!("p3e-{stamp}-{label}-{index}");
            if label == "4MiB" && index == 0 {
                restart_execution = execution.clone();
            }
            let report = client.measure_pair(&execution, size, 1)?;
            emit_measure(label, &report);
            sized.push(report);
        }
    }
    let growth = format!("p3e-{stamp}-keys");
    let growth_report = client.measure_pair(&growth, 64, 2000)?;
    emit_measure("growth", &growth_report);
    println!(
        "{}",
        serde_json::json!({
            "transport": "one HTTP request, binary frame, SQLite chunks",
            "rssKb": rss_kb(),
            "small": small,
            "overlap": overlap,
            "sized": sized,
            "growth": growth_report,
            "restartExecution": restart_execution,
        })
    );
    let _ = std::io::stdout().flush();
    Ok(())
}

fn emit_measure(kind: &str, report: &serde_json::Value) {
    println!("{}", serde_json::json!({ "kind": kind, "report": report }));
    let _ = std::io::stdout().flush();
}

fn measure_journal(keys: usize) -> BTreeMap<String, JournalRow> {
    let mut journal = BTreeMap::new();
    for index in 0..keys {
        let key = format!("k{index:04}");
        journal.insert(
            key.clone(),
            JournalRow {
                key,
                idempotency_key: format!("exec:{index}"),
                status: "completed".into(),
                input: String::new(),
                result: "1".into(),
            },
        );
    }
    journal
}

fn sample_rss<T>(work: impl FnOnce() -> T) -> (T, u64) {
    let peak = Arc::new(AtomicU64::new(rss_kb()));
    let stop = Arc::new(AtomicBool::new(false));
    let watched = Arc::clone(&peak);
    let stopping = Arc::clone(&stop);
    let sampler = thread::spawn(move || {
        while !stopping.load(Ordering::Relaxed) {
            watched.fetch_max(rss_kb(), Ordering::Relaxed);
            thread::sleep(Duration::from_millis(20));
        }
    });
    let value = work();
    stop.store(true, Ordering::Relaxed);
    let _ = sampler.join();
    peak.fetch_max(rss_kb(), Ordering::Relaxed);
    (value, peak.load(Ordering::Relaxed))
}

fn rss_kb() -> u64 {
    let Ok(output) = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
    else {
        return 0;
    };
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
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

fn checkpoint_frame(header: &serde_json::Value, continuation: &[u8]) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(header).map_err(|err| Error::new(err.to_string()))?;
    let len = u32::try_from(json.len()).map_err(|_| Error::new("checkpoint frame is invalid"))?;
    let mut frame = Vec::with_capacity(4 + json.len() + continuation.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&json);
    frame.extend_from_slice(continuation);
    Ok(frame)
}

fn split_frame(bytes: &[u8]) -> Result<(&[u8], &[u8])> {
    if bytes.len() < 4 {
        return Err(Error::new("checkpoint frame is invalid"));
    }
    let len = u32::from_be_bytes(bytes[..4].try_into().expect("4 bytes")) as usize;
    if bytes.len() < 4 + len {
        return Err(Error::new("checkpoint frame is invalid"));
    }
    Ok((&bytes[4..4 + len], &bytes[4 + len..]))
}

fn split_base(base: &str) -> Result<(String, u16)> {
    let rest = base.strip_prefix("http://").ok_or_else(|| {
        Error::new(
            "the Pulse authority client speaks HTTP/1.1; terminate TLS at the deployment edge",
        )
    })?;
    let (host, port) = rest
        .split_once(':')
        .ok_or_else(|| Error::new("authority URL is missing a port"))?;
    if host.is_empty() || host.contains('/') {
        return Err(Error::new("authority URL is invalid"));
    }
    let port = port
        .trim_end_matches('/')
        .parse()
        .map_err(|_| Error::new("authority URL is invalid"))?;
    Ok((host.to_string(), port))
}

fn parse_http(bytes: &[u8]) -> Result<(u16, Vec<u8>)> {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| Error::new("authority response is truncated"))?;
    let header = std::str::from_utf8(&bytes[..split]).map_err(|err| Error::new(err.to_string()))?;
    let mut lines = header.lines();
    let status = lines
        .next()
        .ok_or_else(|| Error::new("authority response is truncated"))?;
    let code = status
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| Error::new("authority response is truncated"))?
        .parse()
        .map_err(|_| Error::new("authority response is truncated"))?;
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| Error::new("authority response is truncated"))?,
                );
            }
            if name.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
    }
    let body = &bytes[split + 4..];
    let body = if chunked {
        decode_chunks(body)?
    } else if let Some(length) = length {
        body.get(..length)
            .ok_or_else(|| Error::new("authority response is truncated"))?
            .to_vec()
    } else {
        body.to_vec()
    };
    Ok((code, body))
}

fn decode_chunks(mut bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = bytes
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| Error::new("authority response is truncated"))?;
        let size = std::str::from_utf8(&bytes[..line_end])
            .map_err(|err| Error::new(err.to_string()))?
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let size = usize::from_str_radix(size, 16)
            .map_err(|_| Error::new("authority response is truncated"))?;
        bytes = &bytes[line_end + 2..];
        if size == 0 {
            break;
        }
        if bytes.len() < size + 2 {
            return Err(Error::new("authority response is truncated"));
        }
        out.extend_from_slice(&bytes[..size]);
        bytes = &bytes[size + 2..];
    }
    Ok(out)
}

#[derive(Deserialize)]
struct WireRow {
    key: String,
    #[serde(rename = "idempotencyKey")]
    idempotency_key: String,
    status: String,
    input: String,
    result: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SqliteCommitResponse {
    revision: u64,
    owner: String,
    fence: u64,
    lease_until: u64,
    idempotent: bool,
    identity: String,
    validation_us: u64,
    continuation_bytes: u64,
    chunk_count: u64,
    #[serde(default)]
    chunks_written: u64,
    #[serde(default)]
    rows_read: u64,
    #[serde(default)]
    rows_written: u64,
    #[serde(default)]
    bytes_rewritten: u64,
    #[serde(default)]
    buffered_bytes: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionBody {
    revision: u64,
    artifact_hash: String,
    handler_keys: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SqliteClaimResponse {
    revision: u64,
    fence: u64,
    lease_until: u64,
    continuation_digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SqliteRestoreHeader {
    revision: u64,
    owner: String,
    fence: u64,
    lease_until: u64,
    identity: String,
    continuation_digest: String,
    continuation_bytes: u64,
    journal: Vec<WireRow>,
    timers: Vec<TimerJson>,
    waits: Vec<WaitJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenewResponse {
    owner: String,
    fence: u64,
    lease_until: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryResponse {
    accepted: bool,
    revision: u64,
    identity: String,
    owner: String,
    fence: u64,
    lease_until: u64,
}

#[derive(Deserialize)]
struct TimerJson {
    branch: String,
    #[serde(rename = "dueAt")]
    due_at: u64,
}

#[derive(Deserialize)]
struct WaitJson {
    #[serde(rename = "waitId")]
    wait_id: String,
    event: String,
}

#[cfg(test)]
pub(crate) struct TestAuthority {
    child: std::process::Child,
    client: RemoteClient,
}

#[cfg(test)]
impl Drop for TestAuthority {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
impl RemoteClient {
    pub(crate) fn test_hook(&self, path: &str, execution: &str) -> Result<()> {
        let body = serde_json::to_vec(&json!({ "execution": execution }))
            .map_err(|err| Error::new(err.to_string()))?;
        self.endpoint
            .call("POST", path, "A", Some(&body), "application/json")?;
        Ok(())
    }

    pub(crate) fn test_renewals(&self, execution: &str) -> Result<u64> {
        let response = self.endpoint.call(
            "GET",
            &format!("/v1/test/renewals/{execution}"),
            "A",
            None,
            "text/plain",
        )?;
        String::from_utf8_lossy(&response.body)
            .trim()
            .parse()
            .map_err(|err| Error::new(format!("renewal count: {err}")))
    }
}

#[cfg(test)]
impl TestAuthority {
    pub(crate) fn client(&self) -> RemoteClient {
        self.client.fresh()
    }
}

#[cfg(test)]
pub(crate) fn serve() -> TestAuthority {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    use std::sync::Arc;

    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../trigora-platform/apps/pulse-authority/src/serve.ts");
    let mut command = Command::new("node");
    command
        .arg("--experimental-strip-types")
        .arg(&script)
        .env("PULSE_TEST_HOOKS", "1");
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("start pulse authority: {err}"));
    let stderr = child.stderr.take().expect("stderr");
    let errors = Arc::new(Mutex::new(String::new()));
    let logged = Arc::clone(&errors);
    thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stderr);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            logged.lock().expect("stderr").push_str(&line);
            line.clear();
        }
    });
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!(
                "pulse authority did not listen: {}",
                errors.lock().expect("stderr")
            );
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                let _ = child.wait();
                panic!("pulse authority exited: {}", errors.lock().expect("stderr"));
            }
            Ok(_) => {
                if let Some(port) = line.trim().strip_prefix("listening ") {
                    let url = format!("http://127.0.0.1:{port}");
                    let client =
                        RemoteClient::sqlite(url, "ws", "workspace-token", HARD_CONTINUATION_MAX);
                    return TestAuthority { child, client };
                }
            }
            Err(err) => panic!("read authority port: {err}"),
        }
    }
}
