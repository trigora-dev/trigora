use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PulseLevel {
    Debug,
    Info,
    Warn,
}

pub(super) struct PulseLog {
    min: PulseLevel,
    lines: Mutex<Vec<String>>,
    refusals: Mutex<HashMap<String, String>>,
    /// `None` until the first authority result. The first success is not a transition.
    authority_up: Mutex<Option<bool>>,
}

impl PulseLog {
    pub(super) fn parse(value: &str) -> Self {
        let min = if value.trim() == "debug" {
            PulseLevel::Debug
        } else {
            PulseLevel::Info
        };
        Self {
            min,
            lines: Mutex::new(Vec::new()),
            refusals: Mutex::new(HashMap::new()),
            authority_up: Mutex::new(None),
        }
    }

    pub(super) fn lines(&self) -> Vec<String> {
        self.lines.lock().expect("lines").clone()
    }

    pub(super) fn claim(
        &self,
        execution_id: &str,
        outcome: &str,
        fence: Option<u64>,
        duration: Duration,
        reason: Option<&str>,
    ) {
        if outcome == "refused" {
            let reason = reason.unwrap_or("other");
            let mut refusals = self.refusals.lock().expect("refusals");
            if refusals.get(execution_id).map(String::as_str) == Some(reason) {
                return;
            }
            refusals.insert(execution_id.to_string(), reason.to_string());
            self.emit(
                PulseLevel::Info,
                json!({
                    "event": "claim",
                    "executionId": execution_id,
                    "outcome": "refused",
                    "reason": reason,
                    "durationMs": millis(duration),
                }),
            );
            return;
        }
        self.refusals.lock().expect("refusals").remove(execution_id);
        let mut fields = json!({
            "event": "claim",
            "executionId": execution_id,
            "outcome": outcome,
            "durationMs": millis(duration),
        });
        if let Some(fence) = fence {
            fields["fence"] = json!(fence);
        }
        self.emit(PulseLevel::Info, fields);
    }

    pub(super) fn commit_ok(&self, execution_id: &str, revision: u64, duration: Duration) {
        self.emit(
            PulseLevel::Debug,
            json!({
                "event": "commit",
                "executionId": execution_id,
                "outcome": "ok",
                "revision": revision,
                "durationMs": millis(duration),
            }),
        );
    }

    pub(super) fn commit_failed(&self, execution_id: &str, duration: Duration, reason: &str) {
        self.emit(
            PulseLevel::Warn,
            json!({
                "event": "commit",
                "executionId": execution_id,
                "outcome": "failed",
                "reason": reason,
                "durationMs": millis(duration),
            }),
        );
    }

    pub(super) fn fence_lost(&self, execution_id: &str) {
        self.emit(
            PulseLevel::Warn,
            json!({
                "event": "fence-lost",
                "executionId": execution_id,
            }),
        );
    }

    pub(super) fn waiting_for_version(&self, execution_id: &str, artifact_hash: &str) {
        self.emit(
            PulseLevel::Warn,
            json!({
                "event": "waiting-for-version",
                "executionId": execution_id,
                "artifactHash": artifact_hash,
            }),
        );
    }

    /// Connectivity only. A reachable authority does not mean claims or commits are healthy.
    pub(super) fn authority(&self, operation: &str, reachable: bool) {
        let mut current = self.authority_up.lock().expect("authority");
        match *current {
            None if reachable => {
                *current = Some(true);
            }
            Some(previous) if previous == reachable => {}
            _ => {
                *current = Some(reachable);
                drop(current);
                self.emit(
                    PulseLevel::Warn,
                    json!({
                        "event": "authority",
                        "operation": operation,
                        "reachable": reachable,
                    }),
                );
            }
        }
    }

    fn allows(&self, level: PulseLevel) -> bool {
        match self.min {
            PulseLevel::Debug => true,
            PulseLevel::Info | PulseLevel::Warn => level != PulseLevel::Debug,
        }
    }

    fn emit(&self, level: PulseLevel, mut fields: Value) {
        if !self.allows(level) {
            return;
        }
        if let Some(object) = fields.as_object_mut() {
            object.insert("component".to_string(), json!("pulse"));
            object.insert("level".to_string(), json!(level_name(level)));
        }
        let line = format!("pulse {fields}");
        eprintln!("{line}");
        self.lines.lock().expect("lines").push(line);
    }
}

pub(super) fn refusal_reason(message: &str) -> &'static str {
    if message == "execution is owned" || message == "stale owner" {
        "owned"
    } else if message.contains("artifact")
        || message.contains("version")
        || message.contains("handler")
    {
        "version"
    } else {
        "other"
    }
}

pub(super) fn commit_reason(message: &str) -> &'static str {
    if is_connectivity(message) {
        "unreachable"
    } else if message == "stale owner" {
        "stale"
    } else if message == "execution is owned" {
        "owned"
    } else if message == "commit conflict" {
        "conflict"
    } else {
        "other"
    }
}

pub(super) fn is_fence_loss(message: &str) -> bool {
    message == "stale owner" || message == "execution is owned"
}

pub(super) fn is_connectivity(message: &str) -> bool {
    message.starts_with("authority connect:")
        || message.starts_with("authority write:")
        || message.starts_with("authority read:")
        || message.starts_with("tls:")
        || message.starts_with("authority status 5")
        || message == "authority response is truncated"
        || message == "authority response is too large"
        || message == "authority hostname is invalid"
}

fn level_name(level: PulseLevel) -> &'static str {
    match level {
        PulseLevel::Debug => "debug",
        PulseLevel::Info => "info",
        PulseLevel::Warn => "warn",
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::PulseLog;
    use std::time::Duration;

    fn objects(log: &PulseLog) -> Vec<serde_json::Value> {
        log.lines()
            .iter()
            .map(|line| {
                assert!(line.starts_with("pulse "), "{line}");
                serde_json::from_str(line.trim_start_matches("pulse ")).expect(line)
            })
            .collect()
    }

    #[test]
    fn info_hides_a_successful_commit_and_debug_shows_it() {
        let info = PulseLog::parse("");
        info.commit_ok("exec", 4, Duration::from_millis(12));
        assert!(info.lines().is_empty());
        info.commit_failed("exec", Duration::from_millis(9), "stale");
        info.fence_lost("exec");
        info.waiting_for_version("exec", "artifact");
        info.authority("discover", false);
        let events: Vec<_> = objects(&info)
            .into_iter()
            .map(|line| line["event"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            events,
            ["commit", "fence-lost", "waiting-for-version", "authority"]
        );
        assert!(objects(&info).iter().all(|line| line["level"] != "debug"));

        let debug = PulseLog::parse("debug");
        debug.commit_ok("exec", 4, Duration::from_millis(12));
        let line = &objects(&debug)[0];
        assert_eq!(line["level"], "debug");
        assert_eq!(line["outcome"], "ok");
        assert_eq!(line["revision"], 4);
        assert_eq!(line["durationMs"], 12);

        let unknown = PulseLog::parse("trace");
        unknown.commit_ok("exec", 1, Duration::from_millis(1));
        assert!(unknown.lines().is_empty());
    }

    #[test]
    fn a_repeated_refusal_is_logged_once() {
        let log = PulseLog::parse("info");
        log.claim(
            "exec",
            "refused",
            None,
            Duration::from_millis(3),
            Some("owned"),
        );
        log.claim(
            "exec",
            "refused",
            None,
            Duration::from_millis(3),
            Some("owned"),
        );
        assert_eq!(objects(&log).len(), 1);
        log.claim(
            "exec",
            "refused",
            None,
            Duration::from_millis(3),
            Some("version"),
        );
        assert_eq!(objects(&log).len(), 2);
        log.claim("exec", "acquired", Some(2), Duration::from_millis(8), None);
        log.claim(
            "exec",
            "refused",
            None,
            Duration::from_millis(3),
            Some("version"),
        );
        let lines = objects(&log);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[2]["outcome"], "acquired");
        assert_eq!(lines[2]["fence"], 2);
        assert_eq!(lines[2]["component"], "pulse");
    }

    #[test]
    fn a_commit_line_keeps_the_reason_class_only() {
        let log = PulseLog::parse("info");
        let noisy = "boom continuation TRIGORA_TOKEN=secret";
        assert_eq!(super::commit_reason("stale owner"), "stale");
        assert_eq!(super::commit_reason(noisy), "other");
        log.commit_failed(
            "exec",
            Duration::from_millis(1),
            super::commit_reason(noisy),
        );
        let line = &log.lines()[0];
        assert!(!line.contains("TRIGORA_TOKEN"));
        assert!(!line.contains("continuation"));
        assert!(!line.contains("secret"));
        assert!(line.contains("\"reason\":\"other\""));
    }

    #[test]
    fn authority_logs_connectivity_transitions_only() {
        let log = PulseLog::parse("info");
        log.authority("discover", true);
        assert!(log.lines().is_empty());
        log.authority("discover", false);
        log.authority("renew", false);
        log.authority("commit", true);
        let lines = objects(&log);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["event"], "authority");
        assert_eq!(lines[0]["operation"], "discover");
        assert_eq!(lines[0]["reachable"], false);
        assert_eq!(lines[1]["reachable"], true);
        assert_eq!(lines[1]["operation"], "commit");
    }
}
