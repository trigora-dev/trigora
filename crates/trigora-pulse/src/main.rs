use std::env;
use std::fs;
use std::process::ExitCode;
use std::sync::Arc;

use trigora_pulse::{run_remote_measure, Authority, Config, RemoteClient, Runtime};

mod bench;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        eprintln!("usage: trigora-pulse <directory> <artifact.json>");
        return ExitCode::from(2);
    };
    if first == "bench" {
        return bench::run();
    }
    if first == "measure-remote" {
        return match run_remote_measure() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::from(1)
            }
        };
    }
    let Some(artifact_path) = args.next() else {
        eprintln!("usage: trigora-pulse <directory> <artifact.json>");
        return ExitCode::from(2);
    };
    let dir = first;
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
    let mut authority = Authority::new();
    if let Ok(url) = env::var("TRIGORA_PULSE_AUTHORITY_URL") {
        let workspace = match env::var("TRIGORA_PULSE_WORKSPACE_ID") {
            Ok(workspace) => workspace,
            Err(_) => {
                eprintln!("TRIGORA_PULSE_WORKSPACE_ID is required with an authority URL");
                return ExitCode::from(2);
            }
        };
        let token = match env::var("TRIGORA_PULSE_WORKSPACE_TOKEN") {
            Ok(token) => token,
            Err(_) => {
                eprintln!("TRIGORA_PULSE_WORKSPACE_TOKEN is required with an authority URL");
                return ExitCode::from(2);
            }
        };
        let max = env::var("TRIGORA_PULSE_MAX_CONTINUATION_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(16 * 1024 * 1024);
        if max == 0 || max > 16 * 1024 * 1024 {
            eprintln!("TRIGORA_PULSE_MAX_CONTINUATION_BYTES is above 16 MiB");
            return ExitCode::from(2);
        }
        authority = authority.with_remote(RemoteClient::sqlite(url, workspace, token, max));
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
