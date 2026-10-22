use std::env;
use std::fs;
use std::process::ExitCode;
use std::sync::Arc;

use trigora_pulse::{Authority, Config, Runtime};

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
    let runtime = match Runtime::open(Arc::new(Authority::new()), artifact, config) {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };
    // `started` accepts an execution. The in-memory authority dies with this process.
    std::thread::park();
    drop(runtime);
    ExitCode::SUCCESS
}
