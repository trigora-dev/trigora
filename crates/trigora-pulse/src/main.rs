use std::env;
use std::fs;
use std::process::ExitCode;
use std::sync::Arc;

use trigora_pulse::{Authority, Config, Runtime};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(dir), Some(artifact_path)) = (args.next(), args.next()) else {
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
    let runtime = match Runtime::open(
        Arc::new(Authority::new()),
        artifact,
        Config::new(&dir, "pulse"),
    ) {
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
