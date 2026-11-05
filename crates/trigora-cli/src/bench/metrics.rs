use std::time::{Duration, Instant};

use tcc_core::Engine;
use tcc_host_sqlite::{HostTraceEvent, HostTraceKind};
use tcc_ir::{decode_artifact, EngineCaps};
use tcc_state::decode_continuation;

use super::{CheckpointSample, Distribution};
use crate::error::CliError;

pub(in crate::bench) fn continuation_restore_ms(
    artifact_json: &str,
    bodies: &[String],
) -> Result<Vec<f64>, CliError> {
    if bodies.is_empty() {
        return Ok(Vec::new());
    }
    let artifact =
        decode_artifact(artifact_json).map_err(|error| CliError::plain(error.to_string()))?;
    let caps = EngineCaps::current();
    let mut samples = Vec::with_capacity(bodies.len());
    for body in bodies {
        let started = Instant::now();
        let continuation = decode_continuation(body.as_bytes())
            .map_err(|error| CliError::plain(error.to_string()))?;
        Engine::resume(artifact.clone(), continuation, &caps)
            .map_err(|error| CliError::plain(error.to_string()))?;
        samples.push(duration_ms(started.elapsed()));
    }
    Ok(samples)
}

pub(in crate::bench) fn checkpoint_samples(events: &[HostTraceEvent]) -> Vec<CheckpointSample> {
    events
        .iter()
        .filter(|event| event.kind == HostTraceKind::CheckpointPersisted)
        .map(|event| CheckpointSample {
            revision: event.revision.unwrap_or(0),
            duration_ms: duration_ms(event.duration),
            bytes: event.bytes.unwrap_or(0) as u64,
            suspended: event.suspended,
        })
        .collect()
}

pub(in crate::bench) fn distribution(samples: &[f64], with_total: bool) -> Distribution {
    let mut ordered = samples.to_vec();
    ordered.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mean = if ordered.is_empty() {
        None
    } else {
        Some(ordered.iter().sum::<f64>() / ordered.len() as f64)
    };
    Distribution {
        total: with_total.then(|| ordered.iter().sum()),
        min: ordered.first().copied(),
        mean,
        p50: nearest_rank(&ordered, 50.0),
        p95: nearest_rank(&ordered, 95.0),
        p99: nearest_rank(&ordered, 99.0),
        max: ordered.last().copied(),
    }
}

fn nearest_rank(sorted: &[f64], percentile: f64) -> Option<f64> {
    let count = sorted.len();
    if count == 0 {
        return None;
    }
    let rank = ((percentile / 100.0) * count as f64).ceil() as usize;
    let rank = rank.clamp(1, count);
    Some(sorted[rank - 1])
}

pub(in crate::bench) fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

pub(in crate::bench) fn process_cpu() -> Duration {
    #[cfg(unix)]
    {
        let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
        let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        if rc != 0 {
            return Duration::ZERO;
        }
        timeval(usage.ru_utime) + timeval(usage.ru_stime)
    }
    #[cfg(not(unix))]
    {
        Duration::ZERO
    }
}

fn timeval(value: libc::timeval) -> Duration {
    Duration::from_secs(value.tv_sec.max(0) as u64)
        + Duration::from_micros(value.tv_usec.max(0) as u64)
}

pub(in crate::bench) fn cpu_brand() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Some(brand) = command_stdout("sysctl", &["-n", "machdep.cpu.brand_string"]) {
            return brand;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(info) = std::fs::read_to_string("/proc/cpuinfo") {
            if let Some(model) = info.lines().find_map(|line| {
                line.strip_prefix("model name")
                    .and_then(|rest| rest.split(':').nth(1))
                    .map(str::trim)
            }) {
                if !model.is_empty() {
                    return model.to_string();
                }
            }
        }
    }
    std::env::consts::ARCH.to_string()
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_covers_empty_and_single_samples() {
        assert_eq!(nearest_rank(&[], 50.0), None);
        assert_eq!(nearest_rank(&[3.0], 50.0), Some(3.0));
        assert_eq!(nearest_rank(&[3.0], 95.0), Some(3.0));
        assert_eq!(nearest_rank(&[3.0], 99.0), Some(3.0));
        assert_eq!(nearest_rank(&[1.0, 2.0, 3.0], 50.0), Some(2.0));
        assert_eq!(nearest_rank(&[1.0, 2.0, 3.0], 95.0), Some(3.0));
    }
}
