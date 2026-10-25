use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use tcc_state::{Continuation, ContinuationStatus, PendingOp, WaitKind};

use super::types::{JournalRow, TimerRow, WaitRow};

pub(crate) fn canonical_keys(keys: &[String]) -> Vec<String> {
    let mut keys = keys.to_vec();
    keys.sort();
    keys.dedup();
    keys
}

pub(crate) fn timers_of(continuation: &Continuation) -> Vec<TimerRow> {
    match &continuation.pending {
        Some(PendingOp::Wait {
            kind: WaitKind::Timer { resume_at_ms },
        }) => vec![TimerRow {
            branch: String::new(),
            due_at: *resume_at_ms,
        }],
        _ => Vec::new(),
    }
}

pub(super) fn is_terminal(status: ContinuationStatus) -> bool {
    matches!(
        status,
        ContinuationStatus::Completed | ContinuationStatus::Failed | ContinuationStatus::Cancelled
    )
}

pub(crate) fn waits_of(continuation: &Continuation) -> Vec<WaitRow> {
    match &continuation.pending {
        Some(PendingOp::Wait {
            kind:
                WaitKind::Event {
                    wait_id,
                    event_name,
                    ..
                },
        }) => vec![WaitRow {
            wait_id: wait_id.clone(),
            event: event_name.clone(),
        }],
        _ => Vec::new(),
    }
}

pub(crate) fn commit_identity(
    revision: u64,
    digest: &str,
    journal: &BTreeMap<String, JournalRow>,
    timers: &[TimerRow],
    waits: &[WaitRow],
) -> String {
    let mut raw = format!("v1\n{revision}\n{digest}\n");
    for (key, row) in journal {
        raw.push_str(&format!(
            "j\t{key}\t{}\t{}\t{}\t{}\n",
            row.idempotency_key, row.status, row.input, row.result
        ));
    }
    let mut timers: Vec<&TimerRow> = timers.iter().collect();
    timers.sort();
    for timer in timers {
        raw.push_str(&format!("t\t{}\t{}\n", timer.branch, timer.due_at));
    }
    let mut waits: Vec<&WaitRow> = waits.iter().collect();
    waits.sort();
    for wait in waits {
        raw.push_str(&format!("w\t{}\t{}\n", wait.wait_id, wait.event));
    }
    sha256(raw.as_bytes())
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
