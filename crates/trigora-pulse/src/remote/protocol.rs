use serde::Deserialize;

use crate::authority::{TimerRow, WaitRow};
use crate::error::{Error, Result};

#[derive(Deserialize)]
pub(super) struct WireRow {
    pub(super) key: String,
    #[serde(rename = "idempotencyKey")]
    pub(super) idempotency_key: String,
    pub(super) status: String,
    pub(super) input: String,
    pub(super) result: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SqliteCommitResponse {
    pub(super) revision: u64,
    pub(super) owner: String,
    pub(super) fence: u64,
    pub(super) lease_until: u64,
    pub(super) idempotent: bool,
    pub(super) identity: String,
    pub(super) validation_us: u64,
    pub(super) continuation_bytes: u64,
    pub(super) chunk_count: u64,
    #[serde(default)]
    pub(super) chunks_written: u64,
    #[serde(default)]
    pub(super) rows_read: u64,
    #[serde(default)]
    pub(super) rows_written: u64,
    #[serde(default)]
    pub(super) bytes_rewritten: u64,
    #[serde(default)]
    pub(super) buffered_bytes: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VersionBody {
    pub(super) revision: u64,
    pub(super) artifact_hash: String,
    pub(super) handler_keys: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SqliteClaimResponse {
    pub(super) revision: u64,
    pub(super) fence: u64,
    pub(super) lease_until: u64,
    pub(super) continuation_digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SqliteRestoreHeader {
    pub(super) revision: u64,
    pub(super) owner: String,
    pub(super) fence: u64,
    pub(super) lease_until: u64,
    pub(super) identity: String,
    pub(super) continuation_digest: String,
    pub(super) continuation_bytes: u64,
    pub(super) journal: Vec<WireRow>,
    pub(super) timers: Vec<TimerJson>,
    pub(super) waits: Vec<WaitJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RenewResponse {
    pub(super) owner: String,
    pub(super) fence: u64,
    pub(super) lease_until: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QueryResponse {
    pub(super) accepted: bool,
    pub(super) revision: u64,
    pub(super) identity: String,
    pub(super) owner: String,
    pub(super) fence: u64,
    pub(super) lease_until: u64,
}

#[derive(Deserialize)]
pub(super) struct TimerJson {
    pub(super) branch: String,
    #[serde(rename = "dueAt")]
    pub(super) due_at: u64,
}

#[derive(Deserialize)]
pub(super) struct WaitJson {
    #[serde(rename = "waitId")]
    pub(super) wait_id: String,
    pub(super) event: String,
}

pub(super) fn manifest_timers(timers: &[TimerJson]) -> Vec<TimerRow> {
    timers
        .iter()
        .map(|timer| TimerRow {
            branch: timer.branch.clone(),
            due_at: timer.due_at,
        })
        .collect()
}

pub(super) fn manifest_waits(waits: &[WaitJson]) -> Vec<WaitRow> {
    waits
        .iter()
        .map(|wait| WaitRow {
            wait_id: wait.wait_id.clone(),
            event: wait.event.clone(),
        })
        .collect()
}

pub(super) fn checkpoint_frame(header: &serde_json::Value, continuation: &[u8]) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(header).map_err(|err| Error::new(err.to_string()))?;
    let len = u32::try_from(json.len()).map_err(|_| Error::new("checkpoint frame is invalid"))?;
    let mut frame = Vec::with_capacity(4 + json.len() + continuation.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&json);
    frame.extend_from_slice(continuation);
    Ok(frame)
}

pub(super) fn split_frame(bytes: &[u8]) -> Result<(&[u8], &[u8])> {
    if bytes.len() < 4 {
        return Err(Error::new("checkpoint frame is invalid"));
    }
    let len = u32::from_be_bytes(bytes[..4].try_into().expect("4 bytes")) as usize;
    if bytes.len() < 4 + len {
        return Err(Error::new("checkpoint frame is invalid"));
    }
    Ok((&bytes[4..4 + len], &bytes[4 + len..]))
}

pub(super) fn decode_chunks(mut bytes: &[u8]) -> Result<Vec<u8>> {
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
