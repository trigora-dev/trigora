use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;

use serde_json::{json, Value as Json};

use crate::cli::Invocation;
use crate::commands::{self, local_endpoint};
use crate::config::load_config;
use crate::deploy::sync_deployment;
use crate::model::{program_slug, Effect, Program};
use crate::parse_invocation;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn arg(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn remote_flag_has_three_executions_shapes() {
    assert!(matches!(
        parse_invocation(&arg(&["programs"])).unwrap(),
        Invocation::Programs { remote: false }
    ));
    assert!(matches!(
        parse_invocation(&arg(&["programs", "--remote"])).unwrap(),
        Invocation::Programs { remote: true }
    ));
    assert!(matches!(
        parse_invocation(&arg(&["executions"])).unwrap(),
        Invocation::Executions { remote: false }
    ));
    assert!(matches!(
        parse_invocation(&arg(&["executions", "--remote"])).unwrap(),
        Invocation::Executions { remote: true }
    ));
    assert!(matches!(
        parse_invocation(&arg(&["executions", "inspect", "ex_123", "--remote"])).unwrap(),
        Invocation::Inspect { remote: true, .. }
    ));
    assert!(matches!(
        parse_invocation(&arg(&["executions", "--remote", "inspect", "ex_123"])).unwrap(),
        Invocation::Inspect { remote: true, .. }
    ));
    assert!(matches!(
        parse_invocation(&arg(&["start", "approval", "--remote"])).unwrap(),
        Invocation::Start { remote: true, .. }
    ));
    assert!(matches!(
        parse_invocation(&arg(&["send", "exec_1", "approved", "--remote"])).unwrap(),
        Invocation::Send { remote: true, .. }
    ));
    assert!(matches!(
        parse_invocation(&arg(&["cancel", "exec_1", "--remote"])).unwrap(),
        Invocation::Cancel { remote: true, .. }
    ));
    assert!(parse_invocation(&arg(&["deploy", "--remote"])).is_err());
    assert!(parse_invocation(&arg(&["whoami", "--remote"])).is_err());
}

#[test]
fn local_target_ignores_a_set_token() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("TRIGORA_TOKEN", "secret");
    std::env::set_var("TRIGORA_RUNTIME_URL", "http://127.0.0.1:9");
    let endpoint = local_endpoint();
    assert!(!endpoint.cloud);
    assert!(endpoint.token.is_none());
    assert_eq!(endpoint.base, "http://127.0.0.1:9");
    std::env::remove_var("TRIGORA_RUNTIME_URL");
    let remote = commands::endpoint(true);
    assert!(remote.is_ok());
    assert!(remote.unwrap().token.is_some());
    std::env::remove_var("TRIGORA_TOKEN");
    assert!(commands::endpoint(true).is_err());
}

#[test]
fn program_slug_splits_camel_case() {
    assert_eq!(program_slug("reportNightly").unwrap(), "report-nightly");
    assert!(program_slug("9bad").is_err());
}

#[test]
fn manifest_requires_a_project_name() {
    let error = crate::config::load_config(PathBuf::from("/tmp/does-not-exist-trigora").as_path());
    assert!(error.is_err());
}

#[test]
fn empty_trigger_list_is_put_and_program_filter_does_not_limit_it() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = std::sync::Arc::clone(&seen);
    thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = String::new();
            let mut byte = [0u8; 1];
            while stream.read(&mut byte).ok() == Some(1) {
                buffer.push(byte[0] as char);
                if buffer.contains("\r\n\r\n") {
                    break;
                }
            }
            let length = buffer
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().to_string())
                })
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let mut body = vec![0; length];
            if length > 0 {
                let _ = stream.read_exact(&mut body);
            }
            recorded.lock().unwrap().push((
                buffer.lines().next().unwrap_or("").to_string(),
                String::from_utf8_lossy(&body).to_string(),
            ));
            let response = if buffer.starts_with("GET /v1/projects") {
                r#"{"projects":[{"id":"prj_1","name":"demo"}]}"#
            } else if buffer.starts_with("POST /v1/programs/deploy") {
                r#"{"program":{"name":"report"},"version":{"artifactHash":"abc","language":"typescript"}}"#
            } else {
                r#"{"triggers":[]}"#
            };
            let payload = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response}",
                response.len()
            );
            let _ = stream.write_all(payload.as_bytes());
        }
    });
    let dir = std::env::temp_dir().join(format!("trigora-deploy-{}", port));
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(
        dir.join("trigora.toml"),
        "[project]\nname = \"demo\"\nprograms = [\"src/**/*.ts\"]\n",
    )
    .unwrap();
    let config = load_config(&dir).unwrap();
    let programs = vec![sample("report"), sample("nightly")];
    let endpoint = crate::http::Endpoint {
        base: format!("http://127.0.0.1:{port}"),
        token: Some("token".to_string()),
        cloud: true,
    };
    sync_deployment(&endpoint, &config, &programs, Some("report"), &registry()).unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(seen[0].0.starts_with("GET /v1/projects"));
    assert!(seen[1].0.starts_with("POST /v1/programs/deploy"));
    assert!(seen[1].1.contains("\"name\":\"report\""));
    assert!(!seen
        .iter()
        .any(|(_, body)| body.contains("\"name\":\"nightly\"")));
    assert!(seen[2].0.starts_with("PUT /v1/projects/prj_1/triggers"));
    let put: Json = serde_json::from_str(&seen[2].1).unwrap();
    assert_eq!(put["triggers"], json!([]));
}

#[test]
fn program_filter_still_puts_every_trigger() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let recorded = std::sync::Arc::clone(&seen);
    thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let (start, body) = read_http(&mut stream);
            if start.starts_with("PUT ") {
                *recorded.lock().unwrap() = body.clone();
            }
            let response = if start.starts_with("GET ") {
                r#"{"projects":[{"id":"prj_1","name":"demo"}]}"#
            } else if start.starts_with("POST ") {
                r#"{"program":{"name":"report"},"version":{}}"#
            } else {
                r#"{"triggers":[{"name":"github","type":"webhook"},{"name":"nightly","type":"cron","schedule":"0 0 * * *","timezone":"UTC"}]}"#
            };
            let payload = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response}",
                response.len()
            );
            let _ = stream.write_all(payload.as_bytes());
        }
    });
    let dir = std::env::temp_dir().join(format!("trigora-triggers-{}", port));
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(
        dir.join("trigora.toml"),
        "[project]\nname = \"demo\"\nprograms = [\"src/**/*.ts\"]\n\n[[triggers]]\nname = \"github\"\ntype = \"webhook\"\nprogram = \"report\"\n\n[[triggers]]\nname = \"nightly\"\ntype = \"cron\"\nprogram = \"other\"\nschedule = \"0 0 * * *\"\n",
    )
    .unwrap();
    let config = load_config(&dir).unwrap();
    let programs = vec![sample("report"), sample("other")];
    let endpoint = crate::http::Endpoint {
        base: format!("http://127.0.0.1:{port}"),
        token: Some("token".to_string()),
        cloud: true,
    };
    sync_deployment(&endpoint, &config, &programs, Some("report"), &registry()).unwrap();
    let body = seen.lock().unwrap().clone();
    assert!(body.contains("github"));
    assert!(body.contains("nightly"));
    assert!(body.contains("other") || body.contains("\"program\":\"other\""));
}

#[test]
fn cloud_commands_fail_without_a_token() {
    let _guard = ENV_LOCK.lock().unwrap();
    let previous = std::env::var("TRIGORA_TOKEN").ok();
    std::env::remove_var("TRIGORA_TOKEN");
    let whoami = crate::commands::whoami().unwrap_err();
    assert_eq!(whoami.title, "Not authenticated");
    assert!(whoami
        .details
        .iter()
        .any(|(_, value)| value == "TRIGORA_TOKEN is not set."));
    let deploy = crate::deploy::cloud_endpoint().unwrap_err();
    assert_eq!(deploy.title, "Request failed");
    assert!(deploy
        .details
        .iter()
        .any(|(_, value)| value == "TRIGORA_TOKEN is not set."));
    if let Some(previous) = previous {
        std::env::set_var("TRIGORA_TOKEN", previous);
    }
}

#[test]
fn local_programs_request_has_no_authorization_header() {
    let _guard = ENV_LOCK.lock().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let recorded = std::sync::Arc::clone(&seen);
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = vec![0; 4096];
        let count = stream.read(&mut buffer).unwrap_or(0);
        *recorded.lock().unwrap() = String::from_utf8_lossy(&buffer[..count]).to_string();
        let response = r#"{"programs":[]}"#;
        let payload = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response}",
            response.len()
        );
        let _ = stream.write_all(payload.as_bytes());
    });
    let previous_token = std::env::var("TRIGORA_TOKEN").ok();
    let previous_url = std::env::var("TRIGORA_RUNTIME_URL").ok();
    std::env::set_var("TRIGORA_TOKEN", "secret-token");
    std::env::set_var("TRIGORA_RUNTIME_URL", format!("http://127.0.0.1:{port}"));
    crate::commands::programs(false).unwrap();
    let request = seen.lock().unwrap().clone();
    assert!(request.contains("GET /v1/programs"));
    assert!(!request.to_ascii_lowercase().contains("authorization"));
    restore("TRIGORA_TOKEN", previous_token);
    restore("TRIGORA_RUNTIME_URL", previous_url);
}

#[test]
fn failed_trigger_put_stays_incomplete() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let (start, _body) = read_http(&mut stream);
            let (status, response) = if start.starts_with("PUT ") {
                (500, r#"{"error":{"message":"trigger sync failed"}}"#)
            } else if start.starts_with("GET ") {
                (200, r#"{"projects":[{"id":"prj_1","name":"demo"}]}"#)
            } else {
                (200, r#"{"program":{"name":"report"},"version":{}}"#)
            };
            let payload = format!(
                "HTTP/1.1 {status} OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response}",
                response.len()
            );
            let _ = stream.write_all(payload.as_bytes());
        }
    });
    let dir = std::env::temp_dir().join(format!("trigora-incomplete-{}", port));
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(
        dir.join("trigora.toml"),
        "[project]\nname = \"demo\"\nprograms = [\"src/**/*.ts\"]\n",
    )
    .unwrap();
    let config = load_config(&dir).unwrap();
    let endpoint = crate::http::Endpoint {
        base: format!("http://127.0.0.1:{port}"),
        token: Some("token".to_string()),
        cloud: true,
    };
    let error =
        sync_deployment(&endpoint, &config, &[sample("report")], None, &registry()).unwrap_err();
    assert_eq!(error.title, "Deploy incomplete");
    assert_eq!(
        error.message.as_deref(),
        Some("Programs were uploaded and trigger sync did not finish.")
    );
}

fn read_http(stream: &mut std::net::TcpStream) -> (String, String) {
    let mut reader = BufReader::new(stream);
    let mut start = String::new();
    let _ = reader.read_line(&mut start);
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
            break;
        }
        let lowered = line.to_ascii_lowercase();
        if let Some(value) = lowered.strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; content_length];
    if content_length > 0 {
        let _ = reader.read_exact(&mut body);
    }
    (start, String::from_utf8_lossy(&body).into_owned())
}

fn restore(key: &str, value: Option<String>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

fn registry() -> crate::adapter::Registry {
    crate::adapter::Registry::new(&crate::paths::Tools {
        local_bin: PathBuf::from("missing"),
        node_helper: None,
        rust_compiler: None,
    })
}

fn sample(id: &str) -> Program {
    Program {
        id: id.to_string(),
        export_name: id.to_string(),
        file: format!("src/{id}.ts"),
        language: "typescript".to_string(),
        frontend_id: "typescript".to_string(),
        semantics_version: "ts.subset.v1".to_string(),
        source: String::new(),
        artifact_json: r#"{"envelope":{"artifact_hash":"abc","engine_format_version":1,"language_semantics_version":"ts.subset.v1","frontend_id":"typescript","frontend_version":"0.1.0"},"program":{"entry":0,"functions":[{"id":0,"name":"program"}]}}"#.to_string(),
        artifact_hash: "abc".to_string(),
        compiler_version: "0.1.0".to_string(),
        effects: Vec::<Effect>::new(),
    }
}
