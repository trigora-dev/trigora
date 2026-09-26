use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value as Json};
use tcc_ir::{
    encode_artifact, Artifact, ConstValue, EngineFeature, Envelope, FuncId, Function,
    HostCapability, Instruction, LocalId, Program, ENGINE_FORMAT_VERSION, LANGUAGE_SEMANTICS_RUST,
};

use crate::{EffectEndpoint, EventSink, FileProgram, Listeners, LocalRuntime};

fn quiet() -> EventSink {
    Arc::new(|_| {})
}

fn temp_db() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("trigora-local-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("state.db")
}

fn file_program(id: &str, artifact: &Artifact) -> FileProgram {
    let artifact_json = encode_artifact(artifact).unwrap();
    FileProgram {
        id: id.to_string(),
        artifact_hash: artifact.envelope.artifact_hash.clone(),
        artifact_json,
        language: "typescript".to_string(),
        frontend_id: "typescript".to_string(),
        frontend_version: "0.0.0".to_string(),
        language_semantics_version: artifact.envelope.language_semantics_version.clone(),
        engine_format_version: ENGINE_FORMAT_VERSION as i64,
    }
}

fn return_arg(hash: &str, semantics: &str) -> Artifact {
    let instructions = vec![
        Instruction::LoadLocal { local: LocalId(0) },
        Instruction::Return,
    ];
    Artifact {
        envelope: Envelope {
            language_semantics_version: semantics.to_string(),
            ..Envelope::typescript_v1(hash)
        },
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".to_string(),
                param_count: 1,
                local_count: 1,
                param_defaults: Vec::new(),
                spans: vec![None; instructions.len()],
                instructions,
            }],
        },
    }
}

fn zero_arg_rust() -> Artifact {
    let instructions = vec![
        Instruction::LoadConst {
            value: ConstValue::Number(1.0),
        },
        Instruction::Return,
    ];
    Artifact {
        envelope: Envelope {
            language_semantics_version: LANGUAGE_SEMANTICS_RUST.to_string(),
            ..Envelope::typescript_v1("rust-zero")
        },
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".to_string(),
                param_count: 0,
                local_count: 0,
                param_defaults: Vec::new(),
                spans: vec![None; instructions.len()],
                instructions,
            }],
        },
    }
}

fn wait_only() -> Artifact {
    let instructions = vec![
        Instruction::LoadConst {
            value: ConstValue::String("approved".to_string()),
        },
        Instruction::WaitForEvent,
        Instruction::Pop,
        Instruction::LoadConst {
            value: ConstValue::Number(1.0),
        },
        Instruction::Return,
    ];
    let mut envelope = Envelope::typescript_v1("wait-only");
    envelope
        .required_engine_features
        .push(EngineFeature("durable.wait_for_event".to_string()));
    envelope
        .required_host_capabilities
        .push(HostCapability("host.event".to_string()));
    Artifact {
        envelope,
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".to_string(),
                param_count: 0,
                local_count: 0,
                param_defaults: Vec::new(),
                spans: vec![None; instructions.len()],
                instructions,
            }],
        },
    }
}

fn wait_return_arg() -> Artifact {
    let instructions = vec![
        Instruction::LoadConst {
            value: ConstValue::String("approved".to_string()),
        },
        Instruction::WaitForEvent,
        Instruction::Pop,
        Instruction::LoadLocal { local: LocalId(0) },
        Instruction::Return,
    ];
    let mut envelope = Envelope::typescript_v1("wait-arg");
    envelope
        .required_engine_features
        .push(EngineFeature("durable.wait_for_event".to_string()));
    envelope
        .required_host_capabilities
        .push(HostCapability("host.event".to_string()));
    Artifact {
        envelope,
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".to_string(),
                param_count: 1,
                local_count: 1,
                param_defaults: Vec::new(),
                spans: vec![None; instructions.len()],
                instructions,
            }],
        },
    }
}

fn runtime(path: &PathBuf) -> LocalRuntime {
    LocalRuntime::open(path, None, quiet()).unwrap()
}

#[test]
fn seeds_the_default_project() {
    let path = temp_db();
    let runtime = runtime(&path);
    let projects = runtime.list_projects().unwrap();
    assert_eq!(projects["projects"][0]["slug"], "default");
    assert_eq!(projects["projects"][0]["id"], "default");
}

#[test]
fn file_replace_keeps_a_deployed_program_with_a_different_id() {
    let path = temp_db();
    let runtime = runtime(&path);
    let artifact = Artifact::sdk_first_example("deployed-hash");
    runtime
        .deploy(&json!({
            "name": "deployed",
            "artifact": {
                "hash": artifact.envelope.artifact_hash,
                "blob": encode_artifact(&artifact).unwrap(),
                "engineFormatVersion": 1,
                "languageSemanticsVersion": "ts.subset.v1",
                "frontendId": "typescript",
                "frontendVersion": "0.0.0",
            },
            "effectBundle": { "language": "typescript", "files": [] },
        }))
        .unwrap();
    runtime
        .replace_files(vec![file_program(
            "from-file",
            &Artifact::minimal_return("file-hash"),
        )])
        .unwrap();
    let ids: Vec<_> = runtime.list_programs().unwrap()["programs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|program| program["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, vec!["deployed".to_string(), "from-file".to_string()]);

    runtime
        .replace_files(vec![file_program(
            "deployed",
            &Artifact::minimal_return("file-wins"),
        )])
        .unwrap();
    let started = runtime.start(&json!({"programId": "deployed"})).unwrap();
    assert_eq!(started["execution"]["status"], "completed");
}

#[test]
fn nonempty_deployed_bundle_can_be_read_and_cannot_start() {
    let path = temp_db();
    let runtime = runtime(&path);
    let artifact = Artifact::minimal_return("bundle");
    runtime
        .deploy(&json!({
            "name": "effects",
            "artifact": {
                "hash": "bundle",
                "blob": encode_artifact(&artifact).unwrap(),
                "engineFormatVersion": 1,
                "languageSemanticsVersion": "1",
                "frontendId": "typescript",
                "frontendVersion": "0.0.0",
            },
            "effectBundle": {
                "language": "typescript",
                "files": [{ "path": "worker.js", "contents": "export {}" }],
            },
        }))
        .unwrap();
    assert_eq!(
        runtime.get_program("effects").unwrap()["program"]["id"],
        "effects"
    );
    let error = runtime.start(&json!({"programId": "effects"})).unwrap_err();
    assert_eq!(
        error.message,
        "Local deploy execution does not support deployed effect bundles yet."
    );
}

#[test]
fn fresh_start_binds_normalized_input_and_resume_does_not() {
    let path = temp_db();
    let runtime = runtime(&path);
    runtime
        .replace_files(vec![file_program(
            "echo",
            &return_arg("echo", "ts.subset.v1"),
        )])
        .unwrap();
    let from_array = runtime
        .start(&json!({"programId": "echo", "input": [7]}))
        .unwrap();
    assert_eq!(from_array["execution"]["result"], 7);
    assert_eq!(from_array["execution"]["input"], json!([7]));

    let from_value = runtime
        .start(&json!({"programId": "echo", "input": 9}))
        .unwrap();
    assert_eq!(from_value["execution"]["result"], 9);

    let omitted = runtime.start(&json!({"programId": "echo"})).unwrap();
    assert_eq!(omitted["execution"]["input"], Json::Null);
    assert_eq!(omitted["execution"]["result"], Json::Null);

    runtime
        .replace_files(vec![
            file_program("echo", &return_arg("echo", "ts.subset.v1")),
            file_program("wait", &wait_return_arg()),
        ])
        .unwrap();
    let waiting = runtime
        .start(&json!({"programId": "wait", "input": [7]}))
        .unwrap();
    assert_eq!(waiting["execution"]["status"], "waiting");
    let resumed = runtime
        .send(
            waiting["execution"]["id"].as_str().unwrap(),
            &json!({"name": "approved", "payload": "replaced"}),
        )
        .unwrap();
    assert_eq!(resumed["execution"]["status"], "completed");
    assert_eq!(resumed["execution"]["result"], 7);
}

#[test]
fn rust_rejects_an_object_passed_to_a_zero_arg_program() {
    let path = temp_db();
    let runtime = runtime(&path);
    runtime
        .replace_files(vec![file_program("main", &zero_arg_rust())])
        .unwrap();
    let error = runtime
        .start(&json!({"programId": "main", "input": {}}))
        .unwrap_err();
    assert!(
        error.message.contains("positional arguments"),
        "{}",
        error.message
    );
}

#[test]
fn effect_callback_receives_program_and_execution_ids() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_thread = Arc::clone(&seen);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 2048];
        loop {
            let read = stream.read(&mut chunk).unwrap_or(0);
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&buffer);
                let length = header.lines().find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().to_string())
                });
                if let Some(length) = length {
                    let length: usize = length.parse().unwrap_or(0);
                    let body_at = buffer
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .unwrap()
                        + 4;
                    if buffer.len() >= body_at + length {
                        seen_thread.lock().unwrap().push(
                            String::from_utf8_lossy(&buffer[body_at..body_at + length]).to_string(),
                        );
                        break;
                    }
                }
            }
        }
        let body = br#"{"value":42}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    });

    let path = temp_db();
    let runtime = LocalRuntime::open(
        &path,
        Some(EffectEndpoint {
            url: format!("http://127.0.0.1:{port}/effects"),
            secret: "effect-secret".to_string(),
        }),
        quiet(),
    )
    .unwrap();
    runtime
        .replace_files(vec![file_program(
            "approval",
            &Artifact::sdk_first_example("approval-hash"),
        )])
        .unwrap();
    let started = runtime
        .start(&json!({"programId": "approval", "input": {}}))
        .unwrap();
    assert_eq!(started["execution"]["status"], "waiting");
    assert_eq!(
        started["execution"]["wait"],
        json!({"type": "event", "event": "approved"})
    );
    let body: Json = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(body["programId"], "approval");
    assert_eq!(body["executionId"], started["execution"]["id"]);
    assert_eq!(body["key"], "generate");

    let id = started["execution"]["id"].as_str().unwrap().to_string();
    let resumed = runtime
        .send(&id, &json!({"name": "approved", "payload": "ok"}))
        .unwrap();
    assert_eq!(resumed["execution"]["status"], "completed");
    assert_eq!(
        resumed["execution"]["result"],
        json!({"approval": "ok", "result": 42})
    );

    let reopened = LocalRuntime::open(&path, None, quiet()).unwrap();
    reopened
        .replace_files(vec![file_program(
            "approval",
            &Artifact::sdk_first_example("approval-hash"),
        )])
        .unwrap();
    assert_eq!(reopened.restored_waiting().unwrap(), 0);
    let listed = reopened.list_executions().unwrap();
    assert_eq!(listed["executions"][0]["id"], id);
    assert_eq!(listed["executions"][0]["status"], "completed");
}

#[test]
fn projects_and_deploys_survive_reopen() {
    let path = temp_db();
    let first = runtime(&path);
    let project = first.create_project(&json!({"name": "billing"})).unwrap();
    let artifact = wait_only();
    first
        .deploy(&json!({
            "name": "wait",
            "artifact": {
                "hash": "wait-only",
                "blob": encode_artifact(&artifact).unwrap(),
                "engineFormatVersion": 1,
                "languageSemanticsVersion": "ts.subset.v1",
                "frontendId": "typescript",
                "frontendVersion": "0.0.0",
            },
            "effectBundle": { "language": "typescript", "files": [] },
        }))
        .unwrap();
    drop(first);

    let second = runtime(&path);
    let slugs: Vec<_> = second.list_projects().unwrap()["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["slug"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        slugs,
        vec![
            "default".to_string(),
            project["project"]["slug"].as_str().unwrap().to_string()
        ]
    );
    assert_eq!(second.list_programs().unwrap()["programs"][0]["id"], "wait");
    assert_eq!(
        second.list_versions("wait").unwrap()["versions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let started = second.start(&json!({"programId": "wait"})).unwrap();
    assert_eq!(started["execution"]["status"], "waiting");
    assert_eq!(second.restored_waiting().unwrap(), 1);
}

#[test]
fn control_requests_without_a_token_are_rejected() {
    let path = temp_db();
    let runtime = Arc::new(runtime(&path));
    let listeners = Listeners::spawn(Arc::clone(&runtime), "127.0.0.1", 0).unwrap();
    assert_ne!(listeners.port, listeners.control_port);

    let rejected = http(
        listeners.control_port,
        "POST /programs HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
    );
    assert!(rejected.starts_with("HTTP/1.1 401"), "{rejected}");

    let accepted = http(
        listeners.control_port,
        &format!(
            "POST /programs HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{{\"programs\":[]}}",
            listeners.control_token
        ),
    );
    assert!(accepted.starts_with("HTTP/1.1 200"), "{accepted}");

    let health = http(
        listeners.port,
        "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(health.contains("\"ok\":true"), "{health}");
    drop(listeners);
}

#[test]
fn waiting_execution_survives_reopen_and_then_resumes() {
    let path = temp_db();
    let first = runtime(&path);
    first
        .replace_files(vec![file_program("wait", &wait_only())])
        .unwrap();
    let started = first.start(&json!({"programId": "wait"})).unwrap();
    let id = started["execution"]["id"].as_str().unwrap().to_string();
    drop(first);

    let second = runtime(&path);
    second
        .replace_files(vec![file_program("wait", &wait_only())])
        .unwrap();
    assert_eq!(second.restored_waiting().unwrap(), 1);
    let resumed = second
        .send(&id, &json!({"name": "approved", "payload": "ok"}))
        .unwrap();
    assert_eq!(resumed["execution"]["status"], "completed");
    assert_eq!(resumed["execution"]["result"], 1);
}

#[test]
fn rejects_a_mismatched_event_and_cancels_a_wait() {
    let path = temp_db();
    let runtime = runtime(&path);
    runtime
        .replace_files(vec![file_program("wait", &wait_only())])
        .unwrap();
    let started = runtime.start(&json!({"programId": "wait"})).unwrap();
    let id = started["execution"]["id"].as_str().unwrap();
    let mismatch = runtime
        .send(id, &json!({"name": "rejected", "payload": "ok"}))
        .unwrap_err();
    assert_eq!(mismatch.code, "event_mismatch");

    let missing = runtime.start(&json!({"programId": "missing"})).unwrap_err();
    assert_eq!(missing.code, "program_not_found");

    let cancelled = runtime.cancel(id).unwrap();
    assert_eq!(cancelled["execution"]["status"], "cancelled");
    let after = runtime
        .send(id, &json!({"name": "approved", "payload": "ok"}))
        .unwrap_err();
    assert_eq!(after.code, "execution_not_waiting");
    let listed = runtime.list_executions().unwrap();
    assert_eq!(listed["executions"][0]["id"], id);
}

fn http(port: u16, request: &str) -> String {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}
