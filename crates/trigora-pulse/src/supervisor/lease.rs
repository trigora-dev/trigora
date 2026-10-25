use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::time::Duration;

use super::state::{Phase, Slot};
use super::Inner;

impl Inner {
    pub(super) fn abandon(&self, _id: &str, slot: &Slot) {
        {
            let mut phase = slot.phase.lock().expect("phase");
            if matches!(
                *phase,
                Phase::Suspended | Phase::Completed | Phase::Abandoned
            ) {
                return;
            }
            *phase = Phase::Abandoned;
        }
        *slot.engine.lock().expect("engine") = None;
        self.abandoned.fetch_add(1, Ordering::SeqCst);
        self.release_inflight();
    }

    pub(super) fn spawn_renewal(&self, id: &str, slot: &Arc<Slot>) {
        if !self.authority.has_remote() {
            return;
        }
        if slot.renewing.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(inner) = self.me.get().and_then(Weak::upgrade) else {
            return;
        };
        let id = id.to_string();
        let slot = Arc::clone(slot);
        let handle = std::thread::spawn(move || inner.renew_loop(id, slot));
        self.renewers.lock().expect("renewers").push(handle);
    }

    pub(super) fn renew_loop(self: &Arc<Self>, id: String, slot: Arc<Slot>) {
        let mut failures = 0u32;
        while !self.stop.load(Ordering::SeqCst) && slot.live.load(Ordering::SeqCst) {
            let until = slot.lease_until.load(Ordering::SeqCst);
            let margin = (self.lease_ms / 3).max(50);
            let now = unix_ms();
            let wake = until.saturating_sub(margin);
            if until == 0 || now < wake {
                let pause = if until == 0 {
                    50
                } else {
                    (wake - now).min(200)
                };
                std::thread::sleep(Duration::from_millis(pause));
                continue;
            }
            let fence = slot.fence.load(Ordering::SeqCst);
            match self.authority.renew(&id, &self.owner, fence, self.lease_ms) {
                Ok(renewed) if renewed.fence == fence => {
                    failures = 0;
                    slot.lease_until
                        .store(renewed.lease_until, Ordering::SeqCst);
                }
                Ok(_) => {
                    slot.live.store(false, Ordering::SeqCst);
                    self.abandon(&id, &slot);
                    break;
                }
                Err(err) => {
                    let message = err.to_string();
                    let fatal = message == "stale owner" || message == "execution is owned";
                    failures += 1;
                    if fatal || failures >= 3 {
                        slot.live.store(false, Ordering::SeqCst);
                        self.abandon(&id, &slot);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50 * u64::from(failures)));
                }
            }
        }
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
