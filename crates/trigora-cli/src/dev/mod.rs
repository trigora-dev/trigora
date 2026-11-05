use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value as Json};

use crate::adapter::Registry;
use crate::compile::discover;
use crate::config::{self, ProjectConfig};
use crate::error::CliError;
use crate::http::{self, Endpoint};
use crate::model::Program;
use crate::paths;

mod effects;
mod watcher;

use effects::serve_effects;
use watcher::{session_loop, watch_files};

pub fn dev(host: Option<String>, port: Option<u16>) -> Result<(), CliError> {
    let root = std::env::current_dir().map_err(|error| CliError::plain(error.to_string()))?;
    let loaded = config::load_config(&root)?;
    let host = host.unwrap_or_else(|| "127.0.0.1".to_string());
    let requested_port = port.unwrap_or(3477);
    let tools = paths::tools()?;
    let registry = Arc::new(Registry::new(&tools));
    let _stop_adapters = StopAdapters(Arc::clone(&registry));
    let programs = discover(&loaded.root, &loaded.programs, &registry)?;
    let secret = paths::random_token();
    let shared = Arc::new(Mutex::new(programs));
    let shutdown = Arc::new(AtomicBool::new(false));
    let effect_port = serve_effects(
        Arc::clone(&shared),
        secret.clone(),
        Arc::clone(&registry),
        Arc::clone(&shutdown),
    )?;
    let db = loaded.root.join(".trigora").join("state.db");
    if let Some(parent) = db.parent() {
        std::fs::create_dir_all(parent).map_err(|error| CliError::plain(error.to_string()))?;
    }
    let mut local = Command::new(&tools.local_bin)
        .arg("--db")
        .arg(&db)
        .arg("--host")
        .arg(&host)
        .arg("--port")
        .arg(requested_port.to_string())
        .arg("--effect-url")
        .arg(format!("http://127.0.0.1:{effect_port}/effects"))
        .arg("--effect-secret")
        .arg(&secret)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| CliError::plain(error.to_string()))?;
    let stdout = local.stdout.take().expect("stdout");
    let ready = wait_ready(stdout)?;
    push_programs(
        ready.control_port,
        &ready.control_token,
        &shared.lock().expect("programs"),
    )?;
    if ready.port != requested_port {
        println!(
            "Port {requested_port} was in use, using {} instead.",
            ready.port
        );
    }
    let (tx, rx) = mpsc::channel();
    let signal_tx = tx.clone();
    ctrlc::set_handler(move || {
        let _ = signal_tx.send(true);
    })
    .map_err(|error| CliError::plain(error.to_string()))?;
    let watch_tx = tx.clone();
    let mut watcher =
        notify::recommended_watcher(move |result: Result<notify::Event, notify::Error>| {
            if result.is_ok() {
                let _ = watch_tx.send(false);
            }
        })
        .map_err(|error| CliError::plain(error.to_string()))?;
    watch_files(
        &mut watcher,
        &loaded,
        &shared.lock().expect("programs"),
        &registry,
    );
    print_ready(
        &loaded,
        &shared.lock().expect("programs"),
        &host,
        ready.port,
        ready.restored,
    );
    session_loop(
        rx,
        &loaded,
        &registry,
        &shared,
        &mut watcher,
        ready.control_port,
        &ready.control_token,
    );
    drop(watcher);
    shutdown.store(true, Ordering::SeqCst);
    let _ = TcpStream::connect(("127.0.0.1", effect_port));
    registry.shutdown();
    terminate(&mut local);
    println!();
    println!("Stopped");
    Ok(())
}

struct Ready {
    port: u16,
    control_port: u16,
    control_token: String,
    restored: u64,
}

fn wait_ready(stdout: std::process::ChildStdout) -> Result<Ready, CliError> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let Ok(event) = serde_json::from_str::<Json>(&line) else {
                continue;
            };
            if event.get("type").and_then(Json::as_str) == Some("ready") {
                let _ = tx.send(event);
                continue;
            }
            print_engine_event(&event);
        }
    });
    let event = rx
        .recv_timeout(Duration::from_secs(30))
        .map_err(|_| CliError::plain("The local runtime did not become ready."))?;
    Ok(Ready {
        port: event.get("port").and_then(Json::as_u64).unwrap_or(0) as u16,
        control_port: event.get("controlPort").and_then(Json::as_u64).unwrap_or(0) as u16,
        control_token: event
            .get("controlToken")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
        restored: event.get("restored").and_then(Json::as_u64).unwrap_or(0),
    })
}

fn print_engine_event(event: &Json) {
    let kind = event.get("type").and_then(Json::as_str).unwrap_or("");
    let execution = event.get("execution").cloned().unwrap_or(Json::Null);
    let id = execution.get("id").and_then(Json::as_str).unwrap_or("");
    let program = execution
        .get("programId")
        .and_then(Json::as_str)
        .unwrap_or("");
    let line = match kind {
        "started" => format!("▶ Started {program} {id}"),
        "waiting" => {
            let wait = execution.get("wait");
            let detail = match wait
                .and_then(|wait| wait.get("type"))
                .and_then(Json::as_str)
            {
                Some("event") => format!(
                    "event {}",
                    wait.and_then(|wait| wait.get("event"))
                        .and_then(Json::as_str)
                        .unwrap_or("")
                ),
                Some("timer") => "timer".to_string(),
                Some("child") => "child execution".to_string(),
                _ => "durable boundary".to_string(),
            };
            format!("⏸ {program} {id} waiting on {detail}")
        }
        "resumed" => format!("▶ {program} {id} resumed"),
        "completed" => format!("✔ {program} {id} completed"),
        "failed" => format!("✖ {program} {id} failed"),
        "cancelled" => format!("■ {program} {id} cancelled"),
        "effect" => format!(
            "· {program} {id} effect {}",
            event.get("name").and_then(Json::as_str).unwrap_or("")
        ),
        _ => return,
    };
    println!("{line}");
}

fn print_ready(config: &ProjectConfig, programs: &[Program], host: &str, port: u16, restored: u64) {
    let names = programs
        .iter()
        .map(|program| program.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    println!();
    println!("✔ Local runtime ready");
    println!();
    println!("  Programs  {names}");
    println!("  Runtime   http://{host}:{port}");
    if restored > 0 {
        let plural = if restored == 1 { "" } else { "s" };
        println!("  Restored  {restored} waiting execution{plural}");
    }
    println!();
    println!("  Start a program with `trigora start <program>` while this process is running.");
    println!("  Stop it and run `trigora dev` again to resume waiting executions.");
    let _ = std::io::stdout().flush();
    let _ = config;
}

pub(in crate::dev) fn push_programs(
    port: u16,
    token: &str,
    programs: &[Program],
) -> Result<(), CliError> {
    let body = json!({
        "programs": programs.iter().map(|program| json!({
            "id": program.id,
            "artifactJson": program.artifact_json,
            "artifactHash": program.artifact_hash,
            "language": program.language,
            "frontendId": program.frontend_id,
            "frontendVersion": program.compiler_version,
            "languageSemanticsVersion": program.semantics_version,
            "engineFormatVersion": 1,
        })).collect::<Vec<_>>()
    });
    let endpoint = Endpoint {
        base: format!("http://127.0.0.1:{port}"),
        token: Some(token.to_string()),
        cloud: false,
        project_id: None,
    };
    http::request(&endpoint, "POST", "/programs", Some(&body)).map_err(|error| {
        CliError::plain(format!(
            "Program registration failed ({}). {}",
            error.status, error.message
        ))
    })?;
    Ok(())
}

struct StopAdapters(Arc<Registry>);

impl Drop for StopAdapters {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

fn terminate(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let _ = child.wait();
}
