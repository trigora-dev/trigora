mod adapter;
mod cli;
mod commands;
mod compile;
mod config;
mod deploy;
mod dev;
mod env;
mod error;
mod http;
mod init;
mod model;
mod output;
mod paths;
mod python;
mod rust_harness;
mod versions;

pub use cli::{parse_invocation, Invocation};
pub use commands::{endpoint, local_endpoint};
pub use config::load_config;
pub use deploy::sync_deployment;
pub use env::{cloud_url, runtime_url, DEFAULT_CLOUD_URL, DEFAULT_RUNTIME_URL};
pub use error::CliError;
pub use model::{program_slug, Program};
pub use output::render_error;

use std::env::current_dir;

pub fn run() -> Result<(), CliError> {
    let root = current_dir().map_err(|error| CliError::plain(error.to_string()))?;
    env::load_project_env(&root);
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    dispatch(&args)
}

pub fn dispatch(args: &[String]) -> Result<(), CliError> {
    match parse_invocation(args)? {
        Invocation::Init {
            force,
            language,
            name,
            example,
        } => init::init(force, language, name, example),
        Invocation::Dev { host, port } => dev::dev(host, port),
        Invocation::Deploy { program } => deploy_command(program),
        Invocation::Programs { remote } => commands::programs(remote),
        Invocation::Executions { remote } => commands::executions(remote),
        Invocation::Inspect { execution, remote } => commands::inspect(&execution, remote),
        Invocation::Start {
            program,
            input,
            remote,
        } => commands::start(&program, input.as_deref(), remote),
        Invocation::Send {
            execution,
            event,
            payload,
            remote,
        } => commands::send(&execution, &event, payload.as_deref(), remote),
        Invocation::Cancel { execution, remote } => commands::cancel(&execution, remote),
        Invocation::Whoami => commands::whoami(),
    }
}

fn deploy_command(program: Option<String>) -> Result<(), CliError> {
    let _ = deploy::cloud_endpoint()?;
    let root = current_dir().map_err(|error| CliError::plain(error.to_string()))?;
    let config = config::load_config(&root)?;
    let tools = paths::tools()?;
    let registry = adapter::Registry::new(&tools);
    let result = (|| {
        let programs = compile::discover(&config.root, &config.programs, &registry)?;
        let endpoint = deploy::cloud_endpoint()?;
        deploy::sync_deployment(&endpoint, &config, &programs, program.as_deref(), &registry)
    })();
    registry.shutdown();
    result
}

#[cfg(test)]
mod tests;
