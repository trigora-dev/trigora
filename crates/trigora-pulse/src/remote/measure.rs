use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde_json::json;

use super::protocol::{checkpoint_frame, split_frame, SqliteCommitResponse, SqliteRestoreHeader};
use super::{RemoteClient, HARD_CONTINUATION_MAX};
use crate::authority::{self, JournalRow};
use crate::error::{Error, Result};

impl RemoteClient {
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
}

pub fn run_remote_measure() -> Result<()> {
    run_sqlite_measure()
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
