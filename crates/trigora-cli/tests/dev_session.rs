#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static SESSION: Mutex<()> = Mutex::new(());

fn session() -> std::sync::MutexGuard<'static, ()> {
    SESSION.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[test]
fn dev_starts_reloads_and_reaps_children() {
    let _session = session();
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_trigora"));
    let debug = cli.parent().expect("debug dir");
    let local = debug.join(if cfg!(windows) {
        "trigora-local.exe"
    } else {
        "trigora-local"
    });
    if !local.is_file() {
        let status = Command::new("cargo")
            .args(["build", "-p", "trigora-local", "--target-dir"])
            .arg(debug.parent().expect("target"))
            .status()
            .expect("cargo");
        assert!(status.success(), "trigora-local failed to build");
    }
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/cli/helper/node-helper.js");
    assert!(helper.is_file(), "node helper is missing");
    let root = std::env::temp_dir().join(format!("trigora-dev-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("trigora.toml"),
        "[project]\nname = \"demo\"\nprograms = [\"src/**/*.ts\"]\n",
    )
    .unwrap();
    let program = root.join("src/program.ts");
    std::fs::write(
        &program,
        "export default async function program() {\n  return 1;\n}\n",
    )
    .unwrap();
    let stub = root.join("tcc-rust-compile");
    std::fs::write(&stub, "#!/bin/sh\nprintf '%s\\n' '{}'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&stub).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&stub, permissions).unwrap();
    }
    let node_modules =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/cli/node_modules");
    let mut child = Command::new(&cli)
        .arg("dev")
        .current_dir(&root)
        .env("TRIGORA_LOCAL_BIN", &local)
        .env("TRIGORA_NODE_HELPER", &helper)
        .env("TRIGORA_RUST_COMPILER_BIN", &stub)
        .env("NODE_PATH", &node_modules)
        .env_remove("TRIGORA_TOKEN")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn trigora dev");
    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");
    let (tx, rx) = std::sync::mpsc::channel();
    let tx_err = tx.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let _ = tx.send(line);
        }
    });
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let _ = tx_err.send(line);
        }
    });
    let mut lines = Vec::new();
    let ready_at = Instant::now();
    let mut runtime_port = None;
    while ready_at.elapsed() < Duration::from_secs(40) {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(200)) {
            if let Some(port) = line.split("http://127.0.0.1:").nth(1) {
                runtime_port = port
                    .split_whitespace()
                    .next()
                    .and_then(|value| value.trim_end_matches('.').parse().ok());
            }
            lines.push(line);
            if lines
                .iter()
                .any(|line| line.contains("Local runtime ready"))
                && runtime_port.is_some()
            {
                break;
            }
        }
        if child.try_wait().ok().flatten().is_some() {
            while let Ok(line) = rx.try_recv() {
                lines.push(line);
            }
            break;
        }
    }
    let joined = lines.join("\n");
    assert!(
        joined.contains("Local runtime ready"),
        "dev did not become ready:\n{joined}"
    );
    std::fs::write(
        &program,
        "export default async function program() {\n  return 2;\n}\n",
    )
    .unwrap();
    let reload_at = Instant::now();
    let mut reloaded = joined.contains("Reloaded");
    while !reloaded && reload_at.elapsed() < Duration::from_secs(5) {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(200)) {
            reloaded = line.contains("Reloaded");
            lines.push(line);
        }
    }
    assert!(reloaded, "dev did not reload:\n{}", lines.join("\n"));
    let commands = process_list();
    let helpers = commands
        .lines()
        .filter(|line| line.contains(&helper.display().to_string()))
        .count();
    assert_eq!(helpers, 1, "expected one Node helper:\n{commands}");
    terminate(&mut child);
    let status = child.wait().expect("wait");
    assert!(status.success(), "dev exit status {status}");
    let port = runtime_port.expect("port");
    let closed = TcpStream::connect(("127.0.0.1", port)).is_err();
    assert!(closed, "trigora-local is still listening on {port}");
    let commands = process_list();
    assert!(
        !commands.contains(&local.display().to_string()),
        "trigora-local was not reaped"
    );
    assert!(
        !commands.contains(&helper.display().to_string()),
        "node helper was not reaped"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn python_dev_reaches_ready_without_node() {
    let _session = session();
    let (cli, local) = binaries();
    let root = std::env::temp_dir().join(format!("trigora-py-dev-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("trigora.toml"),
        "[project]\nname = \"demo\"\nprograms = [\"src/**/*.py\"]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/program.py"),
        "from trigora import program\n\n@program\nasync def program():\n    return 1\n",
    )
    .unwrap();
    let engine =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tcc-engine/bindings/python/python");
    let (mut child, joined, port) = run_until_ready(
        &cli,
        &root,
        Command::new(&cli)
            .arg("dev")
            .current_dir(&root)
            .env("TRIGORA_LOCAL_BIN", &local)
            .env("PYTHONPATH", &engine)
            .env("PATH", path_without_node())
            .env_remove("TRIGORA_NODE_HELPER")
            .env_remove("TRIGORA_TOKEN"),
    );
    assert!(
        joined.contains("Local runtime ready"),
        "python dev did not become ready:\n{joined}"
    );
    assert!(!joined.contains("node-helper"));
    stop(&mut child, port, &local);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rust_dev_reaches_ready_without_node() {
    let _session = session();
    let (cli, local) = binaries();
    let root = std::env::temp_dir().join(format!("trigora-rs-dev-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("trigora.toml"),
        "[project]\nname = \"demo\"\nprograms = [\"src/**/*.rs\"]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/program.rs"),
        "pub async fn main() -> Result<String, String> {\n    Ok(String::from(\"ok\"))\n}\n",
    )
    .unwrap();
    let compiler = rust_compiler(&root);
    let (mut child, joined, port) = run_until_ready(
        &cli,
        &root,
        Command::new(&cli)
            .arg("dev")
            .current_dir(&root)
            .env("TRIGORA_LOCAL_BIN", &local)
            .env("TRIGORA_RUST_COMPILER_BIN", &compiler)
            .env("PATH", path_without_node())
            .env_remove("TRIGORA_NODE_HELPER")
            .env_remove("TRIGORA_TOKEN"),
    );
    assert!(
        joined.contains("Local runtime ready"),
        "rust dev did not become ready:\n{joined}"
    );
    assert!(!joined.contains("node-helper"));
    stop(&mut child, port, &local);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn typescript_dev_runs_an_effect_then_an_event() {
    let _session = session();
    let (cli, local) = binaries();
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/cli/helper/node-helper.js");
    let root = std::env::temp_dir().join(format!("trigora-effect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("trigora.toml"),
        "[project]\nname = \"demo\"\nprograms = [\"src/**/*.ts\"]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/program.ts"),
        "import { effect, waitForEvent } from '@trigora/sdk';\nexport default async function program() {\n  const result = await effect('generate', () => 42);\n  const approval = await waitForEvent('approved');\n  return { result, approval };\n}\n",
    )
    .unwrap();
    let node_modules =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/cli/node_modules");
    let (mut child, joined, port) = run_until_ready(
        &cli,
        &root,
        Command::new(&cli)
            .arg("dev")
            .current_dir(&root)
            .env("TRIGORA_LOCAL_BIN", &local)
            .env("TRIGORA_NODE_HELPER", &helper)
            .env("NODE_PATH", &node_modules)
            .env_remove("TRIGORA_TOKEN"),
    );
    assert!(
        joined.contains("Local runtime ready"),
        "dev did not become ready:\n{joined}"
    );
    let started = http(
        port,
        "POST",
        "/v1/executions",
        "{\"programId\":\"program\",\"input\":{}}",
    );
    let id = started
        .split("\"id\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or("")
        .to_string();
    assert!(id.starts_with("exec_"), "start failed:\n{started}");
    let mut waiting = String::new();
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(15) {
        waiting = http(port, "GET", &format!("/v1/executions/{id}"), "");
        if waiting.contains("\"status\":\"waiting\"") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        waiting.contains("\"status\":\"waiting\""),
        "execution did not wait:\n{waiting}"
    );
    let _sent = http(
        port,
        "POST",
        &format!("/v1/executions/{id}/events"),
        "{\"name\":\"approved\",\"payload\":\"ok\"}",
    );
    let mut result = String::new();
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(15) {
        result = http(port, "GET", &format!("/v1/executions/{id}/result"), "");
        if result.contains("\"status\":\"completed\"") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        result.contains("\"status\":\"completed\""),
        "result:\n{result}"
    );
    assert!(result.contains("42"), "result:\n{result}");
    let started = http(
        port,
        "POST",
        "/v1/executions",
        "{\"programId\":\"program\",\"input\":{}}",
    );
    let cancel_id = started
        .split("\"id\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or("")
        .to_string();
    let mut waiting = String::new();
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(15) {
        waiting = http(port, "GET", &format!("/v1/executions/{cancel_id}"), "");
        if waiting.contains("\"status\":\"waiting\"") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(waiting.contains("\"status\":\"waiting\""), "{waiting}");
    let _ = http(
        port,
        "POST",
        &format!("/v1/executions/{cancel_id}/cancel"),
        "",
    );
    let cancelled = http(
        port,
        "GET",
        &format!("/v1/executions/{cancel_id}/result"),
        "",
    );
    assert!(
        cancelled.contains("\"status\":\"cancelled\""),
        "cancel result:\n{cancelled}"
    );
    stop(&mut child, port, &local);
    let _ = std::fs::remove_dir_all(&root);
}

fn http(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

fn binaries() -> (PathBuf, PathBuf) {
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_trigora"));
    let debug = cli.parent().expect("debug dir");
    let local = debug.join(if cfg!(windows) {
        "trigora-local.exe"
    } else {
        "trigora-local"
    });
    if !local.is_file() {
        let status = Command::new("cargo")
            .args(["build", "-p", "trigora-local", "--target-dir"])
            .arg(debug.parent().expect("target"))
            .status()
            .expect("cargo");
        assert!(status.success(), "trigora-local failed to build");
    }
    (cli, local)
}

fn rust_compiler(root: &std::path::Path) -> PathBuf {
    let release = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tcc-engine/target/release/tcc-rust-compile");
    if release.is_file() {
        let version = Command::new(&release)
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .unwrap_or_default();
        if version == "26.10.1" {
            return release;
        }
    }
    let stub = root.join("tcc-rust-compile");
    std::fs::write(
        &stub,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf '%s\\n' '26.10.1'; exit 0; fi\nprintf '%s\\n' '{\"envelope\":{\"artifact_hash\":\"abc\",\"frontend_version\":\"26.10.1\"},\"program\":{\"entry\":0,\"functions\":[{\"id\":0,\"name\":\"main\"}]}}'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&stub).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&stub, permissions).unwrap();
    }
    stub
}

fn path_without_node() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("trigora-bin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let python = String::from_utf8(
        Command::new("which")
            .arg("python3")
            .output()
            .expect("which")
            .stdout,
    )
    .unwrap();
    let target = dir.join("python3");
    let _ = std::fs::remove_file(&target);
    std::os::unix::fs::symlink(python.trim(), &target).unwrap();
    dir
}

fn run_until_ready(
    _cli: &std::path::Path,
    _root: &std::path::Path,
    command: &mut Command,
) -> (std::process::Child, String, u16) {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn trigora dev");
    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");
    let (tx, rx) = std::sync::mpsc::channel();
    let tx_err = tx.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let _ = tx.send(line);
        }
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            let _ = tx_err.send(line);
        }
    });
    let mut lines = Vec::new();
    let mut port = None;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(40) {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(200)) {
            if let Some(value) = line.split("http://127.0.0.1:").nth(1) {
                port = value
                    .split_whitespace()
                    .next()
                    .and_then(|value| value.trim_end_matches('.').parse().ok());
            }
            lines.push(line);
            if lines
                .iter()
                .any(|line| line.contains("Local runtime ready"))
                && port.is_some()
            {
                break;
            }
        }
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
    }
    (child, lines.join("\n"), port.unwrap_or(0))
}

fn process_list() -> String {
    let listing = Command::new("ps")
        .args(["-ax", "-o", "args="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&listing.stdout).into_owned()
}

fn terminate(child: &mut std::process::Child) {
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
}

fn stop(child: &mut std::process::Child, port: u16, local: &std::path::Path) {
    terminate(child);
    let status = child.wait().expect("wait");
    assert!(status.success(), "dev exit status {status}");
    if port != 0 {
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    }
    let commands = process_list();
    assert!(!commands.contains(&local.display().to_string()));
    assert!(!commands.contains("node-helper.js"));
}
