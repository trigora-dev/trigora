use std::fs;
use std::path::Path;

use serde_json::{json, Value as Json};

use crate::env::{self, token_missing};
use crate::error::CliError;
use crate::http::{self, runtime_failure, Endpoint};
use crate::output::{execution_record, executions_table, format_wait, programs_table};

pub fn local_endpoint() -> Endpoint {
    Endpoint {
        base: env::runtime_url(),
        token: None,
        cloud: false,
        project_id: None,
    }
}

pub fn remote_endpoint() -> Result<Endpoint, CliError> {
    let root = std::env::current_dir().map_err(|error| CliError::plain(error.to_string()))?;
    remote_project_context(&root)
}

pub fn remote_project_context(root: &Path) -> Result<Endpoint, CliError> {
    let token = env::api_token().ok_or_else(|| token_missing("Not authenticated"))?;
    let config = crate::config::load_config(root)?;
    let lookup = Endpoint {
        base: env::cloud_url(),
        token: Some(token),
        cloud: true,
        project_id: None,
    };
    let project_id = crate::deploy::project_id(&lookup, &config.project_name)?;
    Ok(Endpoint {
        base: lookup.base,
        token: lookup.token,
        cloud: true,
        project_id: Some(project_id),
    })
}

pub fn endpoint(remote: bool) -> Result<Endpoint, CliError> {
    if remote {
        remote_endpoint()
    } else {
        Ok(local_endpoint())
    }
}

pub fn programs(remote: bool) -> Result<(), CliError> {
    let endpoint = endpoint(remote)?;
    let listed = call(&endpoint, "GET", "/v1/programs", None)?;
    let rows = listed
        .get("programs")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    let table = rows
        .iter()
        .map(|program| {
            (
                program
                    .get("name")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string(),
                program
                    .get("language")
                    .or_else(|| program.pointer("/currentVersion/language"))
                    .and_then(Json::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();
    print!("{}", programs_table(&table, remote));
    Ok(())
}

pub fn executions(remote: bool) -> Result<(), CliError> {
    let endpoint = endpoint(remote)?;
    let listed = call(&endpoint, "GET", "/v1/executions", None)?;
    print!("{}", executions_table(&execution_rows(&listed)));
    Ok(())
}

pub fn inspect(execution: &str, remote: bool) -> Result<(), CliError> {
    let endpoint = endpoint(remote)?;
    let body = call(
        &endpoint,
        "GET",
        &format!("/v1/executions/{}", encode(execution)),
        None,
    )?;
    print!(
        "{}",
        execution_record(&record_fields(body.get("execution").unwrap_or(&body)))
    );
    Ok(())
}

pub fn start(program: &str, input: Option<&str>, remote: bool) -> Result<(), CliError> {
    let input = start_input(input)?;
    let endpoint = endpoint(remote)?;
    let started = call(
        &endpoint,
        "POST",
        "/v1/executions",
        Some(&json!({ "programId": program, "input": input })),
    )?;
    if endpoint.cloud {
        print!(
            "{}",
            execution_record(&record_fields(started.get("execution").unwrap_or(&started)))
        );
        return Ok(());
    }
    let id = started
        .pointer("/execution/id")
        .or_else(|| started.get("id"))
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string();
    let fetched = call(
        &endpoint,
        "GET",
        &format!("/v1/executions/{}", encode(&id)),
        None,
    )?;
    print!(
        "{}",
        execution_record(&record_fields(fetched.get("execution").unwrap_or(&fetched)))
    );
    Ok(())
}

pub fn send(
    execution: &str,
    event: &str,
    payload: Option<&str>,
    remote: bool,
) -> Result<(), CliError> {
    let payload = read_json(payload, true, "Invalid payload")?;
    let endpoint = endpoint(remote)?;
    let body = json!({ "name": event, "payload": payload });
    let sent = call(
        &endpoint,
        "POST",
        &format!("/v1/executions/{}/events", encode(execution)),
        Some(&body),
    )?;
    if endpoint.cloud {
        print!(
            "{}",
            execution_record(&record_fields(sent.get("execution").unwrap_or(&sent)))
        );
        return Ok(());
    }
    let fetched = call(
        &endpoint,
        "GET",
        &format!("/v1/executions/{}", encode(execution)),
        None,
    )?;
    print!(
        "{}",
        execution_record(&record_fields(fetched.get("execution").unwrap_or(&fetched)))
    );
    Ok(())
}

pub fn cancel(execution: &str, remote: bool) -> Result<(), CliError> {
    let endpoint = endpoint(remote)?;
    let cancelled = call(
        &endpoint,
        "POST",
        &format!("/v1/executions/{}/cancel", encode(execution)),
        Some(&json!({})),
    )?;
    if endpoint.cloud {
        print!(
            "{}",
            execution_record(&record_fields(
                cancelled.get("execution").unwrap_or(&cancelled)
            ))
        );
        return Ok(());
    }
    let fetched = call(
        &endpoint,
        "GET",
        &format!("/v1/executions/{}", encode(execution)),
        None,
    )?;
    print!(
        "{}",
        execution_record(&record_fields(fetched.get("execution").unwrap_or(&fetched)))
    );
    Ok(())
}

pub fn result(execution: &str, remote: bool) -> Result<(), CliError> {
    let endpoint = endpoint(remote)?;
    let body = call(
        &endpoint,
        "GET",
        &format!("/v1/executions/{}/result", encode(execution)),
        None,
    )?;
    let record = body.get("result").unwrap_or(&body);
    let mut fields = vec![(
        "Status".to_string(),
        record
            .get("status")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
    )];
    if let Some(value) = record.get("result") {
        if !value.is_null() {
            fields.push(("Result".to_string(), value.to_string()));
        }
    }
    if let Some(message) = record.pointer("/error/message").and_then(Json::as_str) {
        fields.push(("Error".to_string(), message.to_string()));
    }
    print!("{}", execution_record(&fields));
    Ok(())
}

pub fn whoami() -> Result<(), CliError> {
    let endpoint = Endpoint {
        base: env::cloud_url(),
        token: Some(env::api_token().ok_or_else(|| token_missing("Not authenticated"))?),
        cloud: true,
        project_id: None,
    };
    let identity = http::request(&endpoint, "GET", "/v1/whoami", None)
        .map_err(|error| http::cloud_failure(error, "Fetching identity"))?;
    print!("{}", whoami_text(&identity));
    Ok(())
}

fn whoami_text(identity: &Json) -> String {
    let workspace = identity
        .pointer("/workspace/slug")
        .and_then(Json::as_str)
        .unwrap_or("");
    let fields = if identity.get("actorType").and_then(Json::as_str) == Some("api_token") {
        vec![
            ("Workspace".to_string(), workspace.to_string()),
            (
                "Token".to_string(),
                identity
                    .pointer("/token/label")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
            (
                "Status".to_string(),
                identity
                    .pointer("/token/status")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
        ]
    } else {
        vec![
            ("Workspace".to_string(), workspace.to_string()),
            (
                "User".to_string(),
                identity
                    .pointer("/user/email")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
            (
                "Role".to_string(),
                identity
                    .pointer("/workspace/role")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
        ]
    };
    execution_record(&fields)
}

fn call(
    endpoint: &Endpoint,
    method: &str,
    path: &str,
    body: Option<&Json>,
) -> Result<Json, CliError> {
    http::request(endpoint, method, path, body).map_err(|error| {
        if endpoint.cloud {
            http::cloud_failure(error, "Calling Trigora Cloud")
        } else {
            runtime_failure(error)
        }
    })
}

fn execution_rows(body: &Json) -> Vec<Vec<String>> {
    body.get("executions")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|execution| {
            vec![
                execution
                    .get("id")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string(),
                program_label(execution),
                execution
                    .get("status")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string(),
                format_wait(execution.get("wait")),
            ]
        })
        .collect()
}

fn record_fields(execution: &Json) -> Vec<(String, String)> {
    let mut fields = vec![
        (
            "ID".to_string(),
            execution
                .get("id")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string(),
        ),
        ("Program".to_string(), program_label(execution)),
        (
            "Status".to_string(),
            execution
                .get("status")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string(),
        ),
    ];
    let wait = format_wait(execution.get("wait"));
    if !wait.is_empty() {
        fields.push(("Wait".to_string(), wait));
    }
    if let Some(result) = execution.get("result") {
        if !result.is_null() {
            fields.push(("Result".to_string(), result.to_string()));
        }
    }
    if let Some(message) = execution.pointer("/error/message").and_then(Json::as_str) {
        fields.push(("Error".to_string(), message.to_string()));
    }
    fields.push((
        "Created".to_string(),
        execution
            .get("createdAt")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
    ));
    fields.push((
        "Updated".to_string(),
        execution
            .get("updatedAt")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
    ));
    fields
}

fn program_label(execution: &Json) -> String {
    execution
        .get("programName")
        .and_then(Json::as_str)
        .filter(|name| !name.is_empty())
        .or_else(|| execution.get("programId").and_then(Json::as_str))
        .unwrap_or("")
        .to_string()
}

pub(crate) fn start_input(value: Option<&str>) -> Result<Json, CliError> {
    match value {
        None => Ok(json!([])),
        Some(value) => read_json(Some(value), false, "Invalid input"),
    }
}

pub(crate) fn read_json(
    value: Option<&str>,
    default_object: bool,
    title: &str,
) -> Result<Json, CliError> {
    let Some(value) = value else {
        return Ok(if default_object {
            json!({})
        } else {
            Json::Null
        });
    };
    let kind = if title.to_ascii_lowercase().contains("payload") {
        "payload"
    } else {
        "input"
    };
    parse_json(value, kind).map_err(|error| CliError::new(title).detail("Reason", error))
}

fn parse_json(value: &str, kind: &str) -> Result<Json, String> {
    let trimmed = value.trim();
    if trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('"')
        || matches!(trimmed, "null" | "true" | "false")
        || trimmed.starts_with('-') && trimmed.chars().nth(1).is_some_and(|ch| ch.is_ascii_digit())
        || trimmed.starts_with(|ch: char| ch.is_ascii_digit())
    {
        return serde_json::from_str(trimmed).map_err(|_| "Invalid JSON.".to_string());
    }
    let raw = fs::read_to_string(Path::new(trimmed))
        .map_err(|error| format!("Could not read {kind} file \"{trimmed}\": {error}"))?;
    serde_json::from_str(&raw).map_err(|_| format!("\"{trimmed}\" is not valid JSON."))
}

fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
