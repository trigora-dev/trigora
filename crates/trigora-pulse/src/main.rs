use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: trigora-pulse <directory>");
        return ExitCode::from(2);
    };
    if let Err(err) = trigora_pulse::listen(Path::new(&dir)) {
        eprintln!("{err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
