use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn cli() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_trigora"))
}

fn helper() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/cli/helper/node-helper.js")
}

fn pulse_bin() -> PathBuf {
    let cli = cli();
    let debug = cli.parent().expect("debug dir");
    let binary = debug.join("trigora-pulse");
    if !binary.is_file() {
        let status = Command::new("cargo")
            .args(["build", "-p", "trigora-pulse", "--target-dir"])
            .arg(debug.parent().expect("target"))
            .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .status()
            .expect("cargo");
        assert!(status.success(), "trigora-pulse failed to build");
    }
    binary
}

fn project(label: &str) -> PathBuf {
    let root = PathBuf::from("/tmp").join(format!("tp-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("trigora.toml"),
        "[project]\nname = \"demo\"\nprograms = [\"src/program.ts\"]\n",
    )
    .unwrap();
    root
}

fn write_program(root: &std::path::Path) {
    std::fs::write(
        root.join("src/quote.ts"),
        "export function quote(): number {\n  return 17;\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/program.ts"),
        r#"import { effect } from "@trigora/sdk";
import { writeFileSync } from "node:fs";
import { quote } from "./quote.ts";

export default async function program() {
  return await effect("price", () => {
    writeFileSync(process.env.PULSE_MARKER ?? "", String(quote()));
    return quote();
  });
}
"#,
    )
    .unwrap();
    let sdk = root.join("node_modules/@trigora/sdk");
    std::fs::create_dir_all(&sdk).unwrap();
    std::fs::write(
        sdk.join("package.json"),
        r#"{"name":"@trigora/sdk","type":"module","exports":{".":"./index.js"}}"#,
    )
    .unwrap();
    std::fs::write(sdk.join("index.js"), "export function effect() {}\n").unwrap();
}

#[test]
fn missing_token_does_not_start_the_host() {
    let root = project("token");
    write_program(&root);
    let output = Command::new(cli())
        .args(["pulse", "start"])
        .current_dir(&root)
        .env_remove("TRIGORA_TOKEN")
        .env("TRIGORA_NODE_HELPER", helper())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("TRIGORA_TOKEN"));
    assert!(!root.join(".trigora/pulse/pulse.sock").exists());
}

#[test]
fn a_blank_token_does_not_start_the_host() {
    let root = project("blank");
    write_program(&root);
    let output = Command::new(cli())
        .args(["pulse", "start"])
        .current_dir(&root)
        .env("TRIGORA_TOKEN", "   ")
        .env("TRIGORA_NODE_HELPER", helper())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!root.join(".trigora/pulse/pulse.sock").exists());
}

#[test]
fn an_incompatible_host_is_rejected_before_the_socket() {
    let root = project("version");
    write_program(&root);
    let script = root.join("fake-host");
    std::fs::write(
        &script,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo \"trigora-pulse 9.9.9 protocol 1\"\n  exit 0\nfi\ntouch \"$1/socket-created\"\nexit 0\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    let output = Command::new(cli())
        .args(["pulse", "start", "--ephemeral"])
        .current_dir(&root)
        .env("TRIGORA_PULSE_BIN", &script)
        .env("TRIGORA_NODE_HELPER", helper())
        .env_remove("TRIGORA_TOKEN")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("9.9.9"), "{stderr}");
    assert!(stderr.contains("0.1.0"), "{stderr}");
    assert!(!root.join(".trigora/pulse/socket-created").exists());
    assert!(!root.join(".trigora/pulse/pulse.sock").exists());
}

#[test]
fn ephemeral_start_runs_an_imported_handler() {
    let root = project("run");
    write_program(&root);
    let marker = root.join("marker.txt");
    let pulse = pulse_bin();
    let mut child = Command::new(cli())
        .args(["pulse", "start", "--ephemeral"])
        .current_dir(&root)
        .env("TRIGORA_PULSE_BIN", &pulse)
        .env("TRIGORA_NODE_HELPER", helper())
        .env("PULSE_MARKER", &marker)
        .env_remove("TRIGORA_TOKEN")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let logs = std::thread::scope(|scope| {
        let err = scope.spawn(|| read_to_string(stderr));
        let out = scope.spawn(|| read_to_string(stdout));
        let ready = root.join(".trigora/pulse/ready.json");
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(40) && !ready.is_file() {
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        if !ready.is_file() {
            let _ = unsafe { libc::kill(child.id() as i32, libc::SIGKILL) };
            let _ = child.wait();
            panic!(
                "pulse did not become ready\nstdout:\n{}\nstderr:\n{}",
                out.join().unwrap(),
                err.join().unwrap()
            );
        }
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&ready).unwrap()).unwrap();
        let program = &manifest["programs"][0];
        let bearer = std::fs::read_to_string(root.join(".trigora/pulse/bearer")).unwrap();
        let mut stream = UnixStream::connect(root.join(".trigora/pulse/pulse.sock")).unwrap();
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
                "artifactHash": program["hash"],
                "keys": program["keys"],
                "artifact": std::fs::read_to_string(program["artifact"].as_str().unwrap()).unwrap(),
            }),
        );
        let registered = read_frame(&mut stream);
        assert_eq!(registered["type"], "registered", "{registered}");
        write_frame(
            &mut stream,
            &serde_json::json!({ "type": "start", "executionId": "once" }),
        );
        let marked = Instant::now();
        while marked.elapsed() < Duration::from_secs(10) && !marker.is_file() {
            std::thread::sleep(Duration::from_millis(30));
        }
        assert_eq!(std::fs::read_to_string(&marker).unwrap_or_default(), "17");
        let _ = unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        let signaled = Instant::now();
        let status = child.wait().unwrap();
        assert!(signaled.elapsed() < Duration::from_secs(12));
        assert!(status.success(), "shutdown status {status}");
        (out.join().unwrap(), err.join().unwrap())
    });
    let stderr = logs.1;
    assert!(
        stderr.contains("Pulse running in ephemeral mode."),
        "{stderr}"
    );
    assert!(
        stderr.contains("Execution state will be lost when the host exits."),
        "{stderr}"
    );
}

fn read_to_string(mut pipe: impl Read) -> String {
    let mut text = String::new();
    let _ = pipe.read_to_string(&mut text);
    text
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
