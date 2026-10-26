use std::io::{BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tcc_ir::{
    Artifact, ConstValue, Envelope, FuncId, Function, HostCapability, Instruction, Program,
    ENGINE_FORMAT_VERSION,
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_trigora-pulse"))
}

fn dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("pulse-start-{}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn artifact() -> String {
    let instructions = vec![
        Instruction::LoadConst {
            value: ConstValue::String("work".into()),
        },
        Instruction::Effect { has_input: false },
        Instruction::Pop,
        Instruction::LoadConst {
            value: ConstValue::Number(1.0),
        },
        Instruction::Return,
    ];
    let spans = vec![None; instructions.len()];
    let artifact = Artifact {
        envelope: Envelope {
            artifact_hash: "program".into(),
            frontend_id: "typescript".into(),
            frontend_version: "0.0.0".into(),
            language_semantics_version: "ts.subset.v1".into(),
            engine_format_version: ENGINE_FORMAT_VERSION,
            required_engine_features: tcc_ir::FeatureSet::current_engine(),
            required_host_capabilities: HostCapability::known()
                .iter()
                .map(|id| HostCapability((*id).to_string()))
                .collect(),
            runtime_modules: Vec::new(),
        },
        program: Program {
            entry: FuncId(0),
            functions: vec![Function {
                id: FuncId(0),
                name: "run".into(),
                param_count: 0,
                local_count: 0,
                param_defaults: Vec::new(),
                instructions,
                spans,
            }],
        },
    };
    tcc_ir::encode_artifact(&artifact).unwrap()
}

fn clear_remote(command: &mut Command) {
    command
        .env_remove("TRIGORA_TOKEN")
        .env_remove("TRIGORA_PULSE_AUTHORITY_URL")
        .env_remove("TRIGORA_PULSE_WORKSPACE_ID")
        .env_remove("TRIGORA_PULSE_WORKSPACE_TOKEN");
}

#[test]
fn missing_token_exits_before_the_socket() {
    let root = dir("closed");
    let artifact_path = root.join("artifact.json");
    std::fs::write(&artifact_path, "{}").unwrap();
    let mut command = Command::new(bin());
    command.arg(&root).arg(&artifact_path);
    clear_remote(&mut command);
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(stderr.contains("durability is not configured"));
    assert!(!root.join("pulse.sock").exists());
}

#[test]
fn whitespace_token_does_not_start_in_memory() {
    let root = dir("blank");
    let artifact_path = root.join("artifact.json");
    std::fs::write(&artifact_path, "{}").unwrap();
    let mut command = Command::new(bin());
    command
        .arg(&root)
        .arg(&artifact_path)
        .env("TRIGORA_TOKEN", "   ");
    clear_remote(&mut command);
    command.env("TRIGORA_TOKEN", "   ");
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr)
        .to_lowercase()
        .contains("durability is not configured"));
    assert!(!root.join("pulse.sock").exists());
}

#[test]
fn version_reports_the_protocol() {
    let output = Command::new(bin()).arg("--version").output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("trigora-pulse 0.1.0 protocol 1"));
}

#[test]
fn missing_token_with_only_a_directory_exits_before_the_socket() {
    let root = dir("closed-dir");
    let mut command = Command::new(bin());
    command.arg(&root);
    clear_remote(&mut command);
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr)
        .to_lowercase()
        .contains("durability is not configured"));
    assert!(!root.join("pulse.sock").exists());
}

#[test]
fn directory_only_binds_and_reports_its_version() {
    let root = dir("listen");
    let mut command = Command::new(bin());
    command
        .arg("--ephemeral")
        .arg(&root)
        .stderr(Stdio::null())
        .stdout(Stdio::null());
    let mut child = command.spawn().unwrap();
    let bearer = wait_file(&root.join("bearer"));
    let mut stream = UnixStream::connect(root.join("pulse.sock")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(
        &mut stream,
        &serde_json::json!({ "type": "auth", "bearer": bearer }),
    );
    let auth = read_frame(&mut stream);
    assert_eq!(auth["type"], "authOk");
    assert_eq!(auth["version"], "0.1.0");
    assert_eq!(auth["protocol"], 1);
    drop(stream);
    let check = Command::new(bin())
        .arg("--check")
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let _ = nix_kill(child.id(), libc::SIGTERM);
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn ephemeral_warns_and_listens() {
    let root = dir("ephemeral");
    let artifact_path = root.join("artifact.json");
    std::fs::write(&artifact_path, artifact()).unwrap();
    let mut command = Command::new(bin());
    command
        .arg("--ephemeral")
        .arg(&root)
        .arg(&artifact_path)
        .env("TRIGORA_TOKEN", "present")
        .stderr(Stdio::piped())
        .stdout(Stdio::null());
    let mut child = command.spawn().unwrap();
    let stderr = child.stderr.take().unwrap();
    let text = Arc::new(Mutex::new(String::new()));
    let captured = Arc::clone(&text);
    std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut buf = [0u8; 256];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            captured
                .lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    });
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        let seen = text.lock().unwrap().clone();
        if seen.contains("Pulse running in ephemeral mode.")
            && seen.contains("Execution state will be lost when the host exits.")
            && root.join("pulse.sock").exists()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let text = text.lock().unwrap().clone();
    assert!(text.contains("Pulse running in ephemeral mode."));
    assert!(text.contains("Execution state will be lost when the host exits."));
    assert!(root.join("pulse.sock").exists());
    let _ = nix_kill(child.id(), libc::SIGTERM);
    let finished = Instant::now();
    let status = child.wait().unwrap();
    assert!(finished.elapsed() < Duration::from_secs(12));
    assert!(status.success());
}

#[test]
fn signal_during_an_effect_exits_within_the_deadline() {
    let root = dir("signal");
    let artifact_path = root.join("artifact.json");
    let encoded = artifact();
    std::fs::write(&artifact_path, &encoded).unwrap();
    let mut command = Command::new(bin());
    command
        .arg("--ephemeral")
        .arg(&root)
        .arg(&artifact_path)
        .env("TRIGORA_PULSE_EFFECT_TIMEOUT_MS", "60000")
        .stderr(Stdio::null())
        .stdout(Stdio::null());
    clear_remote(&mut command);
    let mut child = command.spawn().unwrap();
    let bearer = wait_file(&root.join("bearer"));
    let mut stream = UnixStream::connect(root.join("pulse.sock")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write_frame(
        &mut stream,
        &serde_json::json!({ "type": "auth", "bearer": bearer }),
    );
    let auth = read_frame(&mut stream);
    assert_eq!(auth["type"], "authOk");
    write_frame(
        &mut stream,
        &serde_json::json!({
            "type": "register",
            "artifactHash": "program",
            "keys": ["work"],
            "artifact": encoded,
        }),
    );
    let registered = read_frame(&mut stream);
    assert_eq!(registered["type"], "registered", "{registered}");
    write_frame(
        &mut stream,
        &serde_json::json!({ "type": "start", "executionId": "blocked" }),
    );
    let started = Instant::now();
    let mut saw_effect = false;
    while started.elapsed() < Duration::from_secs(5) {
        let frame = read_frame(&mut stream);
        if frame["type"] == "effect" {
            saw_effect = true;
            break;
        }
    }
    assert!(saw_effect, "the effect was not dispatched");
    let signaled = Instant::now();
    let _ = nix_kill(child.id(), libc::SIGTERM);
    let status = child.wait().unwrap();
    assert!(
        signaled.elapsed() < Duration::from_secs(12),
        "shutdown exceeded the deadline"
    );
    assert!(status.code().is_some());
}

fn wait_file(path: &std::path::Path) -> String {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if let Ok(text) = std::fs::read_to_string(path) {
            if !text.is_empty() {
                return text;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("missing {}", path.display());
}

fn write_frame(stream: &mut UnixStream, value: &serde_json::Value) {
    let body = serde_json::to_vec(value).unwrap();
    stream
        .write_all(&(body.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&body).unwrap();
}

fn read_frame(stream: &mut UnixStream) -> serde_json::Value {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len).unwrap();
    let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
    stream.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[test]
fn signal_during_an_effect_stops_renewals_without_a_commit() {
    let authority = TestServer::start();
    let root = PathBuf::from("/tmp").join(format!("pulse-renew-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let artifact_path = root.join("artifact.json");
    let encoded = artifact();
    std::fs::write(&artifact_path, &encoded).unwrap();
    let mut command = Command::new(bin());
    command
        .arg(&root)
        .arg(&artifact_path)
        .env("TRIGORA_PULSE_EFFECT_TIMEOUT_MS", "60000")
        .env("TRIGORA_PULSE_LEASE_MS", "400")
        .env("TRIGORA_PULSE_AUTHORITY_URL", &authority.url)
        .env("TRIGORA_PULSE_WORKSPACE_ID", "ws")
        .env("TRIGORA_PULSE_WORKSPACE_TOKEN", "workspace-token")
        .env_remove("TRIGORA_TOKEN")
        .stderr(Stdio::null())
        .stdout(Stdio::null());
    let mut child = command.spawn().unwrap();
    let bearer = wait_file(&root.join("bearer"));
    let mut stream = UnixStream::connect(root.join("pulse.sock")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write_frame(
        &mut stream,
        &serde_json::json!({ "type": "auth", "bearer": bearer }),
    );
    assert_eq!(read_frame(&mut stream)["type"], "authOk");
    write_frame(
        &mut stream,
        &serde_json::json!({
            "type": "register",
            "artifactHash": "program",
            "keys": ["work"],
            "artifact": encoded,
        }),
    );
    assert_eq!(read_frame(&mut stream)["type"], "registered");
    write_frame(
        &mut stream,
        &serde_json::json!({ "type": "start", "executionId": "blocked" }),
    );
    let started = Instant::now();
    let mut saw_effect = false;
    while started.elapsed() < Duration::from_secs(5) {
        if read_frame(&mut stream)["type"] == "effect" {
            saw_effect = true;
            break;
        }
    }
    assert!(saw_effect, "the effect was not dispatched");
    let renewing = Instant::now();
    let mut renewals = 0u64;
    while renewing.elapsed() < Duration::from_secs(5) {
        renewals = authority.renewals("blocked");
        if renewals >= 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert!(renewals >= 1, "the lease was not renewed");
    let signaled = Instant::now();
    let _ = nix_kill(child.id(), libc::SIGTERM);
    let status = wait_bounded(&mut child, Duration::from_secs(12));
    assert!(
        signaled.elapsed() < Duration::from_secs(12),
        "shutdown exceeded the deadline"
    );
    assert!(status.code().is_some());
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(authority.renewals("blocked"), renewals);
    let version = authority.version("blocked");
    assert!(
        version.contains("\"revision\":0"),
        "a commit landed after shutdown: {version}"
    );
}

struct TestServer {
    child: std::process::Child,
    url: String,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl TestServer {
    fn start() -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../trigora-platform/apps/pulse-authority/src/serve.ts");
        let mut child = Command::new("node")
            .arg("--experimental-strip-types")
            .arg(&script)
            .env("PULSE_TEST_HOOKS", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        let started = Instant::now();
        loop {
            if started.elapsed() > Duration::from_secs(10) {
                panic!("pulse authority did not listen");
            }
            line.clear();
            if reader.read_line(&mut line).unwrap() == 0 {
                panic!("pulse authority exited");
            }
            if let Some(port) = line.trim().strip_prefix("listening ") {
                return Self {
                    child,
                    url: format!("http://127.0.0.1:{port}"),
                };
            }
        }
    }

    fn renewals(&self, execution: &str) -> u64 {
        let body = http_get(&format!("{}/v1/test/renewals/{execution}", self.url), None);
        body.trim().parse().unwrap_or(0)
    }

    fn version(&self, execution: &str) -> String {
        http_get(
            &format!(
                "{}/v1/workspaces/ws/executions/{execution}/sql-version",
                self.url
            ),
            Some("workspace-token"),
        )
    }
}

fn http_get(url: &str, bearer: Option<&str>) -> String {
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_once('/').unwrap();
    let mut stream = std::net::TcpStream::connect(host).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let auth = bearer
        .map(|token| format!("authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "GET /{path} HTTP/1.1\r\nhost: {host}\r\n{auth}connection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default()
}

fn wait_bounded(child: &mut std::process::Child, limit: Duration) -> std::process::ExitStatus {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if started.elapsed() > limit {
            let _ = nix_kill(child.id(), libc::SIGKILL);
            panic!("shutdown exceeded the deadline");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn nix_kill(pid: u32, sig: i32) -> i32 {
    unsafe { libc::kill(pid as i32, sig) }
}
