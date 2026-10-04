use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value as Json};
use tcc_core::{Engine, HOST_PROTOCOL_VERSION};
use tcc_host::HostError;
use tcc_host_sqlite::{
    resume_execution, start_execution_with_args, EffectProvider, HostTraceEvent, HostTraceKind,
    RunResult, SqliteHost,
};
use tcc_ir::{decode_artifact, EngineCaps};
use tcc_state::{decode_continuation, encode_value, Value};

use crate::adapter::Registry;
use crate::commands;
use crate::compile;
use crate::config;
use crate::error::CliError;
use crate::model::{program_slug, Program};
use crate::paths;
use crate::versions::TCC_ENGINE;

const EXECUTION_ID: &str = "bench";
const SAMPLE_CAP: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Faults {
    Sample,
    All,
}

impl std::fmt::Display for Faults {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Faults::Sample => write!(formatter, "sample"),
            Faults::All => write!(formatter, "all"),
        }
    }
}

pub enum Mode {
    Bench,
    Verify,
}

pub enum EffectSource {
    Absent,
    Fixtures(HashMap<String, Value>),
    Live(Box<dyn FnMut(&str, &Value) -> Result<Value, String> + Send>),
}

pub struct Request {
    pub name: String,
    pub artifact_json: String,
    pub artifact_hash: String,
    pub input: Option<Json>,
    pub has_effects: bool,
    pub effects: EffectSource,
    pub events: Vec<(String, Json)>,
    pub faults: Option<Faults>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub program: String,
    pub engine_version: String,
    pub host_protocol_version: u32,
    pub artifact_hash: String,
    pub baseline: Baseline,
    pub checkpoints: Vec<CheckpointSample>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault_injection: Option<FaultInjection>,
    pub environment: Environment,
}

#[derive(Clone, Debug, Serialize)]
pub struct Baseline {
    pub durable_boundaries: u64,
    pub healthy_runtime_ms: f64,
    pub checkpoint_ms: Distribution,
    pub continuation_bytes: Distribution,
    pub continuation_restore_ms: Distribution,
    pub provider_invocations: u64,
    pub journal_hits: u64,
    pub events_awaited: u64,
    pub cpu_ms: f64,
    pub sleeps_fast_forwarded: bool,
    pub status: String,
    pub result: Json,
}

#[derive(Clone, Debug, Serialize)]
pub struct Distribution {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p50: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p99: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckpointSample {
    pub revision: u64,
    pub duration_ms: f64,
    pub bytes: u64,
    pub suspended: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FaultInjection {
    pub mode: String,
    pub fault_points_tested: u64,
    pub fault_points_total: u64,
    pub successful_recoveries: u64,
    pub failed_recoveries: u64,
    pub result_mismatches: u64,
    pub duplicate_effects: u64,
    pub missing_effects: u64,
    pub provider_invocations: u64,
    pub journal_hits: u64,
    pub recovery_ms: Distribution,
    pub cases: Vec<FaultCase>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FaultCase {
    pub boundary: u64,
    pub matched: bool,
    pub mismatch: bool,
    pub duplicate_effects: u64,
    pub missing_effects: u64,
    pub provider_invocations: u64,
    pub resume_provider_invocations: u64,
    pub journal_hits: u64,
    pub recovery_ms: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Environment {
    pub os: String,
    pub arch: String,
    pub runtime: String,
    pub cpu: String,
}

#[derive(Clone)]
struct Recorded {
    key: String,
    input: String,
    result: Value,
}

struct Stopped {
    host: SqliteHost,
    log: Arc<Mutex<Vec<HostTraceEvent>>>,
    finished: Option<RunResult>,
    crashed: bool,
    elapsed_ms: f64,
    _db: TempDb,
}

struct TempDb {
    path: PathBuf,
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(format!("{}-wal", self.path.display()));
        let _ = fs::remove_file(format!("{}-shm", self.path.display()));
    }
}

pub fn run(mut request: Request) -> Result<Report, CliError> {
    if request.has_effects && matches!(request.effects, EffectSource::Absent) {
        return Err(CliError::new("Effects need an explicit source")
            .message(
                "This program has durable effects. Benchmarks do not call them unless you opt in.",
            )
            .detail("Fixtures", "--effects fixtures.json")
            .detail("Record once", "--effects live"));
    }
    let recording = Arc::new(Mutex::new(Vec::<Recorded>::new()));
    let cpu_before = process_cpu();
    let setup = effect_setup(&mut request.effects, &recording, true)?;
    let baseline = execute(&request, None, true, setup)?;
    let cpu_ms = duration_ms(process_cpu().saturating_sub(cpu_before));
    if baseline.crashed {
        return Err(CliError::plain(
            "The baseline run stopped at a crash hook. Bench and verify do not arm one.",
        ));
    }
    let finished = baseline
        .finished
        .clone()
        .ok_or_else(|| CliError::plain("The baseline run stopped before the program finished."))?;
    let events = baseline.log.lock().expect("trace").clone();
    let checkpoints = checkpoint_samples(&events);
    let bodies: Vec<String> = events
        .iter()
        .filter(|event| event.kind == HostTraceKind::CheckpointPersisted)
        .filter_map(|event| event.body.clone())
        .collect();
    let restore = continuation_restore_ms(&request.artifact_json, &bodies)?;
    let boundaries = checkpoints.len() as u64;
    let checkpoint_durations: Vec<f64> = checkpoints
        .iter()
        .map(|sample| sample.duration_ms)
        .collect();
    let sizes: Vec<f64> = checkpoints
        .iter()
        .map(|sample| sample.bytes as f64)
        .collect();
    let recorded = recording.lock().expect("recording").clone();
    let fault_injection = match request.faults {
        None => None,
        Some(mode) => Some(inject(
            &request, &finished, &events, &recorded, mode, boundaries,
        )?),
    };
    Ok(Report {
        schema_version: 1,
        program: request.name,
        engine_version: TCC_ENGINE.to_string(),
        host_protocol_version: HOST_PROTOCOL_VERSION,
        artifact_hash: request.artifact_hash,
        baseline: Baseline {
            durable_boundaries: boundaries,
            healthy_runtime_ms: baseline.elapsed_ms,
            checkpoint_ms: distribution(&checkpoint_durations, true),
            continuation_bytes: distribution(&sizes, false),
            continuation_restore_ms: distribution(&restore, false),
            provider_invocations: count_effects(&events, false),
            journal_hits: count_effects(&events, true),
            events_awaited: events
                .iter()
                .filter(|event| event.kind == HostTraceKind::CheckpointPersisted && event.suspended)
                .count() as u64,
            cpu_ms,
            sleeps_fast_forwarded: true,
            status: finished.status,
            result: value_to_json(finished.result.as_ref()),
        },
        checkpoints,
        fault_injection,
        environment: Environment {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            runtime: "in-process sqlite host".to_string(),
            cpu: cpu_brand(),
        },
    })
}

pub fn render(report: &Report, verify: bool) -> String {
    let mut lines = Vec::new();
    if verify {
        lines.push(format!("Trigora verify — {}", report.program));
        lines.push(String::new());
        lines.push("Baseline".to_string());
        lines.push(row(
            "Durable boundaries",
            &format_int(report.baseline.durable_boundaries),
        ));
        lines.push(row("Status", &report.baseline.status));
        if let Some(faults) = &report.fault_injection {
            lines.push(String::new());
            lines.push("Recovery".to_string());
            lines.push(row(
                "Fault points tested",
                &format!(
                    "{} / {}",
                    format_int(faults.fault_points_tested),
                    format_int(faults.fault_points_total)
                ),
            ));
            lines.push(row(
                "Successful recoveries",
                &format_int(faults.successful_recoveries),
            ));
            lines.push(row(
                "Failed recoveries",
                &format_int(faults.failed_recoveries),
            ));
            lines.push(row(
                "Result mismatches",
                &format_int(faults.result_mismatches),
            ));
            lines.push(row(
                "Duplicate effects",
                &format_int(faults.duplicate_effects),
            ));
            lines.push(row("Missing effects", &format_int(faults.missing_effects)));
            lines.push(row(
                "Provider invocations",
                &format_int(faults.provider_invocations),
            ));
            lines.push(row("Journal hits", &format_int(faults.journal_hits)));
            lines.push(row("Median recovery", &format_ms(faults.recovery_ms.p50)));
            lines.push(row("p95 recovery", &format_ms(faults.recovery_ms.p95)));
            lines.push(row("p99 recovery", &format_ms(faults.recovery_ms.p99)));
            lines.push(String::new());
            lines.push(verify_verdict(faults));
        }
    } else {
        lines.push(format!("Trigora benchmark — {}", report.program));
        lines.push(String::new());
        lines.push("Execution".to_string());
        lines.push(row(
            "Durable boundaries",
            &format_int(report.baseline.durable_boundaries),
        ));
        lines.push(row(
            "Healthy runtime",
            &format_ms(Some(report.baseline.healthy_runtime_ms)),
        ));
        let overhead = report.baseline.checkpoint_ms.total.unwrap_or(0.0);
        let share = if report.baseline.healthy_runtime_ms > 0.0 {
            format!(
                "  ({:.1}%)",
                100.0 * overhead / report.baseline.healthy_runtime_ms
            )
        } else {
            String::new()
        };
        lines.push(row(
            "Checkpoint overhead",
            &format!("{}{share}", format_ms(Some(overhead))),
        ));
        lines.push(String::new());
        lines.push("Continuation".to_string());
        lines.push(row(
            "Median size",
            &format_bytes(report.baseline.continuation_bytes.p50),
        ));
        lines.push(row(
            "p95 size",
            &format_bytes(report.baseline.continuation_bytes.p95),
        ));
        lines.push(row(
            "Maximum size",
            &format_bytes(report.baseline.continuation_bytes.max),
        ));
        lines.push(String::new());
        lines.push("Continuation restore".to_string());
        lines.push(row(
            "p50",
            &format_ms(report.baseline.continuation_restore_ms.p50),
        ));
        lines.push(row(
            "p95",
            &format_ms(report.baseline.continuation_restore_ms.p95),
        ));
        lines.push(row(
            "p99",
            &format_ms(report.baseline.continuation_restore_ms.p99),
        ));
        lines.push(String::new());
        lines.push("Effects".to_string());
        lines.push(row(
            "Provider invocations",
            &format_int(report.baseline.provider_invocations),
        ));
        lines.push(row(
            "Journal hits",
            &format_int(report.baseline.journal_hits),
        ));
        lines.push(row(
            "Events awaited",
            &format_int(report.baseline.events_awaited),
        ));
        lines.push(row("CPU time", &format_ms(Some(report.baseline.cpu_ms))));
    }
    lines.push(String::new());
    lines.push(
        "Sleeps are fast-forwarded. Healthy runtime does not include sleep delays.".to_string(),
    );
    lines.push(String::new());
    lines.join("\n")
}

pub fn verdict_error(report: &Report) -> Option<CliError> {
    let Some(faults) = &report.fault_injection else {
        return None;
    };
    if faults.failed_recoveries == 0
        && faults.result_mismatches == 0
        && faults.duplicate_effects == 0
        && faults.missing_effects == 0
    {
        return None;
    }
    Some(
        CliError::new("Recovery check failed")
            .detail("Failed recoveries", faults.failed_recoveries.to_string())
            .detail("Result mismatches", faults.result_mismatches.to_string())
            .detail("Duplicate effects", faults.duplicate_effects.to_string())
            .detail("Missing effects", faults.missing_effects.to_string()),
    )
}

fn verify_verdict(faults: &FaultInjection) -> String {
    if faults.failed_recoveries == 0
        && faults.result_mismatches == 0
        && faults.duplicate_effects == 0
        && faults.missing_effects == 0
    {
        if faults.mode == "all" {
            "✓ Recovery verified at every durable boundary".to_string()
        } else {
            format!(
                "✓ Recovery verified at {} sampled durable boundaries",
                format_int(faults.fault_points_tested)
            )
        }
    } else {
        "✗ Recovery check failed".to_string()
    }
}

fn inject(
    request: &Request,
    baseline: &RunResult,
    baseline_events: &[HostTraceEvent],
    recorded: &[Recorded],
    mode: Faults,
    boundaries: u64,
) -> Result<FaultInjection, CliError> {
    let indexes = match mode {
        Faults::All => (1..=boundaries).collect::<Vec<_>>(),
        Faults::Sample => sample_boundaries(boundaries, SAMPLE_CAP),
    };
    if indexes.is_empty() {
        return Err(CliError::plain(
            "This program committed no durable checkpoint to recover from.",
        ));
    }
    let baseline_providers = provider_keys(baseline_events);
    let mut cases = Vec::new();
    for boundary in indexes {
        cases.push(fault_case(
            request,
            baseline,
            recorded,
            &baseline_providers,
            boundary,
        )?);
    }
    let recoveries: Vec<f64> = cases.iter().map(|case| case.recovery_ms).collect();
    Ok(FaultInjection {
        mode: match mode {
            Faults::All => "all".to_string(),
            Faults::Sample => "sample".to_string(),
        },
        fault_points_tested: cases.len() as u64,
        fault_points_total: boundaries,
        successful_recoveries: cases.iter().filter(|case| case.matched).count() as u64,
        failed_recoveries: cases.iter().filter(|case| !case.matched).count() as u64,
        result_mismatches: cases.iter().filter(|case| case.mismatch).count() as u64,
        duplicate_effects: cases.iter().map(|case| case.duplicate_effects).sum(),
        missing_effects: cases.iter().map(|case| case.missing_effects).sum(),
        provider_invocations: cases.iter().map(|case| case.provider_invocations).sum(),
        journal_hits: cases.iter().map(|case| case.journal_hits).sum(),
        recovery_ms: distribution(&recoveries, false),
        cases,
    })
}

fn fault_case(
    request: &Request,
    baseline: &RunResult,
    recorded: &[Recorded],
    baseline_providers: &[String],
    boundary: u64,
) -> Result<FaultCase, CliError> {
    let recording = Arc::new(Mutex::new(recorded.to_vec()));
    let setup = if recorded.is_empty() {
        let mut source = match &request.effects {
            EffectSource::Fixtures(map) => EffectSource::Fixtures(map.clone()),
            EffectSource::Absent => EffectSource::Absent,
            EffectSource::Live(_) => EffectSource::Live(Box::new(|_, _| {
                Err("live handlers are not called while probing fault points".to_string())
            })),
        };
        effect_setup(&mut source, &recording, false)?
    } else {
        replay_setup(&recording)
    };
    let mut crashed = execute(request, Some(boundary), false, setup)?;
    if !crashed.crashed {
        return Ok(empty_case(boundary, crashed.elapsed_ms, false, false));
    }
    let prefix = crashed.log.lock().expect("trace").len();
    let completed = {
        let events = crashed.log.lock().expect("trace");
        completed_keys(&crashed.host, provider_keys(&events[..prefix]))
    };
    crashed.host.crash.want = None;
    let started = Instant::now();
    let resumed = resume_execution(
        &mut crashed.host,
        Some(&request.artifact_json),
        EXECUTION_ID,
    );
    let recovery_ms = duration_ms(started.elapsed());
    let events = crashed.log.lock().expect("trace").clone();
    let prefix_events = &events[..prefix.min(events.len())];
    let resume_events = &events[prefix.min(events.len())..];
    let resume_providers = provider_keys(resume_events);
    let fault_providers = provider_keys(prefix_events)
        .into_iter()
        .chain(resume_providers.iter().cloned())
        .collect::<Vec<_>>();
    let duplicates = resume_providers
        .iter()
        .filter(|key| completed.contains(*key))
        .count() as u64;
    match resumed {
        Ok(result) => {
            let mismatch = result.status != baseline.status || result.result != baseline.result;
            let missing = missing_keys(baseline_providers, &fault_providers);
            Ok(FaultCase {
                boundary,
                matched: !mismatch && duplicates == 0 && missing == 0,
                mismatch,
                duplicate_effects: duplicates,
                missing_effects: missing,
                provider_invocations: fault_providers.len() as u64,
                resume_provider_invocations: resume_providers.len() as u64,
                journal_hits: count_effects(resume_events, true),
                recovery_ms,
            })
        }
        Err(_) => Ok(empty_case(boundary, recovery_ms, false, false)),
    }
}

fn empty_case(boundary: u64, recovery_ms: f64, matched: bool, mismatch: bool) -> FaultCase {
    FaultCase {
        boundary,
        matched,
        mismatch,
        duplicate_effects: 0,
        missing_effects: 0,
        provider_invocations: 0,
        resume_provider_invocations: 0,
        journal_hits: 0,
        recovery_ms,
    }
}

fn execute(
    request: &Request,
    crash_at: Option<u64>,
    retain: bool,
    setup: EffectSetup,
) -> Result<Stopped, CliError> {
    let db = TempDb { path: temp_db() };
    let log = Arc::new(Mutex::new(Vec::new()));
    let seen = log.clone();
    let mut host = SqliteHost::open(&db.path, EXECUTION_ID).map_err(host_err)?;
    host.trace_continuations = retain;
    host.auto_deliver = true;
    host.set_trace(Box::new(move |event| {
        seen.lock().expect("trace").push(event);
    }));
    match setup {
        EffectSetup::Map(map) => host.effects = map,
        EffectSetup::Provider(provider) => host.set_effect_provider(provider),
        EffectSetup::None => {}
    }
    if let Some(boundary) = crash_at {
        host.crash.want = Some(format!("after_persist_checkpoint:{boundary}"));
        host.crash.kill = false;
    }
    seed_events(&mut host, &request.events)?;
    let args = argument_vector(request.input.as_ref());
    let started = Instant::now();
    let outcome = start_execution_with_args(&mut host, &request.artifact_json, EXECUTION_ID, &args);
    let elapsed_ms = duration_ms(started.elapsed());
    match outcome {
        Ok(result) => Ok(Stopped {
            host,
            log,
            finished: Some(result),
            crashed: false,
            elapsed_ms,
            _db: db,
        }),
        Err(error) if is_crash(&error) => Ok(Stopped {
            host,
            log,
            finished: None,
            crashed: true,
            elapsed_ms,
            _db: db,
        }),
        Err(error) => Err(host_err(error)),
    }
}

enum EffectSetup {
    None,
    Map(HashMap<String, Value>),
    Provider(EffectProvider),
}

fn effect_setup(
    source: &mut EffectSource,
    recording: &Arc<Mutex<Vec<Recorded>>>,
    live: bool,
) -> Result<EffectSetup, CliError> {
    match source {
        EffectSource::Absent => Ok(EffectSetup::None),
        EffectSource::Fixtures(map) => Ok(EffectSetup::Map(map.clone())),
        EffectSource::Live(_) if !live => Ok(replay_setup(recording)),
        EffectSource::Live(handler) => {
            let mut handler = std::mem::replace(
                handler,
                Box::new(|_, _| Err("effect handler was already consumed".to_string())),
            );
            let recording = recording.clone();
            Ok(EffectSetup::Provider(Box::new(move |key, input| {
                let result = handler(key, input).map_err(HostError::Message)?;
                let encoded = canonical(input).map_err(HostError::Message)?;
                recording.lock().expect("recording").push(Recorded {
                    key: key.to_string(),
                    input: encoded,
                    result: result.clone(),
                });
                Ok(result)
            })))
        }
    }
}

fn replay_setup(recording: &Arc<Mutex<Vec<Recorded>>>) -> EffectSetup {
    let recorded = recording.lock().expect("recording").clone();
    EffectSetup::Provider(Box::new(move |key, input| {
        let encoded = canonical(input).map_err(HostError::Message)?;
        recorded
            .iter()
            .find(|row| row.key == key && row.input == encoded)
            .map(|row| row.result.clone())
            .ok_or_else(|| {
                HostError::Message(format!(
                    "no recorded result for effect `{key}` with this input"
                ))
            })
    }))
}

fn seed_events(host: &mut SqliteHost, events: &[(String, Json)]) -> Result<(), CliError> {
    for (name, payload) in events {
        let encoded = canonical(&json_to_value(payload)).map_err(|error| CliError::plain(error))?;
        host.store
            .enqueue_event(EXECUTION_ID, name, &encoded)
            .map_err(|error| CliError::plain(error.message))?;
    }
    Ok(())
}

fn continuation_restore_ms(artifact_json: &str, bodies: &[String]) -> Result<Vec<f64>, CliError> {
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

fn checkpoint_samples(events: &[HostTraceEvent]) -> Vec<CheckpointSample> {
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

fn count_effects(events: &[HostTraceEvent], journal_hit: bool) -> u64 {
    events
        .iter()
        .filter(|event| event.kind == HostTraceKind::Effect && event.journal_hit == journal_hit)
        .count() as u64
}

fn provider_keys(events: &[HostTraceEvent]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event.kind == HostTraceKind::Effect && !event.journal_hit)
        .filter_map(|event| event.effect_key.clone())
        .collect()
}

fn completed_keys(host: &SqliteHost, keys: Vec<String>) -> HashSet<String> {
    let mut done = HashSet::new();
    for key in keys {
        if let Ok(Some(row)) = host.store.effect(EXECUTION_ID, &key) {
            if row.status == "completed" {
                done.insert(key);
            }
        }
    }
    done
}

fn missing_keys(baseline: &[String], fault: &[String]) -> u64 {
    let mut available: HashMap<&str, u64> = HashMap::new();
    for key in fault {
        *available.entry(key.as_str()).or_insert(0) += 1;
    }
    let mut missing = 0;
    let mut seen: HashMap<&str, u64> = HashMap::new();
    for key in baseline {
        let nth = seen.entry(key.as_str()).or_insert(0);
        *nth += 1;
        if *nth > *available.get(key.as_str()).unwrap_or(&0) {
            missing += 1;
        }
    }
    missing
}

fn sample_boundaries(total: u64, cap: usize) -> Vec<u64> {
    if total == 0 || cap == 0 {
        return Vec::new();
    }
    if total as usize <= cap {
        return (1..=total).collect();
    }
    let mut picks = Vec::new();
    let last = cap - 1;
    for index in 0..cap {
        let boundary = 1 + (index as u64) * (total - 1) / last as u64;
        if picks.last() != Some(&boundary) {
            picks.push(boundary);
        }
    }
    picks
}

fn distribution(samples: &[f64], with_total: bool) -> Distribution {
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

fn argument_vector(input: Option<&Json>) -> Vec<Value> {
    match input {
        None => Vec::new(),
        Some(Json::Array(items)) => items.iter().map(json_to_value).collect(),
        Some(other) => vec![json_to_value(other)],
    }
}

fn json_to_value(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(flag) => Value::Bool(*flag),
        Json::Number(number) => Value::Number(number.as_f64().unwrap_or(f64::NAN)),
        Json::String(text) => Value::String(text.clone()),
        Json::Array(items) => Value::Array(items.iter().map(json_to_value).collect()),
        Json::Object(map) => {
            let mut fields = BTreeMap::new();
            for (key, item) in map {
                fields.insert(key.clone(), json_to_value(item));
            }
            Value::Object(fields)
        }
    }
}

pub fn value_to_json(value: Option<&Value>) -> Json {
    match value {
        None => Json::Null,
        Some(value) => match value {
            Value::Undefined | Value::Null | Value::Ref(_) => Json::Null,
            Value::Bool(flag) => Json::Bool(*flag),
            Value::Number(number) => json!(number),
            Value::String(text) => Json::String(text.clone()),
            Value::Array(items) => {
                Json::Array(items.iter().map(|item| value_to_json(Some(item))).collect())
            }
            Value::Object(fields) => {
                let mut map = serde_json::Map::new();
                for (key, item) in fields {
                    map.insert(key.clone(), value_to_json(Some(item)));
                }
                Json::Object(map)
            }
        },
    }
}

fn canonical(value: &Value) -> Result<String, String> {
    let bytes = encode_value(value).map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn is_crash(error: &HostError) -> bool {
    error.to_string().contains("crashed at ")
}

fn host_err(error: impl ToString) -> CliError {
    CliError::plain(error.to_string())
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn temp_db() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "trigora-bench-{}-{nanos}-{n}.db",
        std::process::id()
    ))
}

fn process_cpu() -> Duration {
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

#[cfg(unix)]
fn timeval(value: libc::timeval) -> Duration {
    Duration::from_secs(value.tv_sec.max(0) as u64)
        + Duration::from_micros(value.tv_usec.max(0) as u64)
}

fn cpu_brand() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Some(brand) = command_stdout("sysctl", &["-n", "machdep.cpu.brand_string"]) {
            return brand;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(info) = fs::read_to_string("/proc/cpuinfo") {
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

#[cfg(target_os = "macos")]
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

pub fn launch(
    mode: Mode,
    program_name: &str,
    input: Option<&str>,
    effects: Option<&str>,
    events: Option<&str>,
    faults: Option<Faults>,
    out: Option<&str>,
) -> Result<(), CliError> {
    let root = std::env::current_dir().map_err(|error| CliError::plain(error.to_string()))?;
    let project = config::load_config(&root)?;
    let tools = paths::tools()?;
    let registry = Arc::new(Registry::new(&tools));
    let result = (|| {
        let programs = compile::discover(&project.root, &project.programs, &registry)?;
        let program = select_program(&programs, program_name)?;
        let input = commands::read_json(input, true, "Invalid input")?;
        let events = load_events(events)?;
        let effect_source = load_effects(effects, program, &registry)?;
        let faults = match mode {
            Mode::Bench => None,
            Mode::Verify => Some(faults.unwrap_or(Faults::Sample)),
        };
        let report = run(Request {
            name: program.id.clone(),
            artifact_json: program.artifact_json.clone(),
            artifact_hash: program.artifact_hash.clone(),
            input: Some(input),
            has_effects: !program.effects.is_empty(),
            effects: effect_source,
            events,
            faults,
        })?;
        print!("{}", render(&report, matches!(mode, Mode::Verify)));
        if let Some(path) = out {
            let body = serde_json::to_string_pretty(&report)
                .map_err(|error| CliError::plain(error.to_string()))?;
            fs::write(path, format!("{body}\n"))
                .map_err(|error| CliError::plain(format!("Failed to write {path}: {error}")))?;
            println!("Results written to {path}");
        }
        if let Some(error) = verdict_error(&report) {
            return Err(error);
        }
        Ok(())
    })();
    registry.shutdown();
    result
}

fn select_program<'a>(programs: &'a [Program], name: &str) -> Result<&'a Program, CliError> {
    let matches = programs
        .iter()
        .filter(|program| {
            program.id == name || program_slug(&program.id).ok().as_deref() == Some(name)
        })
        .collect::<Vec<_>>();
    match matches.len() {
        1 => Ok(matches[0]),
        0 => Err(CliError::new("Program not found").detail("Program", name)),
        _ => Err(CliError::new("Program name is ambiguous").detail("Program", name)),
    }
}

fn load_effects(
    flag: Option<&str>,
    program: &Program,
    registry: &Arc<Registry>,
) -> Result<EffectSource, CliError> {
    let Some(flag) = flag else {
        return Ok(EffectSource::Absent);
    };
    if flag == "live" {
        let registry = registry.clone();
        let language = program.language.clone();
        let effects = program.effects.clone();
        return Ok(EffectSource::Live(Box::new(move |key, input| {
            let effect = effects
                .iter()
                .find(|effect| effect.key == key)
                .cloned()
                .ok_or_else(|| format!("no effect `{key}` in the compiled program"))?;
            let produced = registry
                .run_effect(&language, &effect, key, &value_to_json(Some(input)))
                .map_err(|error| error.to_string())?;
            let Some(produced) = produced else {
                return Err(format!("effect `{key}` has no local handler"));
            };
            Ok(json_to_value(&produced))
        })));
    }
    let parsed = commands::read_json(Some(flag), false, "Invalid effects")?;
    let Json::Object(map) = parsed else {
        return Err(CliError::new("Invalid effects")
            .message("The fixtures file must be a JSON object of effect key to value."));
    };
    let mut fixtures = HashMap::new();
    for (key, value) in map {
        fixtures.insert(key, json_to_value(&value));
    }
    Ok(EffectSource::Fixtures(fixtures))
}

fn load_events(flag: Option<&str>) -> Result<Vec<(String, Json)>, CliError> {
    let Some(flag) = flag else {
        return Ok(Vec::new());
    };
    let parsed = commands::read_json(Some(flag), false, "Invalid events")?;
    let Json::Object(map) = parsed else {
        return Err(CliError::new("Invalid events").message(
            "The events file must be a JSON object of event name to payload or payload array.",
        ));
    };
    let mut events = Vec::new();
    for (name, value) in map {
        if let Json::Array(items) = value {
            for item in items {
                events.push((name.clone(), item));
            }
        } else {
            events.push((name, value));
        }
    }
    Ok(events)
}

fn row(label: &str, value: &str) -> String {
    format!("  {label:<26}{value}")
}

fn format_int(value: u64) -> String {
    let raw = value.to_string();
    let mut out = String::new();
    for (index, ch) in raw.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_ms(value: Option<f64>) -> String {
    let Some(value) = value else {
        return "—".to_string();
    };
    if value < 1.0 {
        format!("{value:.3} ms")
    } else if value < 100.0 {
        format!("{value:.1} ms")
    } else {
        format!("{value:.0} ms")
    }
}

fn format_bytes(value: Option<f64>) -> String {
    let Some(value) = value else {
        return "—".to_string();
    };
    if value >= 1024.0 {
        format!("{:.1} KB", value / 1024.0)
    } else {
        format!("{value:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tcc_ir::{
        encode_artifact, Artifact, ConstValue, Envelope, FuncId, Function, Instruction, Program,
    };

    fn program(instructions: Vec<Instruction>, locals: u32) -> (String, String) {
        let artifact = Artifact {
            envelope: Envelope::typescript_v1("bench-hash"),
            program: Program {
                entry: FuncId(0),
                functions: vec![Function {
                    id: FuncId(0),
                    name: "run".to_string(),
                    param_count: 0,
                    local_count: locals,
                    param_defaults: Vec::new(),
                    spans: vec![None; instructions.len()],
                    instructions,
                }],
            },
        };
        let json = encode_artifact(&artifact).unwrap();
        (json, artifact.envelope.artifact_hash)
    }

    fn request(
        json: String,
        hash: String,
        effects: EffectSource,
        faults: Option<Faults>,
    ) -> Request {
        let has_effects = !matches!(effects, EffectSource::Absent);
        Request {
            name: "sample".to_string(),
            artifact_json: json,
            artifact_hash: hash,
            input: None,
            has_effects,
            effects,
            events: Vec::new(),
            faults,
        }
    }

    #[test]
    fn nearest_rank_covers_empty_and_single_samples() {
        assert_eq!(nearest_rank(&[], 50.0), None);
        assert_eq!(nearest_rank(&[3.0], 50.0), Some(3.0));
        assert_eq!(nearest_rank(&[3.0], 95.0), Some(3.0));
        assert_eq!(nearest_rank(&[3.0], 99.0), Some(3.0));
        assert_eq!(nearest_rank(&[1.0, 2.0, 3.0], 50.0), Some(2.0));
        assert_eq!(nearest_rank(&[1.0, 2.0, 3.0], 95.0), Some(3.0));
    }

    #[test]
    fn bench_boundary_count_matches_the_trace() {
        let (json, hash) = program(
            vec![
                Instruction::LoadConst {
                    value: ConstValue::Number(1.0),
                },
                Instruction::Return,
            ],
            0,
        );
        let report = run(request(json, hash, EffectSource::Absent, None)).unwrap();
        assert_eq!(
            report.baseline.durable_boundaries as usize,
            report.checkpoints.len()
        );
        assert!(report.baseline.durable_boundaries >= 1);
        assert!(report.fault_injection.is_none());
        assert!(report.baseline.continuation_restore_ms.p50.is_some());
    }

    #[test]
    fn verify_sample_recovers_with_zero_mismatches() {
        let (json, hash) = program(
            vec![
                Instruction::LoadConst {
                    value: ConstValue::String("approval".to_string()),
                },
                Instruction::WaitForEvent,
                Instruction::LoadConst {
                    value: ConstValue::Number(1.0),
                },
                Instruction::Return,
            ],
            0,
        );
        let report = run(request(
            json,
            hash,
            EffectSource::Absent,
            Some(Faults::Sample),
        ))
        .unwrap();
        let faults = report.fault_injection.unwrap();
        assert!(faults.fault_points_tested >= 1);
        assert_eq!(faults.result_mismatches, 0);
        assert_eq!(faults.failed_recoveries, 0);
        assert_eq!(faults.duplicate_effects, 0);
        assert_eq!(faults.successful_recoveries, faults.fault_points_tested);
    }

    #[test]
    fn effects_without_a_source_are_refused() {
        let (json, hash) = program(
            vec![
                Instruction::LoadConst {
                    value: ConstValue::String("charge".to_string()),
                },
                Instruction::Effect { has_input: false },
                Instruction::Return,
            ],
            0,
        );
        let error = run(Request {
            has_effects: true,
            effects: EffectSource::Absent,
            ..request(json, hash, EffectSource::Absent, None)
        })
        .unwrap_err();
        assert!(error.title.contains("Effects"));
    }

    #[test]
    fn live_effects_record_once_and_resume_hits_the_journal() {
        let (json, hash) = program(
            vec![
                Instruction::LoadConst {
                    value: ConstValue::String("charge".to_string()),
                },
                Instruction::Effect { has_input: false },
                Instruction::Return,
            ],
            0,
        );
        let calls = Arc::new(AtomicU32::new(0));
        let seen = calls.clone();
        let report = run(request(
            json,
            hash,
            EffectSource::Live(Box::new(move |_key, _input| {
                seen.fetch_add(1, Ordering::SeqCst);
                Ok(Value::Number(7.0))
            })),
            Some(Faults::All),
        ))
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let faults = report.fault_injection.unwrap();
        assert_eq!(faults.duplicate_effects, 0);
        assert_eq!(faults.missing_effects, 0);
        assert_eq!(faults.result_mismatches, 0);
        assert!(
            faults
                .cases
                .iter()
                .any(|case| case.resume_provider_invocations == 0),
            "recovery after a committed effect must not call the provider again: {:?}",
            faults
                .cases
                .iter()
                .map(|case| (
                    case.boundary,
                    case.resume_provider_invocations,
                    case.journal_hits
                ))
                .collect::<Vec<_>>()
        );
    }
}
