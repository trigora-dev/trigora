use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value as Json;
use tcc_host_sqlite::{HostTraceEvent, RunResult, SqliteHost};
use tcc_state::Value;

use crate::adapter::Registry;
use crate::commands;
use crate::compile;
use crate::config;
use crate::error::CliError;
use crate::model::{program_slug, Program};
use crate::paths;

mod execution;
mod metrics;
mod report;
#[cfg(test)]
mod tests;

use execution::json_to_value;
pub use execution::{run, value_to_json};
pub use report::{render, verdict_error};

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
