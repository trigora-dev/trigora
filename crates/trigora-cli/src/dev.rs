use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::{json, Value as Json};

use crate::adapter::Registry;
use crate::compile::discover;
use crate::config::{self, ProjectConfig};
use crate::error::CliError;
use crate::http::{self, Endpoint};
use crate::model::Program;
use crate::paths;

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

fn push_programs(port: u16, token: &str, programs: &[Program]) -> Result<(), CliError> {
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
    };
    http::request(&endpoint, "POST", "/programs", Some(&body)).map_err(|error| {
        CliError::plain(format!(
            "Program registration failed ({}). {}",
            error.status, error.message
        ))
    })?;
    Ok(())
}

fn session_loop(
    rx: Receiver<bool>,
    config: &ProjectConfig,
    registry: &Registry,
    programs: &Arc<Mutex<Vec<Program>>>,
    watcher: &mut RecommendedWatcher,
    control_port: u16,
    control_token: &str,
) {
    let mut pending = false;
    loop {
        let wait = if pending {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(60 * 60)
        };
        match rx.recv_timeout(wait) {
            Ok(true) => break,
            Ok(false) => pending = true,
            Err(mpsc::RecvTimeoutError::Timeout) if pending => {
                pending = false;
                reload(
                    config,
                    registry,
                    programs,
                    watcher,
                    control_port,
                    control_token,
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn reload(
    config: &ProjectConfig,
    registry: &Registry,
    programs: &Arc<Mutex<Vec<Program>>>,
    watcher: &mut RecommendedWatcher,
    control_port: u16,
    control_token: &str,
) {
    match discover(&config.root, &config.programs, registry) {
        Ok(next) => {
            if let Err(error) = push_programs(control_port, control_token, &next) {
                eprintln!();
                eprintln!("✖ Reload failed");
                eprintln!("{error}");
                return;
            }
            let names = next
                .iter()
                .map(|program| program.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            *programs.lock().expect("programs") = next;
            watch_files(
                watcher,
                config,
                &programs.lock().expect("programs"),
                registry,
            );
            println!();
            println!("Programs changed. Reloaded {names}.");
            let _ = std::io::stdout().flush();
        }
        Err(error) => {
            eprintln!();
            eprintln!("✖ Reload failed");
            eprintln!("{error}");
        }
    }
}

fn watch_files(
    watcher: &mut RecommendedWatcher,
    config: &ProjectConfig,
    programs: &[Program],
    registry: &Registry,
) {
    let mut paths = vec![config.path.clone()];
    for program in programs {
        paths.push(config.root.join(&program.file));
    }
    paths.extend(registry.watch_paths(&config.root, programs));
    for path in paths {
        let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
    }
}

fn serve_effects(
    programs: Arc<Mutex<Vec<Program>>>,
    secret: String,
    registry: Arc<Registry>,
    shutdown: Arc<AtomicBool>,
) -> Result<u16, CliError> {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).map_err(|error| CliError::plain(error.to_string()))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| CliError::plain(error.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|error| CliError::plain(error.to_string()))?
        .port();
    std::thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let programs = Arc::clone(&programs);
                    let secret = secret.clone();
                    let registry = Arc::clone(&registry);
                    std::thread::spawn(move || {
                        handle_effect(stream, &programs, &secret, &registry)
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    Ok(port)
}

fn handle_effect(
    mut stream: TcpStream,
    programs: &Mutex<Vec<Program>>,
    secret: &str,
    registry: &Registry,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let Ok((method, path, headers, body)) = read_request(&mut stream) else {
        return;
    };
    let authorized = headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("authorization") && value == &format!("Bearer {secret}")
    });
    if method != "POST" || path != "/effects" || !authorized {
        let _ = write_json(
            &mut stream,
            401,
            &json!({"error": "Effect secret was rejected."}),
        );
        return;
    }
    let parsed: Json = match serde_json::from_slice(&body) {
        Ok(parsed) => parsed,
        Err(_) => {
            let _ = write_json(
                &mut stream,
                400,
                &json!({"error": "Request body must be valid JSON."}),
            );
            return;
        }
    };
    let program_id = parsed.get("programId").and_then(Json::as_str).unwrap_or("");
    let key = parsed.get("key").and_then(Json::as_str).unwrap_or("");
    let input = parsed.get("input").cloned().unwrap_or(Json::Null);
    let found = {
        let programs = programs.lock().expect("programs");
        programs
            .iter()
            .find(|program| program.id == program_id)
            .and_then(|program| {
                program
                    .effects
                    .iter()
                    .find(|effect| effect.key == key)
                    .cloned()
                    .map(|effect| (program.language.clone(), effect))
            })
    };
    let Some((language, effect)) = found else {
        let _ = write_json(
            &mut stream,
            200,
            &json!({"error": format!("No effect handler for `{key}`.")}),
        );
        return;
    };
    let response = match registry.run_effect(&language, &effect, key, &input) {
        Ok(Some(value)) => json!({"value": value}),
        Ok(None) => json!({"error": format!("No effect handler for `{key}`.")}),
        Err(error) => json!({"error": error.to_string()}),
    };
    let _ = write_json(&mut stream, 200, &response);
}

struct StopAdapters(Arc<Registry>);

impl Drop for StopAdapters {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

fn read_request(
    stream: &mut TcpStream,
) -> std::io::Result<(String, String, Vec<(String, String)>, Vec<u8>)> {
    let mut reader = BufReader::new(stream);
    let mut start = String::new();
    reader.read_line(&mut start)?;
    let mut parts = start.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
            headers.push((name.to_string(), value.trim().to_string()));
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok((method, path, headers, body))
}

fn write_json(stream: &mut TcpStream, status: u16, body: &Json) -> std::io::Result<()> {
    let payload = body.to_string();
    let reason = if status == 200 {
        "OK"
    } else if status == 400 {
        "Bad Request"
    } else {
        "Unauthorized"
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
        payload.len()
    )?;
    stream.flush()
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
