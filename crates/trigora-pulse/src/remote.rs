use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::TcpStream;
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

const RESTORE_IN_FLIGHT: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Upload {
    Snapshot,
    Delta,
    None,
}

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
    snapshot: Option<String>,
    deltas: usize,
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
    pub journal_bytes: u64,
    pub objects: u64,
    pub compactions: u64,
    pub revision: u64,
}

pub struct RemoteClient {
    endpoint: Endpoint,
    bound: usize,
    cache: Mutex<HashMap<String, CacheEntry>>,
    report: Mutex<Option<RemoteReport>>,
    owner_hint: Mutex<Option<String>>,
}

impl RemoteClient {
    pub fn new(
        base: impl Into<String>,
        workspace: impl Into<String>,
        token: impl Into<String>,
        bound: usize,
    ) -> Self {
        let base = base.into();
        let (host, port) = split_base(&base).unwrap_or_else(|_| ("invalid".into(), 0));
        Self {
            endpoint: Endpoint {
                host,
                port,
                workspace: workspace.into(),
                token: token.into(),
            },
            bound,
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
            bound: self.bound,
            cache: Mutex::new(HashMap::new()),
            report: Mutex::new(None),
            owner_hint: Mutex::new(None),
        }
    }

    pub fn commit(&self, commit: &Commit) -> Result<CommitResult> {
        self.remember(&commit.owner_token);
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
        self.endpoint.put(
            &commit.execution_id,
            &commit.owner_token,
            &digest,
            &commit.continuation,
        )?;
        let cached = self.prepare(&commit.execution_id, &commit.owner_token)?;
        let changed = changed_rows(&cached.journal, &commit.journal)?;
        let upload = journal_upload(
            cached.snapshot.is_some(),
            cached.deltas,
            !changed.is_empty(),
            self.bound,
        );
        let (journal, journal_bytes) = match upload {
            Upload::None => (None, 0),
            Upload::Snapshot => {
                let bytes = canonical_journal(commit.journal.values());
                let digest = authority::sha256(&bytes);
                self.endpoint
                    .put(&commit.execution_id, &commit.owner_token, &digest, &bytes)?;
                (Some(("snapshot", digest)), bytes.len() as u64)
            }
            Upload::Delta => {
                let bytes = canonical_journal(changed.iter());
                let digest = authority::sha256(&bytes);
                self.endpoint
                    .put(&commit.execution_id, &commit.owner_token, &digest, &bytes)?;
                (Some(("delta", digest)), bytes.len() as u64)
            }
        };
        let body = json!({
            "ownerToken": commit.owner_token,
            "fence": commit.fence,
            "expectedRevision": commit.expected_revision,
            "continuationDigest": digest,
            "identity": identity,
            "journal": journal.as_ref().map(|(kind, digest)| json!({"kind": kind, "digest": digest})),
            "timers": timers.iter().map(|timer| json!({"branch": timer.branch, "dueAt": timer.due_at})).collect::<Vec<_>>(),
            "waits": waits.iter().map(|wait| json!({"waitId": wait.wait_id, "event": wait.event})).collect::<Vec<_>>(),
        });
        let posted: Result<CommitResponse> = self.endpoint.post_json(
            &format!(
                "/v1/workspaces/{}/executions/{}/commits",
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
            journal_bytes,
            objects: response.objects,
            compactions: response.compactions,
            revision: response.revision,
        });
        self.cache.lock().expect("cache").insert(
            commit.execution_id.clone(),
            CacheEntry {
                journal: commit.journal.clone(),
                snapshot: Some(response.snapshot),
                deltas: response.deltas.len(),
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

    pub fn claim(&self, execution_id: &str, owner: &str, lease_ms: u64) -> Result<Claimed> {
        self.remember(owner);
        let manifest: ManifestResponse = self.endpoint.post_json(
            &format!(
                "/v1/workspaces/{}/executions/{execution_id}/claim",
                self.endpoint.workspace
            ),
            owner,
            &json!({ "leaseMs": lease_ms }),
        )?;
        self.claimed(execution_id, owner, manifest)
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
                "/v1/workspaces/{}/executions/{execution_id}/renew",
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
        let found: QueryResponse = self.endpoint.post_json(
            &format!(
                "/v1/workspaces/{}/executions/{execution_id}/reconcile",
                self.endpoint.workspace
            ),
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

    fn claimed(
        &self,
        execution_id: &str,
        owner: &str,
        manifest: ManifestResponse,
    ) -> Result<Claimed> {
        let fence = manifest.fence;
        let lease_until = manifest.lease_until;
        if manifest.revision == 0 || manifest.continuation_digest.is_empty() {
            return Ok(Claimed {
                revision: manifest.revision,
                fence,
                lease_until,
                continuation: None,
                journal: BTreeMap::new(),
                timers: Vec::new(),
                waits: Vec::new(),
                timer_due: false,
                status: ContinuationStatus::Runnable,
            });
        }
        let entry = self.fetch_entry(execution_id, owner)?;
        let status = entry.status;
        self.cache
            .lock()
            .expect("cache")
            .insert(execution_id.to_string(), entry.clone());
        Ok(Claimed {
            revision: entry.revision,
            fence,
            lease_until,
            continuation: Some(entry.continuation),
            journal: entry.journal,
            timers: entry.timers,
            waits: entry.waits,
            timer_due: false,
            status,
        })
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
                snapshot: None,
                deltas: 0,
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
        let manifest: ManifestResponse = self.endpoint.get_json(
            &format!(
                "/v1/workspaces/{}/executions/{execution_id}",
                self.endpoint.workspace
            ),
            owner,
        )?;
        if manifest.revision == 0 || manifest.continuation_digest.is_empty() {
            return Ok(CacheEntry {
                journal: BTreeMap::new(),
                snapshot: None,
                deltas: 0,
                revision: 0,
                continuation: Vec::new(),
                timers: Vec::new(),
                waits: Vec::new(),
                status: ContinuationStatus::Runnable,
                owner: manifest.owner,
                fence: manifest.fence,
                lease_until: manifest.lease_until,
            });
        }
        let continuation = self.endpoint.get_bytes(
            &format!(
                "/v1/workspaces/{}/executions/{execution_id}/objects/{}",
                self.endpoint.workspace, manifest.continuation_digest
            ),
            owner,
        )?;
        if authority::sha256(&continuation) != manifest.continuation_digest {
            return Err(Error::new("corrupt blob"));
        }
        let decoded = decode_continuation(&continuation)
            .map_err(|err| Error::new(format!("restore: {err}")))?;
        if decoded.execution_id != execution_id {
            return Err(Error::new("continuation execution id mismatch"));
        }
        let timers = timers_of(&decoded);
        let waits = waits_of(&decoded);
        if manifest_timers(&manifest.timers) != timers || manifest_waits(&manifest.waits) != waits {
            return Err(Error::new("inconsistent timer metadata"));
        }
        let mut digests = vec![manifest.snapshot.clone()];
        digests.extend(manifest.deltas.iter().cloned());
        let objects = self.endpoint.fetch_all(execution_id, owner, &digests)?;
        let mut journal = BTreeMap::new();
        for (digest, bytes) in digests.iter().zip(objects) {
            if authority::sha256(&bytes) != *digest {
                return Err(Error::new("corrupt blob"));
            }
            for row in parse_journal(&bytes)? {
                journal.insert(row.key.clone(), row);
            }
        }
        let identity = authority::commit_identity(
            manifest.revision,
            &manifest.continuation_digest,
            &journal,
            &timers,
            &waits,
        );
        if identity != manifest.identity {
            return Err(Error::new("commit conflict"));
        }
        Ok(CacheEntry {
            journal,
            snapshot: Some(manifest.snapshot),
            deltas: manifest.deltas.len(),
            revision: manifest.revision,
            continuation,
            timers,
            waits,
            status: decoded.status,
            owner: manifest.owner,
            fence: manifest.fence,
            lease_until: manifest.lease_until,
        })
    }
}

impl Endpoint {
    fn put(&self, execution: &str, owner: &str, digest: &str, bytes: &[u8]) -> Result<()> {
        let path = format!(
            "/v1/workspaces/{}/executions/{execution}/objects/{digest}",
            self.workspace
        );
        self.call("PUT", &path, owner, Some(bytes), "application/octet-stream")?;
        Ok(())
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

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str, owner: &str) -> Result<T> {
        let response = self.call("GET", path, owner, None, "application/json")?;
        serde_json::from_slice(&response.body).map_err(|err| Error::new(err.to_string()))
    }

    fn get_bytes(&self, path: &str, owner: &str) -> Result<Vec<u8>> {
        Ok(self
            .call("GET", path, owner, None, "application/octet-stream")?
            .body)
    }

    fn fetch_all(&self, execution: &str, owner: &str, digests: &[String]) -> Result<Vec<Vec<u8>>> {
        if digests.is_empty() {
            return Err(Error::new("missing object"));
        }
        let mut objects = vec![Vec::new(); digests.len()];
        let mut start = 0;
        while start < digests.len() {
            let end = (start + RESTORE_IN_FLIGHT).min(digests.len());
            let mut handles = Vec::new();
            for (offset, digest) in digests[start..end].iter().cloned().enumerate() {
                let endpoint = self.clone();
                let owner = owner.to_string();
                let execution = execution.to_string();
                handles.push(thread::spawn(move || {
                    let bytes = endpoint.get_bytes(
                        &format!(
                            "/v1/workspaces/{}/executions/{execution}/objects/{digest}",
                            endpoint.workspace
                        ),
                        &owner,
                    )?;
                    if authority::sha256(&bytes) != digest {
                        return Err(Error::new("corrupt blob"));
                    }
                    Ok((offset, bytes))
                }));
            }
            for handle in handles {
                let (offset, bytes) = handle.join().map_err(|_| Error::new("restore failed"))??;
                objects[start + offset] = bytes;
            }
            start = end;
        }
        Ok(objects)
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
        let started = Instant::now();
        let mut stream = TcpStream::connect((self.host.as_str(), self.port))
            .map_err(|err| Error::new(format!("authority connect: {err}")))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|err| Error::new(err.to_string()))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
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
        let (status, payload, server_timing) = parse_http(&response)?;
        Ok(RawHttp {
            status,
            body: payload,
            server_timing,
            client_ms: started.elapsed().as_secs_f64() * 1000.0,
        })
    }
}

struct RawHttp {
    status: u16,
    body: Vec<u8>,
    server_timing: String,
    client_ms: f64,
}

pub fn run_remote_measure() -> Result<()> {
    let url = env::var("TRIGORA_PULSE_AUTHORITY_URL")
        .map_err(|_| Error::new("TRIGORA_PULSE_AUTHORITY_URL is required"))?;
    let workspace = env::var("TRIGORA_PULSE_WORKSPACE_ID")
        .map_err(|_| Error::new("TRIGORA_PULSE_WORKSPACE_ID is required"))?;
    let token = env::var("TRIGORA_PULSE_WORKSPACE_TOKEN")
        .map_err(|_| Error::new("TRIGORA_PULSE_WORKSPACE_TOKEN is required"))?;
    let samples = env::var("TRIGORA_PULSE_SAMPLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5usize);
    let deltas: Vec<usize> = env::var("TRIGORA_PULSE_DELTAS")
        .unwrap_or_else(|_| "0,8".to_string())
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect();
    let keys = env::var("TRIGORA_PULSE_KEYS_FILE").unwrap_or_default();
    let client = RemoteClient::new(&url, &workspace, &token, 32);
    let mut runs = Vec::new();
    for delta in deltas {
        for sample in 0..samples {
            runs.push(client.measure_once(&workspace, delta, sample, &keys)?);
        }
    }
    println!(
        "{}",
        serde_json::json!({
            "transport": "remote.rs Endpoint, one TcpStream per request, Connection: close",
            "pattern": "continuation uploaded on every commit",
            "runs": runs,
        })
    );
    Ok(())
}

impl RemoteClient {
    fn measure_once(
        &self,
        workspace: &str,
        deltas: usize,
        sample: usize,
        keys: &str,
    ) -> Result<serde_json::Value> {
        let millis = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|err| Error::new(err.to_string()))?
            .as_millis();
        let execution = format!("measure-rust-{deltas}-{sample}-{millis}");
        let continuation = format!("continuation {execution} {}", "x".repeat(200));
        let continuation_bytes = continuation.into_bytes();
        let continuation_digest = authority::sha256(&continuation_bytes);
        let owner = "measure";
        let mut logical = BTreeMap::new();
        let mut requests = Vec::new();
        for index in 0..=deltas {
            if index > 0 {
                let key = format!("k{index}");
                logical.insert(
                    key.clone(),
                    JournalRow {
                        key: key.clone(),
                        idempotency_key: format!("{execution}:k{index}"),
                        status: "completed".to_string(),
                        input: String::new(),
                        result: index.to_string(),
                    },
                );
            }
            self.put_observed(
                &execution,
                owner,
                &continuation_digest,
                &continuation_bytes,
                workspace,
                keys,
                index,
                "put-continuation",
                &mut requests,
            )?;
            let journal_bytes = if index == 0 {
                canonical_journal(logical.values())
            } else {
                let row = logical.get(&format!("k{index}")).expect("row");
                canonical_journal(std::iter::once(row))
            };
            let digest = authority::sha256(&journal_bytes);
            self.put_observed(
                &execution,
                owner,
                &digest,
                &journal_bytes,
                workspace,
                keys,
                index,
                "put-journal",
                &mut requests,
            )?;
            let identity = authority::commit_identity(
                (index as u64) + 1,
                &continuation_digest,
                &logical,
                &[],
                &[],
            );
            let body = json!({
                "ownerToken": owner,
                "fence": 1,
                "expectedRevision": index,
                "continuationDigest": continuation_digest,
                "identity": identity,
                "journal": { "kind": if index == 0 { "snapshot" } else { "delta" }, "digest": digest },
                "timers": [],
                "waits": [],
                "leaseMs": 600_000,
            });
            let path = format!(
                "/v1/workspaces/{}/executions/{execution}/commits",
                self.endpoint.workspace
            );
            let bytes = serde_json::to_vec(&body).map_err(|err| Error::new(err.to_string()))?;
            let response =
                self.endpoint
                    .exchange("POST", &path, owner, Some(&bytes), "application/json")?;
            requests.push(observed(index, "commit", &response));
            if !(200..300).contains(&response.status) {
                return Err(Error::new(format!("authority status {}", response.status)));
            }
        }
        Ok(json!({
            "deltas": deltas,
            "sample": sample,
            "execution": execution,
            "requests": requests,
        }))
    }

    fn put_observed(
        &self,
        execution: &str,
        owner: &str,
        digest: &str,
        bytes: &[u8],
        workspace: &str,
        keys: &str,
        index: usize,
        op: &str,
        requests: &mut Vec<serde_json::Value>,
    ) -> Result<()> {
        let path = format!(
            "/v1/workspaces/{}/executions/{execution}/objects/{digest}",
            self.endpoint.workspace
        );
        let response =
            self.endpoint
                .exchange("PUT", &path, owner, Some(bytes), "application/octet-stream")?;
        requests.push(observed(index, op, &response));
        if !(200..300).contains(&response.status) {
            return Err(Error::new(format!("authority status {}", response.status)));
        }
        if !keys.is_empty() {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(keys)
                .map_err(|err| Error::new(err.to_string()))?;
            writeln!(file, "{workspace}/{execution}/{digest}\t{}", bytes.len())
                .map_err(|err| Error::new(err.to_string()))?;
        }
        Ok(())
    }
}

fn observed(index: usize, op: &str, response: &RawHttp) -> serde_json::Value {
    let authority = metric_ms(&response.server_timing, "worker")
        .or_else(|| metric_ms(&response.server_timing, "object"));
    let gap = match authority {
        Some(ms) => response.client_ms - ms,
        None => response.client_ms,
    };
    json!({
        "op": op,
        "index": index,
        "clientMs": response.client_ms,
        "gapMs": gap,
        "status": response.status,
        "serverTiming": response.server_timing,
    })
}

fn metric_ms(header: &str, name: &str) -> Option<f64> {
    let needle = format!("{name};dur=");
    let rest = header.split(&needle).nth(1)?;
    let number: String = rest
        .chars()
        .take_while(|char| char.is_ascii_digit() || *char == '.')
        .collect();
    number.parse().ok()
}

fn journal_upload(has_snapshot: bool, deltas: usize, changed: bool, bound: usize) -> Upload {
    if !has_snapshot || (changed && deltas >= bound) {
        Upload::Snapshot
    } else if changed {
        Upload::Delta
    } else {
        Upload::None
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

fn canonical_journal<'a>(rows: impl IntoIterator<Item = &'a JournalRow>) -> Vec<u8> {
    let mut rows: Vec<&JournalRow> = rows.into_iter().collect();
    rows.sort_by(|left, right| left.key.cmp(&right.key));
    let mut out = b"{\"rows\":[".to_vec();
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        out.extend(
            format!(
                "{{\"idempotencyKey\":{},\"input\":{},\"key\":{},\"result\":{},\"status\":{}}}",
                serde_json::to_string(&row.idempotency_key).expect("string"),
                serde_json::to_string(&row.input).expect("string"),
                serde_json::to_string(&row.key).expect("string"),
                serde_json::to_string(&row.result).expect("string"),
                serde_json::to_string(&row.status).expect("string"),
            )
            .into_bytes(),
        );
    }
    out.extend(b"]}");
    out
}

fn parse_journal(bytes: &[u8]) -> Result<Vec<JournalRow>> {
    let parsed: JournalFile =
        serde_json::from_slice(bytes).map_err(|_| Error::new("journal object is invalid"))?;
    let rows: Vec<JournalRow> = parsed
        .rows
        .into_iter()
        .map(|row| JournalRow {
            key: row.key,
            idempotency_key: row.idempotency_key,
            status: row.status,
            input: row.input,
            result: row.result,
        })
        .collect();
    if canonical_journal(rows.iter()) != bytes {
        return Err(Error::new("journal object is invalid"));
    }
    Ok(rows)
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

fn parse_http(bytes: &[u8]) -> Result<(u16, Vec<u8>, String)> {
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
    let mut server_timing = String::new();
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
            if name.eq_ignore_ascii_case("server-timing") {
                server_timing = value.trim().to_string();
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
    Ok((code, body, server_timing))
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
struct JournalFile {
    rows: Vec<WireRow>,
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
struct CommitResponse {
    revision: u64,
    owner: String,
    fence: u64,
    lease_until: u64,
    idempotent: bool,
    identity: String,
    validation_us: u64,
    continuation_bytes: u64,
    objects: u64,
    compactions: u64,
    snapshot: String,
    deltas: Vec<String>,
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
#[serde(rename_all = "camelCase")]
struct ManifestResponse {
    revision: u64,
    owner: String,
    fence: u64,
    lease_until: u64,
    identity: String,
    continuation_digest: String,
    snapshot: String,
    deltas: Vec<String>,
    timers: Vec<TimerJson>,
    waits: Vec<WaitJson>,
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
    let mut child = Command::new("node")
        .arg("--experimental-strip-types")
        .arg(&script)
        .env("PULSE_TEST_HOOKS", "1")
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
                    let client = RemoteClient::new(
                        format!("http://127.0.0.1:{port}"),
                        "ws",
                        "workspace-token",
                        32,
                    );
                    return TestAuthority { child, client };
                }
            }
            Err(err) => panic!("read authority port: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_journal_matches_the_authority() {
        let row = JournalRow {
            key: "k".into(),
            idempotency_key: "exec:k".into(),
            status: "completed".into(),
            input: String::new(),
            result: "1".into(),
        };
        let bytes = canonical_journal(std::iter::once(&row));
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"rows":[{"idempotencyKey":"exec:k","input":"","key":"k","result":"1","status":"completed"}]}"#
        );
    }

    #[test]
    fn a_full_delta_chain_becomes_a_snapshot() {
        assert_eq!(journal_upload(true, 32, true, 32), Upload::Snapshot);
        assert_eq!(journal_upload(true, 31, true, 32), Upload::Delta);
        assert_eq!(journal_upload(true, 31, false, 32), Upload::None);
        assert_eq!(journal_upload(false, 0, false, 32), Upload::Snapshot);
    }
}
