use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value as Json};
use toml::Value as Toml;

use crate::error::CliError;

#[derive(Clone, Debug)]
pub struct Trigger {
    pub name: String,
    pub kind: String,
    pub program: String,
    pub schedule: Option<String>,
    pub timezone: Option<String>,
    pub input: Option<Json>,
}

#[derive(Clone, Debug)]
pub struct ProjectConfig {
    pub root: PathBuf,
    pub path: PathBuf,
    pub project_name: String,
    pub programs: Vec<String>,
    pub triggers: Vec<Trigger>,
}

const CRON_BOUNDS: [(i64, i64); 5] = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)];

pub fn load_config(root: &Path) -> Result<ProjectConfig, CliError> {
    let path = root.join("trigora.toml");
    let source = fs::read_to_string(&path).map_err(|_| {
        CliError::new("No trigora.toml found")
            .hint("Run `trigora init` or add trigora.toml with `[project].programs`.")
    })?;
    parse_manifest(&source).map(|parsed| ProjectConfig {
        root: root.to_path_buf(),
        path,
        project_name: parsed.0,
        programs: parsed.1,
        triggers: parsed.2,
    })
}

fn parse_manifest(source: &str) -> Result<(String, Vec<String>, Vec<Trigger>), CliError> {
    let parsed: Toml =
        toml::from_str(source).map_err(|error| CliError::plain(error.to_string()))?;
    let table = parsed
        .as_table()
        .ok_or_else(|| CliError::plain("`[project].name` is required."))?;
    for key in table.keys() {
        if key != "project" && key != "triggers" {
            return Err(CliError::plain(format!(
                "Unknown field `{key}` in trigora.toml."
            )));
        }
    }
    let project = table
        .get("project")
        .and_then(Toml::as_table)
        .ok_or_else(|| CliError::plain("`[project].name` is required."))?;
    for key in project.keys() {
        if key != "name" && key != "programs" {
            return Err(CliError::plain(format!(
                "Unknown field `{key}` in [project]."
            )));
        }
    }
    let project_name = project
        .get("name")
        .and_then(Toml::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| CliError::plain("`[project].name` is required."))?
        .to_string();
    let programs = read_globs(project.get("programs"))?;
    let raw_triggers = table
        .get("triggers")
        .cloned()
        .unwrap_or(Toml::Array(Vec::new()));
    let Some(entries) = raw_triggers.as_array() else {
        return Err(CliError::plain("`triggers` must be an array of tables."));
    };
    let mut triggers = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        triggers.push(decode_trigger(entry, index)?);
    }
    let mut names = Vec::new();
    for trigger in &triggers {
        if names.iter().any(|name| name == &trigger.name) {
            return Err(CliError::plain(format!(
                "Trigger name \"{}\" is duplicated.",
                trigger.name
            )));
        }
        names.push(trigger.name.clone());
    }
    Ok((project_name, programs, triggers))
}

fn read_globs(value: Option<&Toml>) -> Result<Vec<String>, CliError> {
    let invalid = || CliError::plain("`[project].programs` must be a glob or a list of globs.");
    let items = match value {
        Some(Toml::String(value)) => vec![value.clone()],
        Some(Toml::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::trim).map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(invalid)?,
        _ => return Err(invalid()),
    };
    if items.is_empty() || items.iter().any(|item| item.trim().is_empty()) {
        return Err(invalid());
    }
    Ok(items
        .into_iter()
        .map(|item| item.trim().to_string())
        .collect())
}

fn decode_trigger(entry: &Toml, index: usize) -> Result<Trigger, CliError> {
    let Some(row) = entry.as_table() else {
        return Err(CliError::plain(format!(
            "Trigger {} must be a table.",
            index + 1
        )));
    };
    for key in row.keys() {
        if !matches!(
            key.as_str(),
            "name" | "type" | "program" | "schedule" | "timezone" | "input"
        ) {
            return Err(CliError::plain(format!(
                "Unknown field `{key}` in triggers[{index}]."
            )));
        }
    }
    let name = row
        .get("name")
        .and_then(Toml::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| CliError::plain(format!("Trigger {} requires a name.", index + 1)))?
        .to_string();
    let kind = row.get("type").and_then(Toml::as_str).unwrap_or("");
    if kind != "webhook" && kind != "cron" {
        return Err(CliError::plain(format!(
            "Trigger \"{name}\" type must be webhook or cron."
        )));
    }
    let program = row
        .get("program")
        .and_then(Toml::as_str)
        .map(str::trim)
        .filter(|program| !program.is_empty())
        .ok_or_else(|| CliError::plain(format!("Trigger \"{name}\" requires a program.")))?
        .to_string();
    let input = match row.get("input") {
        Some(value) => Some(reject_dates(value, &format!("{name}.input"))?),
        None => None,
    };
    if kind == "webhook" {
        if row.contains_key("schedule") || row.contains_key("timezone") || input.is_some() {
            return Err(CliError::plain(format!(
                "Webhook trigger \"{name}\" cannot set schedule, timezone, or input."
            )));
        }
        return Ok(Trigger {
            name,
            kind: kind.to_string(),
            program,
            schedule: None,
            timezone: None,
            input: None,
        });
    }
    let schedule = row
        .get("schedule")
        .and_then(Toml::as_str)
        .map(str::trim)
        .filter(|schedule| !schedule.is_empty())
        .ok_or_else(|| CliError::plain(format!("Cron trigger \"{name}\" requires a schedule.")))?
        .to_string();
    validate_cron(&schedule, &name)?;
    let timezone = match row.get("timezone") {
        None => "UTC".to_string(),
        Some(Toml::String(value)) => value.trim().to_string(),
        Some(_) => {
            return Err(CliError::plain(format!(
                "Cron trigger \"{name}\" has an invalid timezone."
            )))
        }
    };
    if timezone.is_empty() || !valid_timezone(&timezone) {
        return Err(CliError::plain(format!(
            "Cron trigger \"{name}\" has an invalid timezone."
        )));
    }
    Ok(Trigger {
        name,
        kind: kind.to_string(),
        program,
        schedule: Some(schedule),
        timezone: Some(timezone),
        input,
    })
}

fn validate_cron(schedule: &str, name: &str) -> Result<(), CliError> {
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(CliError::plain(format!(
            "Cron trigger \"{name}\" schedule must have five fields."
        )));
    }
    for (index, field) in fields.iter().enumerate() {
        let (min, max) = CRON_BOUNDS[index];
        validate_cron_field(field, min, max, name)?;
    }
    Ok(())
}

fn validate_cron_field(field: &str, min: i64, max: i64, name: &str) -> Result<(), CliError> {
    let invalid = || CliError::plain(format!("Cron trigger \"{name}\" has an invalid schedule."));
    if field.is_empty() {
        return Err(invalid());
    }
    for item in field.split(',') {
        if item.is_empty() {
            return Err(invalid());
        }
        let mut parts = item.split('/');
        let range = parts.next().unwrap_or("");
        let step = parts.next();
        if parts.next().is_some() || range.is_empty() {
            return Err(invalid());
        }
        if let Some(step) = step {
            if step.parse::<u32>().ok().filter(|step| *step >= 1).is_none() {
                return Err(invalid());
            }
        }
        if range == "*" {
            continue;
        }
        let ends: Vec<&str> = range.split('-').collect();
        if ends.len() > 2 || ends.iter().any(|end| end.parse::<i64>().is_err()) {
            return Err(invalid());
        }
        let numbers: Vec<i64> = ends.iter().map(|end| end.parse().unwrap()).collect();
        if numbers.iter().any(|value| *value < min || *value > max) {
            return Err(invalid());
        }
        if numbers.len() == 2 && numbers[0] > numbers[1] {
            return Err(invalid());
        }
    }
    Ok(())
}

fn valid_timezone(timezone: &str) -> bool {
    if timezone == "UTC" {
        return true;
    }
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg("const zone = process.argv[1]; process.exit(Intl.supportedValuesOf('timeZone').includes(zone) ? 0 : 1)")
        .arg(timezone)
        .output();
    matches!(output, Ok(output) if output.status.success())
}

fn reject_dates(value: &Toml, path: &str) -> Result<Json, CliError> {
    match value {
        Toml::Datetime(_) => Err(CliError::plain(format!(
            "TOML datetimes are not allowed at {path}."
        ))),
        Toml::String(value) => Ok(Json::String(value.clone())),
        Toml::Boolean(value) => Ok(Json::Bool(*value)),
        Toml::Integer(value) => Ok(Json::from(*value)),
        Toml::Float(value) => {
            let number = serde_json::Number::from_f64(*value)
                .ok_or_else(|| CliError::plain(format!("Invalid number at {path}.")))?;
            Ok(Json::Number(number))
        }
        Toml::Array(items) => Ok(Json::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| reject_dates(item, &format!("{path}[{index}]")))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Toml::Table(table) => {
            let mut map = Map::new();
            for (key, item) in table {
                map.insert(key.clone(), reject_dates(item, &format!("{path}.{key}"))?);
            }
            Ok(Json::Object(map))
        }
    }
}
