use clap::{Parser, Subcommand};

use crate::error::CliError;

#[derive(Debug, PartialEq, Eq)]
pub enum Invocation {
    Init {
        force: bool,
        language: Option<String>,
        name: Option<String>,
        example: Option<bool>,
    },
    Dev {
        host: Option<String>,
        port: Option<u16>,
    },
    Deploy {
        program: Option<String>,
    },
    Programs {
        remote: bool,
    },
    Executions {
        remote: bool,
    },
    Inspect {
        execution: String,
        remote: bool,
    },
    Start {
        program: String,
        input: Option<String>,
        remote: bool,
    },
    Send {
        execution: String,
        event: String,
        payload: Option<String>,
        remote: bool,
    },
    Cancel {
        execution: String,
        remote: bool,
    },
    Whoami,
}

#[derive(Parser)]
#[command(
    name = "trigora",
    version = "0.9.0",
    about = "Local durable execution runtime"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize a new Trigora project
    Init {
        #[arg(short, long)]
        force: bool,
        #[arg(long)]
        language: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        example: bool,
        #[arg(long = "no-example")]
        no_example: bool,
    },
    /// Start the local durable execution runtime
    Dev {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<String>,
    },
    /// Compile programs locally and deploy them to Trigora Cloud
    Deploy {
        #[arg(long)]
        program: Option<String>,
    },
    /// List programs
    Programs {
        #[arg(long)]
        remote: bool,
    },
    /// List executions
    Executions {
        #[arg(long, global = true)]
        remote: bool,
        #[command(subcommand)]
        action: Option<ExecutionsCommand>,
    },
    /// Start a program execution
    Start {
        program: String,
        #[arg(long)]
        input: Option<String>,
        #[arg(long)]
        remote: bool,
    },
    /// Send an event to a waiting execution
    Send {
        execution: String,
        event: String,
        #[arg(long)]
        payload: Option<String>,
        #[arg(long)]
        remote: bool,
    },
    /// Cancel an execution
    Cancel {
        execution: String,
        #[arg(long)]
        remote: bool,
    },
    /// Show the authenticated workspace and API token
    Whoami,
}

#[derive(Subcommand)]
enum ExecutionsCommand {
    /// Inspect an execution
    Inspect {
        execution: String,
        #[arg(long)]
        remote: bool,
    },
}

pub fn parse_invocation(args: &[String]) -> Result<Invocation, CliError> {
    let mut full = vec!["trigora".to_string()];
    full.extend(args.iter().cloned());
    let cli = Cli::try_parse_from(full).map_err(|error| {
        if error.kind() == clap::error::ErrorKind::DisplayHelp
            || error.kind() == clap::error::ErrorKind::DisplayVersion
        {
            eprint!("{error}");
            std::process::exit(0);
        }
        CliError::plain(error.to_string())
    })?;
    match cli.command {
        Command::Init {
            force,
            language,
            name,
            example,
            no_example,
        } => {
            if example && no_example {
                return Err(CliError::plain(
                    "Pass either `--example` or `--no-example`.",
                ));
            }
            let example = if no_example {
                Some(false)
            } else if example {
                Some(true)
            } else {
                None
            };
            Ok(Invocation::Init {
                force,
                language,
                name,
                example,
            })
        }
        Command::Dev { host, port } => Ok(Invocation::Dev {
            host,
            port: port.map(|value| parse_port(&value)).transpose()?,
        }),
        Command::Deploy { program } => Ok(Invocation::Deploy { program }),
        Command::Programs { remote } => Ok(Invocation::Programs { remote }),
        Command::Executions { remote, action } => match action {
            None => Ok(Invocation::Executions { remote }),
            Some(ExecutionsCommand::Inspect {
                execution,
                remote: child_remote,
            }) => Ok(Invocation::Inspect {
                execution,
                remote: remote || child_remote,
            }),
        },
        Command::Start {
            program,
            input,
            remote,
        } => Ok(Invocation::Start {
            program,
            input,
            remote,
        }),
        Command::Send {
            execution,
            event,
            payload,
            remote,
        } => Ok(Invocation::Send {
            execution,
            event,
            payload,
            remote,
        }),
        Command::Cancel { execution, remote } => Ok(Invocation::Cancel { execution, remote }),
        Command::Whoami => Ok(Invocation::Whoami),
    }
}

fn parse_port(value: &str) -> Result<u16, CliError> {
    let number: f64 = value.parse().map_err(|_| port_error())?;
    if !number.is_finite() || number.fract() != 0.0 || number <= 0.0 || number > u16::MAX as f64 {
        return Err(port_error());
    }
    Ok(number as u16)
}

fn port_error() -> CliError {
    CliError::plain("`--port` must be a positive integer.")
}
