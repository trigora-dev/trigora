use std::env;
use std::fs;
use std::process::ExitCode;
use std::sync::Arc;

use trigora_pulse::{select_remote, Authority, Config, RemoteClient, RemoteMode, Runtime};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: trigora-pulse <directory> <artifact.json>");
        return ExitCode::from(2);
    };
    let Some(artifact_path) = args.next() else {
        eprintln!("usage: trigora-pulse <directory> <artifact.json>");
        return ExitCode::from(2);
    };
    let text = match fs::read_to_string(&artifact_path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("read {artifact_path}: {err}");
            return ExitCode::from(1);
        }
    };
    let artifact = match tcc_ir::decode_artifact(&text) {
        Ok(artifact) => artifact,
        Err(err) => {
            eprintln!("artifact: {err}");
            return ExitCode::from(1);
        }
    };
    let mut config = Config::new(&dir, "pulse");
    if let Ok(ms) = env::var("TRIGORA_PULSE_EFFECT_TIMEOUT_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            config.effect_timeout = std::time::Duration::from_millis(ms);
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
    let mode = match select_remote(
        env::var("TRIGORA_TOKEN").ok().as_deref(),
        env::var("TRIGORA_API_BASE_URL").ok().as_deref(),
        env::var("TRIGORA_PULSE_AUTHORITY_URL").ok().as_deref(),
        env::var("TRIGORA_PULSE_WORKSPACE_ID").ok().as_deref(),
        env::var("TRIGORA_PULSE_WORKSPACE_TOKEN").ok().as_deref(),
    ) {
        Ok(mode) => mode,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let mut authority = Authority::new();
    let remote = match mode {
        RemoteMode::Local => None,
        RemoteMode::Gateway { base, token } => Some(RemoteClient::gateway(base, token, max)),
        RemoteMode::Authority {
            url,
            workspace,
            token,
        } => Some(RemoteClient::sqlite(url, workspace, token, max)),
    };
    match remote {
        None => {}
        Some(Ok(remote)) => authority = authority.with_remote(remote),
        Some(Err(err)) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    }
    let runtime = match Runtime::open(Arc::new(authority), artifact, config) {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };
    // `started` accepts an execution. Without an authority URL, durability dies with this process.
    std::thread::park();
    drop(runtime);
    ExitCode::SUCCESS
}
