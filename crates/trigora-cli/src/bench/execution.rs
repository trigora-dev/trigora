use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value as Json};
use tcc_core::HOST_PROTOCOL_VERSION;
use tcc_host::HostError;
use tcc_host_sqlite::{
    resume_execution, start_execution_with_args, EffectProvider, HostTraceEvent, HostTraceKind,
    RunResult, SqliteHost,
};
use tcc_state::{encode_value, Value};

use crate::error::CliError;

use super::metrics::{
    checkpoint_samples, continuation_restore_ms, cpu_brand, distribution, duration_ms, process_cpu,
};
use super::{
    Baseline, EffectSource, Environment, FaultCase, FaultInjection, Faults, Recorded, Report,
    Request, Stopped, TempDb, EXECUTION_ID, SAMPLE_CAP,
};
use crate::versions::TCC_ENGINE;

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
            "This program has no checkpoint to recover from.",
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

fn argument_vector(input: Option<&Json>) -> Vec<Value> {
    match input {
        None => Vec::new(),
        Some(Json::Array(items)) => items.iter().map(json_to_value).collect(),
        Some(other) => vec![json_to_value(other)],
    }
}

pub(in crate::bench) fn json_to_value(value: &Json) -> Value {
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
