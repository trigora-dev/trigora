use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::authority::{handler_digest, timers_of, Discovered, Readiness, Renewed};
use crate::error::{Error, Result};

use super::log::{is_connectivity, refusal_reason};
use super::state::{Phase, Slot};
use super::Inner;

enum Poll {
    Progress,
    Idle,
    Owned,
}

enum Admit {
    Advanced,
    Refused,
    NeedsActive,
}

pub(super) enum Visit {
    Handled,
    Blocked,
}

/// Move the cursor past claims and expected skips. A candidate that cannot be
/// admitted leaves the cursor where it was, immediately before that id.
pub(super) fn walk_page(
    cursor: &mut String,
    page: &[Discovered],
    mut visit: impl FnMut(&Discovered) -> Result<Visit>,
) -> Result<bool> {
    if page.is_empty() {
        *cursor = String::new();
        return Ok(false);
    }
    for candidate in page {
        match visit(candidate)? {
            Visit::Blocked => return Ok(true),
            Visit::Handled => *cursor = candidate.execution_id.clone(),
        }
    }
    *cursor = String::new();
    Ok(false)
}

struct Registered {
    hash: String,
    keys: Vec<String>,
}

impl Inner {
    pub(super) fn recovery_loop(self: &Arc<Self>) {
        let mut failures = 0u32;
        let mut owned_runs = 0u32;
        while !self.stop.load(Ordering::SeqCst) {
            self.promote_due_parks();
            if !self.authority.discovers() {
                interruptible_sleep(self, Duration::from_millis(200));
                continue;
            }
            match self.poll_discover() {
                Ok(Poll::Progress) => {
                    failures = 0;
                    owned_runs = 0;
                    interruptible_sleep(self, Duration::from_secs(5));
                }
                Ok(Poll::Idle) => {
                    failures = 0;
                    interruptible_sleep(self, Duration::from_secs(1));
                }
                Ok(Poll::Owned) => {
                    owned_runs = owned_runs.saturating_add(1);
                    let pause = if owned_runs >= 3 {
                        backoff(owned_runs)
                    } else {
                        Duration::from_secs(5)
                    };
                    interruptible_sleep(self, pause);
                }
                Err(_) => {
                    failures = failures.saturating_add(1);
                    interruptible_sleep(self, backoff(failures));
                }
            }
        }
    }

    pub(super) fn park_retained(&self, slot: &Arc<Slot>) {
        self.arm_park_timer(slot);
        *slot.phase.lock().expect("phase") = Phase::Suspended;
        self.suspended.fetch_add(1, Ordering::SeqCst);
        self.release_inflight();
    }

    pub(super) fn note_park_signals(&self, id: &str, slot: &Arc<Slot>, renewed: &Renewed) {
        if renewed.cancel_requested {
            slot.cancel.store(true, Ordering::SeqCst);
        }
        if renewed.child_ready {
            slot.child_ready.store(true, Ordering::SeqCst);
        }
        match renewed.next_wake_at {
            Some(due) => {
                slot.parked_until.store(due, Ordering::SeqCst);
                slot.park_timer.store(true, Ordering::SeqCst);
            }
            None => slot.park_timer.store(false, Ordering::SeqCst),
        }
        if renewed.timer_due
            || renewed.cancel_requested
            || renewed.has_accepted_event
            || renewed.child_ready
        {
            self.promote_parked(id, slot);
        }
    }

    pub(super) fn lose_ownership(&self, id: &str, slot: &Arc<Slot>) {
        slot.live.store(false, Ordering::SeqCst);
        slot.park_timer.store(false, Ordering::SeqCst);
        let suspended = matches!(*slot.phase.lock().expect("phase"), Phase::Suspended);
        {
            let mut slots = self.slots.lock().expect("slots");
            if slots
                .get(id)
                .is_some_and(|current| Arc::ptr_eq(current, slot))
            {
                slots.remove(id);
            }
        }
        if !suspended {
            self.abandon(id, slot);
        }
    }

    fn poll_discover(&self) -> Result<Poll> {
        let Some(registered) = self.registered_adapter() else {
            return Ok(Poll::Idle);
        };
        if let Some(blocked) = self.discover_blocked.lock().expect("blocked").clone() {
            if self.discover_blocked_active.load(Ordering::SeqCst) && self.active_full() {
                return Ok(Poll::Idle);
            }
            return match self.admit_discovered(&blocked, &registered)? {
                Admit::NeedsActive => Ok(Poll::Idle),
                Admit::Refused | Admit::Advanced => {
                    *self.discover_blocked.lock().expect("blocked") = None;
                    self.discover_blocked_active.store(false, Ordering::SeqCst);
                    *self.discover_cursor.lock().expect("cursor") = blocked;
                    Ok(Poll::Progress)
                }
            };
        }
        let digest = handler_digest(&registered.keys);
        let after = self.discover_cursor.lock().expect("cursor").clone();
        let page = match self.authority.discover(&registered.hash, &digest, &after) {
            Ok(page) => {
                self.log.authority("discover", true);
                page
            }
            Err(err) => {
                if is_connectivity(&err.to_string()) {
                    self.log.authority("discover", false);
                }
                return Err(err);
            }
        };
        if page.is_empty() {
            *self.discover_cursor.lock().expect("cursor") = String::new();
            return Ok(Poll::Idle);
        }
        let mut refused = 0usize;
        let mut advanced = 0usize;
        let mut cursor = after;
        let blocked = walk_page(&mut cursor, &page, |candidate| {
            if candidate.readiness == Readiness::Runnable && self.active_full() {
                *self.discover_blocked.lock().expect("blocked") =
                    Some(candidate.execution_id.clone());
                self.discover_blocked_active.store(true, Ordering::SeqCst);
                return Ok(Visit::Blocked);
            }
            match self.admit_discovered(&candidate.execution_id, &registered)? {
                Admit::NeedsActive => {
                    *self.discover_blocked.lock().expect("blocked") =
                        Some(candidate.execution_id.clone());
                    self.discover_blocked_active.store(true, Ordering::SeqCst);
                    Ok(Visit::Blocked)
                }
                Admit::Refused => {
                    refused += 1;
                    Ok(Visit::Handled)
                }
                Admit::Advanced => {
                    advanced += 1;
                    Ok(Visit::Handled)
                }
            }
        })?;
        *self.discover_cursor.lock().expect("cursor") = cursor;
        if blocked {
            return Ok(Poll::Idle);
        }
        if advanced == 0 && refused > 0 {
            return Ok(Poll::Owned);
        }
        Ok(Poll::Progress)
    }

    fn admit_discovered(&self, execution_id: &str, registered: &Registered) -> Result<Admit> {
        if self.slot(execution_id).is_some() || self.in_backlog(execution_id) {
            return Ok(Admit::Advanced);
        }
        let started = Instant::now();
        let restored = match self.restore_claimed(execution_id, &registered.hash, &registered.keys)
        {
            Ok(restored) => restored,
            Err(err) if expected_skip(&err) => {
                self.log.claim(
                    execution_id,
                    "refused",
                    None,
                    started.elapsed(),
                    Some(refusal_reason(&err.to_string())),
                );
                return Ok(Admit::Refused);
            }
            Err(err) => {
                if is_connectivity(&err.to_string()) {
                    self.log.authority("discover", false);
                }
                return Err(err);
            }
        };
        self.log.authority("discover", true);
        let fence = restored.fence;
        let elapsed = started.elapsed();
        let keys = crate::authority::canonical_keys(&registered.keys);
        // A cancel or child result can make this claim terminal. That outcome
        // is still uncommitted, so it has to run. Parking is only for a wait
        // that this claim did not wake.
        if restored.suspended && !restored.terminal {
            self.park_new(execution_id, restored, registered.hash.clone(), keys);
            self.log
                .claim(execution_id, "parked", Some(fence), elapsed, None);
            return Ok(Admit::Advanced);
        }
        if !self.take_active() {
            self.log
                .claim(execution_id, "acquired", Some(fence), elapsed, None);
            return Ok(Admit::NeedsActive);
        }
        self.launch(
            execution_id.to_string(),
            restored.engine,
            restored.memory,
            restored.fence,
            restored.lease_until,
            registered.hash.clone(),
            keys,
        );
        self.log
            .claim(execution_id, "acquired", Some(fence), elapsed, None);
        Ok(Admit::Advanced)
    }

    fn park_new(
        &self,
        execution_id: &str,
        restored: super::recovery::Restored,
        artifact_hash: String,
        handler_keys: Vec<String>,
    ) {
        self.insert_slot(
            execution_id.to_string(),
            restored.engine,
            restored.memory,
            restored.fence,
            restored.lease_until,
            artifact_hash,
            handler_keys,
        );
        let Some(slot) = self.slot(execution_id) else {
            return;
        };
        self.arm_park_timer(&slot);
        *slot.phase.lock().expect("phase") = Phase::Suspended;
    }

    fn promote_due_parks(&self) {
        let now = unix_ms();
        let ids: Vec<String> = self.slots.lock().expect("slots").keys().cloned().collect();
        for id in ids {
            let Some(slot) = self.slot(&id) else {
                continue;
            };
            if !slot.live.load(Ordering::SeqCst)
                || !slot.park_timer.load(Ordering::SeqCst)
                || slot.parked_until.load(Ordering::SeqCst) > now
                || !matches!(*slot.phase.lock().expect("phase"), Phase::Suspended)
            {
                continue;
            }
            let fence = slot.fence.load(Ordering::SeqCst);
            match self.authority.renew(&id, &self.owner, fence, self.lease_ms) {
                Ok(renewed) if renewed.fence == fence => {
                    self.log.authority("renew", true);
                    slot.lease_until
                        .store(renewed.lease_until, Ordering::SeqCst);
                    self.note_park_signals(&id, &slot, &renewed);
                }
                Ok(_) => {
                    self.log.fence_lost(&id);
                    self.lose_ownership(&id, &slot);
                }
                Err(err) if fatal_renewal(&err) => {
                    self.log.fence_lost(&id);
                    self.lose_ownership(&id, &slot);
                }
                Err(err) => {
                    if is_connectivity(&err.to_string()) {
                        self.log.authority("renew", false);
                    }
                }
            }
        }
    }

    fn promote_parked(&self, id: &str, slot: &Arc<Slot>) {
        {
            let mut phase = slot.phase.lock().expect("phase");
            if !matches!(*phase, Phase::Suspended) {
                return;
            }
            *phase = Phase::Recovering;
        }
        if !self.take_active() {
            *slot.phase.lock().expect("phase") = Phase::Suspended;
            return;
        }
        *slot.phase.lock().expect("phase") = Phase::Runnable;
        self.enqueue(id.to_string());
    }

    fn arm_park_timer(&self, slot: &Slot) {
        let due = slot
            .engine
            .lock()
            .expect("engine")
            .as_ref()
            .and_then(|engine| {
                timers_of(engine.continuation())
                    .into_iter()
                    .map(|timer| timer.due_at)
                    .min()
            });
        match due {
            Some(due) => {
                slot.parked_until.store(due, Ordering::SeqCst);
                slot.park_timer.store(true, Ordering::SeqCst);
            }
            None => slot.park_timer.store(false, Ordering::SeqCst),
        }
    }

    fn registered_adapter(&self) -> Option<Registered> {
        let connections = self.connections.lock().expect("connections");
        for connection in connections.iter() {
            if !connection.alive() {
                continue;
            }
            let hash = connection.artifact_hash.clone();
            let installed = self
                .artifacts
                .lock()
                .expect("artifacts")
                .contains_key(&hash);
            if !installed {
                continue;
            }
            return Some(Registered {
                hash,
                keys: connection.keys.clone(),
            });
        }
        None
    }
}

fn expected_skip(err: &Error) -> bool {
    matches!(
        err.to_string().as_str(),
        "execution is owned"
            | "artifact version mismatch"
            | "terminal state"
            | "artifact hash does not match the continuation"
    )
}

fn fatal_renewal(err: &Error) -> bool {
    matches!(
        err.to_string().as_str(),
        "stale owner" | "execution is owned"
    )
}

fn backoff(failures: u32) -> Duration {
    let shift = failures.saturating_sub(1).min(5);
    let base = (1_000u64 << shift).min(30_000);
    Duration::from_millis(base + unix_ms() % 250)
}

fn interruptible_sleep(inner: &Inner, total: Duration) {
    let step = Duration::from_millis(50);
    let mut left = total;
    while left > Duration::ZERO && !inner.stop.load(Ordering::SeqCst) {
        let pause = left.min(step);
        std::thread::sleep(pause);
        left = left.saturating_sub(pause);
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{walk_page, Visit};
    use crate::authority::{Discovered, Readiness};
    use crate::error::Error;

    fn candidate(id: &str, readiness: Readiness) -> Discovered {
        Discovered {
            execution_id: id.to_string(),
            readiness,
        }
    }

    #[test]
    fn a_full_active_cap_keeps_the_cursor_in_front_of_the_blocked_id() {
        let page = vec![
            candidate("a", Readiness::Runnable),
            candidate("b", Readiness::Runnable),
            candidate("c", Readiness::Parkable),
        ];
        let mut cursor = String::new();
        let blocked = walk_page(&mut cursor, &page, |item| {
            if item.execution_id == "b" {
                Ok(Visit::Blocked)
            } else {
                Ok(Visit::Handled)
            }
        })
        .unwrap();
        assert!(blocked);
        assert_eq!(cursor, "a");
    }

    #[test]
    fn a_finished_page_wraps_and_an_empty_page_wraps() {
        let page = vec![candidate("m", Readiness::Parkable)];
        let mut cursor = "a".to_string();
        let blocked = walk_page(&mut cursor, &page, |_| Ok(Visit::Handled)).unwrap();
        assert!(!blocked);
        assert_eq!(cursor, "");
        let blocked = walk_page(&mut cursor, &[], |_| Ok::<_, Error>(Visit::Handled)).unwrap();
        assert!(!blocked);
        assert_eq!(cursor, "");
    }

    #[test]
    fn an_error_does_not_advance_past_the_failing_id() {
        let page = vec![
            candidate("a", Readiness::Runnable),
            candidate("b", Readiness::Runnable),
        ];
        let mut cursor = String::new();
        let err = walk_page(&mut cursor, &page, |item| {
            if item.execution_id == "b" {
                Err(Error::new("transport"))
            } else {
                Ok(Visit::Handled)
            }
        })
        .unwrap_err();
        assert_eq!(err.to_string(), "transport");
        assert_eq!(cursor, "a");
    }
}
