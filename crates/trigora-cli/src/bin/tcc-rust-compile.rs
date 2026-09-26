use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("{}", tcc_rust_frontend::PACKAGE_VERSION);
        return ExitCode::SUCCESS;
    }
    let Some(path) = args.first() else {
        eprintln!("usage: tcc-rust-compile <file.rs>");
        return ExitCode::from(2);
    };
    if args.len() != 1 {
        eprintln!("usage: tcc-rust-compile <file.rs>");
        return ExitCode::from(2);
    }
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::from(1);
        }
    };
    match tcc_rust_frontend::compile(&source) {
        Ok(artifact) => match tcc_ir::encode_artifact(&artifact) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(1)
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
