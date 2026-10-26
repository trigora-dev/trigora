use std::env;
use std::fs;
use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use trigora_pulse::{
    select_remote, Adapter, Authority, Config, RemoteClient, RemoteMode, Runtime, ADAPTER_PROTOCOL,
};

const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(10);

fn main() -> ExitCode {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!(
            "trigora-pulse {} protocol {ADAPTER_PROTOCOL}",
            env!("CARGO_PKG_VERSION")
        );
        return ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) == Some("--check") {
        let Some(dir) = args.get(1) else {
            eprintln!("usage: trigora-pulse --check <directory>");
            return ExitCode::from(2);
        };
        return match Adapter::connect(std::path::Path::new(dir)) {
            Ok(_) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::from(1)
            }
        };
    }
    let ephemeral = args.iter().any(|arg| arg == "--ephemeral");
    args.retain(|arg| arg != "--ephemeral");
    if args.is_empty() || args.len() > 2 {
        eprintln!("usage: trigora-pulse [--ephemeral] <directory> [artifact.json]");
        return ExitCode::from(2);
    }
    let dir = &args[0];
    let artifact_path = args.get(1).cloned();
    let remote = if ephemeral {
        eprintln!("Pulse running in ephemeral mode.");
        eprintln!("Execution state will be lost when the host exits.");
        None
    } else {
        match select_remote(
            env::var("TRIGORA_TOKEN").ok().as_deref(),
            env::var("TRIGORA_API_BASE_URL").ok().as_deref(),
            env::var("TRIGORA_PULSE_AUTHORITY_URL").ok().as_deref(),
            env::var("TRIGORA_PULSE_WORKSPACE_ID").ok().as_deref(),
            env::var("TRIGORA_PULSE_WORKSPACE_TOKEN").ok().as_deref(),
        ) {
            Ok(RemoteMode::Local) => {
                eprintln!("Durability is not configured.");
                return ExitCode::from(2);
            }
            Ok(mode) => Some(mode),
            Err(err) => {
                eprintln!("{err}");
                return ExitCode::from(2);
            }
        }
    };
    let artifact = if let Some(artifact_path) = artifact_path.as_deref() {
        let text = match fs::read_to_string(artifact_path) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("read {artifact_path}: {err}");
                return ExitCode::from(1);
            }
        };
        match tcc_ir::decode_artifact(&text) {
            Ok(artifact) => Some(artifact),
            Err(err) => {
                eprintln!("artifact: {err}");
                return ExitCode::from(1);
            }
        }
    } else {
        None
    };
    let mut config = Config::new(dir, "pulse");
    if let Ok(ms) = env::var("TRIGORA_PULSE_EFFECT_TIMEOUT_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            config.effect_timeout = Duration::from_millis(ms);
        }
    }
    if let Ok(ms) = env::var("TRIGORA_PULSE_LEASE_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            if ms > 0 {
                config.lease_ms = ms;
            }
        }
    }
    let max = env::var("TRIGORA_PULSE_MAX_CONTINUATION_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(16 * 1024 * 1024);
    if max == 0 || max > 16 * 1024 * 1024 {
        eprintln!("TRIGORA_PULSE_MAX_CONTINUATION_BYTES is above 16 MiB");
        return ExitCode::from(2);
    }
    let mut authority = Authority::new();
    if let Some(mode) = remote {
        let client = match mode {
            RemoteMode::Local => None,
            RemoteMode::Gateway { base, token } => Some(RemoteClient::gateway(base, token, max)),
            RemoteMode::Authority {
                url,
                workspace,
                token,
            } => Some(RemoteClient::sqlite(url, workspace, token, max)),
        };
        match client {
            None => {}
            Some(Ok(remote)) => authority = authority.with_remote(remote),
            Some(Err(err)) => {
                eprintln!("{err}");
                return ExitCode::from(2);
            }
        }
    }
    let authority = Arc::new(authority);
    let opened = match artifact {
        Some(artifact) => Runtime::open(authority, artifact, config),
        None => Runtime::listen(authority, config),
    };
    let runtime = match opened {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };
    let (tx, rx) = mpsc::sync_channel(1);
    if ctrlc::set_handler(move || {
        let _ = tx.try_send(());
    })
    .is_err()
    {
        std::thread::park();
        drop(runtime);
        return ExitCode::SUCCESS;
    }
    let _ = rx.recv();
    if shutdown_within(runtime, SHUTDOWN_DEADLINE) {
        ExitCode::SUCCESS
    } else {
        eprintln!("Pulse shutdown exceeded 10 seconds.");
        std::process::exit(1);
    }
}

fn shutdown_within(runtime: Runtime, limit: Duration) -> bool {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        runtime.shutdown();
        let _ = tx.send(());
    });
    rx.recv_timeout(limit).is_ok()
}
