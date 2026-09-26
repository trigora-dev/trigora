use serde_json::{json, Value as Json};

use crate::compile::rust_effect_wasm;
use crate::config::{ProjectConfig, Trigger};
use crate::env::{self, token_missing};
use crate::error::CliError;
use crate::http::{self, cloud_failure, Endpoint};
use crate::model::{program_slug, Program};

pub fn cloud_endpoint() -> Result<Endpoint, CliError> {
    let token = env::api_token().ok_or_else(|| token_missing("Request failed"))?;
    Ok(Endpoint {
        base: env::cloud_url(),
        token: Some(token),
        cloud: true,
    })
}

pub fn sync_deployment(
    endpoint: &Endpoint,
    config: &ProjectConfig,
    programs: &[Program],
    only: Option<&str>,
    registry: &crate::adapter::Registry,
) -> Result<(), CliError> {
    let missing: Vec<&Trigger> = config
        .triggers
        .iter()
        .filter(|trigger| !programs.iter().any(|program| program.id == trigger.program))
        .collect();
    if !missing.is_empty() {
        let mut error = CliError::new("Trigger program was not discovered");
        for trigger in missing {
            error = error.detail(&trigger.name, &trigger.program);
        }
        return Err(error);
    }
    let selected: Vec<&Program> = match only {
        Some(name) => programs
            .iter()
            .filter(|program| {
                program.id == name || program_slug(&program.id).ok().as_deref() == Some(name)
            })
            .collect(),
        None => programs.iter().collect(),
    };
    if selected.is_empty() {
        return Err(CliError::new("Program not found").detail("Program", only.unwrap_or("")));
    }
    let projects = http::request(endpoint, "GET", "/v1/projects", None)
        .map_err(|error| cloud_failure(error, "Calling Trigora Cloud"))?;
    let project_id = projects
        .pointer("/projects")
        .and_then(Json::as_array)
        .and_then(|projects| {
            projects.iter().find(|project| {
                project.get("name").and_then(Json::as_str) == Some(config.project_name.as_str())
            })
        })
        .and_then(|project| project.get("id").and_then(Json::as_str))
        .ok_or_else(|| CliError::new("Project not found").detail("Project", &config.project_name))?
        .to_string();

    let mut deployed = Vec::new();
    for program in &selected {
        let name = program_slug(&program.id)?;
        let bundle = effect_bundle(program, &config.root, registry)?;
        let artifact = artifact_metadata(program);
        let result = http::request(
            endpoint,
            "POST",
            "/v1/programs/deploy",
            Some(&json!({
                "name": name,
                "artifact": artifact,
                "effectBundle": bundle,
            })),
        )
        .map_err(|error| cloud_failure(error, "Deploying program"))?;
        deployed.push(
            result
                .pointer("/program/name")
                .and_then(Json::as_str)
                .unwrap_or(&name)
                .to_string(),
        );
    }

    let mut triggers = Vec::new();
    for trigger in &config.triggers {
        let mut body = json!({
            "name": trigger.name,
            "type": trigger.kind,
            "program": program_slug(&trigger.program)?,
        });
        if let Some(schedule) = &trigger.schedule {
            body["schedule"] = Json::String(schedule.clone());
        }
        if let Some(timezone) = &trigger.timezone {
            body["timezone"] = Json::String(timezone.clone());
        }
        if let Some(input) = &trigger.input {
            body["input"] = input.clone();
        }
        triggers.push(body);
    }
    let synced = http::request(
        endpoint,
        "PUT",
        &format!("/v1/projects/{project_id}/triggers"),
        Some(&json!({ "triggers": triggers })),
    )
    .map_err(|error| {
        CliError::new("Deploy incomplete")
            .message("Programs were uploaded and trigger sync did not finish.")
            .detail("Programs", "Uploaded")
            .detail("Triggers", "Not synced")
            .detail("Reason", error.message)
    })?;
    let trigger_lines = synced
        .pointer("/triggers")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    print_deploy(&deployed, &trigger_lines);
    Ok(())
}

fn print_deploy(deployed: &[String], triggers: &[Json]) {
    println!();
    println!("✔ Deployed");
    println!();
    println!("Programs");
    for name in deployed {
        println!("  {name}  deployed");
    }
    println!();
    println!("Triggers");
    for trigger in triggers {
        let name = trigger.get("name").and_then(Json::as_str).unwrap_or("");
        let detail = if trigger.get("type").and_then(Json::as_str) == Some("webhook") {
            format!(
                "Webhook  {}",
                trigger.get("url").and_then(Json::as_str).unwrap_or("")
            )
        } else {
            format!(
                "Cron      {} · {}",
                trigger.get("schedule").and_then(Json::as_str).unwrap_or(""),
                trigger
                    .get("timezone")
                    .and_then(Json::as_str)
                    .unwrap_or("UTC")
            )
        };
        println!("  {name}  {detail}");
    }
}

fn artifact_metadata(program: &Program) -> Json {
    let parsed: Json = serde_json::from_str(&program.artifact_json).unwrap_or(Json::Null);
    let envelope = parsed.get("envelope");
    json!({
        "hash": envelope.and_then(|value| value.get("artifact_hash")).and_then(Json::as_str).unwrap_or(&program.artifact_hash),
        "blob": program.artifact_json,
        "engineFormatVersion": envelope.and_then(|value| value.get("engine_format_version")).and_then(Json::as_i64).unwrap_or(1),
        "languageSemanticsVersion": envelope.and_then(|value| value.get("language_semantics_version")).and_then(Json::as_str).unwrap_or("unknown"),
        "frontendId": envelope.and_then(|value| value.get("frontend_id")).and_then(Json::as_str).unwrap_or(&program.language),
        "frontendVersion": envelope.and_then(|value| value.get("frontend_version")).and_then(Json::as_str).unwrap_or(&program.compiler_version),
    })
}

fn effect_bundle(
    program: &Program,
    root: &std::path::Path,
    registry: &crate::adapter::Registry,
) -> Result<Json, CliError> {
    if program.effects.is_empty() {
        return Ok(json!({ "language": program.language, "files": [] }));
    }
    Ok(registry
        .effect_bundle(program, root)?
        .unwrap_or_else(|| json!({ "language": program.language, "files": [] })))
}

pub(crate) fn bundle_manifest(program: &Program) -> Json {
    let keys: Vec<&str> = program
        .effects
        .iter()
        .map(|effect| effect.key.as_str())
        .collect();
    json!({
        "path": "effects.json",
        "contents": json!({ "keys": keys }).to_string(),
    })
}

pub(crate) fn typescript_effect_bundle(program: &Program) -> Json {
    json!({
        "language": "typescript",
        "files": [
            { "path": "worker.js", "contents": javascript_worker(&program.effects), "entrypoint": true },
            bundle_manifest(program),
        ],
    })
}

pub(crate) fn python_effect_bundle(program: &Program) -> Json {
    json!({
        "language": "python",
        "files": [
            { "path": "worker.py", "contents": python_worker(&program.effects), "entrypoint": true },
            bundle_manifest(program),
        ],
    })
}

pub(crate) fn rust_effect_bundle(
    program: &Program,
    root: &std::path::Path,
) -> Result<Json, CliError> {
    let wasm = base64(&rust_effect_wasm(root, program)?.unwrap_or_default());
    Ok(json!({
        "language": "rust",
        "files": [
            { "path": "worker.js", "contents": RUST_WORKER, "entrypoint": true },
            { "path": "effects.wasm", "contents": wasm, "encoding": "base64" },
            bundle_manifest(program),
        ],
    }))
}

fn javascript_worker(effects: &[crate::model::Effect]) -> String {
    let entries = effects
        .iter()
        .map(|effect| {
            format!(
                "  {}: {}",
                json!(effect.key),
                effect
                    .source
                    .clone()
                    .unwrap_or_else(|| "() => {{}}".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        "const handlers = {{\n{entries}\n}};\n\nexport default {{\n  async fetch(request) {{\n    const body = await request.json();\n    const handler = handlers[body.key];\n    if (!handler) {{\n      return Response.json({{ error: \"Unknown effect \" + String(body.key) }}, {{ status: 400 }});\n    }}\n    try {{\n      const result = await handler(body.input);\n      return Response.json({{ result }});\n    }} catch (error) {{\n      const message = error instanceof Error ? error.message : String(error);\n      return Response.json({{ error: message }}, {{ status: 500 }});\n    }}\n  }},\n}};\n"
    )
}

fn python_worker(effects: &[crate::model::Effect]) -> String {
    let entries = effects
        .iter()
        .map(|effect| {
            format!(
                "    {}: {},",
                json!(effect.key),
                effect
                    .source
                    .clone()
                    .unwrap_or_else(|| "lambda: None".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("import json\nfrom js import Response\n\nhandlers = {{\n{entries}\n}}\n\nasync def on_fetch(request):\n    body = await request.json()\n    key = body.get(\"key\")\n    handler = handlers.get(key)\n    if handler is None:\n        return Response.new(\n            json.dumps({{\"error\": \"Unknown effect \" + str(key)}}),\n            {{\"status\": 400, \"headers\": {{\"content-type\": \"application/json\"}}}},\n        )\n    try:\n        result = handler()\n        return Response.new(\n            json.dumps({{\"result\": result}}),\n            {{\"headers\": {{\"content-type\": \"application/json\"}}}},\n        )\n    except Exception as error:\n        return Response.new(\n            json.dumps({{\"error\": str(error)}}),\n            {{\"status\": 500, \"headers\": {{\"content-type\": \"application/json\"}}}},\n        )\n")
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut value = (chunk[0] as u32) << 16;
        if chunk.len() > 1 {
            value |= (chunk[1] as u32) << 8;
        }
        if chunk.len() > 2 {
            value |= chunk[2] as u32;
        }
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

const RUST_WORKER: &str = r#"import wasmModule from "./effects.wasm";

let ready;

function load() {
  if (!ready) {
    ready = WebAssembly.instantiate(wasmModule);
  }
  return ready;
}

export default {
  async fetch(request) {
    let body;
    try {
      body = await request.json();
    } catch {
      return Response.json({ error: "effect request requires key and input" }, { status: 400 });
    }
    if (typeof body?.key !== "string" || !Object.hasOwn(body, "input")) {
      return Response.json({ error: "effect request requires key and input" }, { status: 400 });
    }
    const instance = await load();
    const { alloc, handle, out_len: outLen, memory } = instance.exports;
    const payload = new TextEncoder().encode(JSON.stringify({ key: body.key, input: body.input }));
    const ptr = alloc(payload.byteLength);
    new Uint8Array(memory.buffer, ptr, payload.byteLength).set(payload);
    const outPtr = handle(ptr, payload.byteLength);
    const response = JSON.parse(new TextDecoder().decode(new Uint8Array(memory.buffer, outPtr, outLen())));
    return Response.json(response, { status: response.error ? 400 : 200 });
  },
};
"#;
