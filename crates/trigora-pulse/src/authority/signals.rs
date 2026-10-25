use super::identity::is_terminal;
use super::state::{State, StoredEvent};
use super::types::{CancelReceipt, EventReceipt, WaitRow};
use crate::error::{Error, Result};

pub(super) fn accept(
    state: &mut State,
    execution_id: &str,
    owner_token: &str,
    fence: u64,
    event_name: &str,
    wait_id: &str,
    payload: &str,
    digest: String,
) -> Result<EventReceipt> {
    let clock = state.clock;
    let record = state
        .executions
        .get_mut(execution_id)
        .ok_or_else(|| Error::new("unknown execution"))?;
    if record.owner.as_deref() != Some(owner_token)
        || record.fence != fence
        || record.lease_until <= clock
    {
        return Err(Error::new("stale owner"));
    }
    let current = record
        .waits
        .iter()
        .any(|wait| wait.wait_id == wait_id && wait.event == event_name);
    if !current {
        return Err(Error::new("wait is not current"));
    }
    if let Some(existing) = &record.event {
        if existing.wait_id == wait_id && existing.digest == digest {
            return Ok(EventReceipt {
                revision: record.revision,
                fence: record.fence,
                idempotent: true,
            });
        }
        return Err(Error::new("event conflict"));
    }
    let revision = record.revision;
    let fence = record.fence;
    record.event = Some(StoredEvent {
        wait_id: wait_id.to_string(),
        event: event_name.to_string(),
        payload: payload.to_string(),
        digest,
    });
    Ok(EventReceipt {
        revision,
        fence,
        idempotent: false,
    })
}

pub(super) fn request_cancel(state: &mut State, execution_id: &str) -> Result<CancelReceipt> {
    let record = state
        .executions
        .get_mut(execution_id)
        .ok_or_else(|| Error::new("unknown execution"))?;
    if is_terminal(record.status) || record.cancel_requested {
        return Ok(CancelReceipt {
            revision: record.revision,
            idempotent: true,
        });
    }
    record.cancel_requested = true;
    Ok(CancelReceipt {
        revision: record.revision,
        idempotent: false,
    })
}

/// Drop a stored payload only when the continuation used to contain its wait
/// and the committed continuation no longer does.
pub(super) fn release_event(
    event: &mut Option<StoredEvent>,
    previous: &[WaitRow],
    next: &[WaitRow],
) {
    let Some(stored) = event.as_ref() else {
        return;
    };
    let was = previous.iter().any(|wait| wait.wait_id == stored.wait_id);
    let still = next.iter().any(|wait| wait.wait_id == stored.wait_id);
    if was && !still {
        *event = None;
    }
}
