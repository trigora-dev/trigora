use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

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
            "expectedRevision": commit.expected_revision,
            "continuationDigest": digest,
            "identity": identity,
            "journal": journal.as_ref().map(|(kind, digest)| json!({"kind": kind, "digest": digest})),
            "timers": timers.iter().map(|timer| json!({"branch": timer.branch, "dueAt": timer.due_at})).collect::<Vec<_>>(),
            "waits": waits.iter().map(|wait| json!({"waitId": wait.wait_id, "event": wait.event})).collect::<Vec<_>>(),
        });
        let response: CommitResponse = self.endpoint.post_json(
            &format!(
                "/v1/workspaces/{}/executions/{}/commits",
                self.endpoint.workspace, commit.execution_id
            ),
            &commit.owner_token,
            &body,
        )?;
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
            },
        );
        Ok(CommitResult {
            revision: response.revision,
            owner: Some(response.owner),
            idempotent: response.idempotent,
        })
    }

    pub fn restore(&self, execution_id: &str, owner: &str) -> Result<Claimed> {
        self.remember(owner);
        let entry = self.fetch_entry(execution_id, owner)?;
        let status = entry.status;
        self.cache
            .lock()
            .expect("cache")
            .insert(execution_id.to_string(), entry.clone());
        Ok(Claimed {
            revision: entry.revision,
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
            lease_until: 0,
            continuation: Some(entry.continuation),
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
        let (_, response) = self.call("POST", path, owner, Some(&bytes), "application/json")?;
        serde_json::from_slice(&response).map_err(|err| Error::new(err.to_string()))
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str, owner: &str) -> Result<T> {
        let (_, response) = self.call("GET", path, owner, None, "application/json")?;
        serde_json::from_slice(&response).map_err(|err| Error::new(err.to_string()))
    }

    fn get_bytes(&self, path: &str, owner: &str) -> Result<Vec<u8>> {
        Ok(self
            .call("GET", path, owner, None, "application/octet-stream")?
            .1)
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
    ) -> Result<(u16, Vec<u8>)> {
        if self.port == 0 {
            return Err(Error::new(
                "the Pulse authority client speaks HTTP/1.1; terminate TLS at the deployment edge",
            ));
        }
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
        let (status, payload) = parse_http(&response)?;
        if !(200..300).contains(&status) {
            let message = String::from_utf8_lossy(&payload);
            let message = message.trim();
            if message.is_empty() {
                return Err(Error::new(format!("authority status {status}")));
            }
            return Err(Error::new(message.to_string()));
        }
        Ok((status, payload))
    }
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
struct ManifestResponse {
    revision: u64,
    owner: String,
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
