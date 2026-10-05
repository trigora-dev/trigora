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
    Result {
        execution: String,
        remote: bool,
    },
    Bench {
        program: String,
        input: Option<String>,
        effects: Option<String>,
        events: Option<String>,
        out: Option<String>,
    },
    Verify {
        program: String,
        input: Option<String>,
        faults: crate::bench::Faults,
        effects: Option<String>,
        events: Option<String>,
        out: Option<String>,
    },
    Whoami,
    Secrets {
        action: crate::secrets::SecretsAction,
    },
}

#[derive(Parser)]
#[command(
    name = "trigora",
    version = env!("CARGO_PKG_VERSION"),
    about = "Durable execution for TypeScript, Python, and Rust",
    arg_required_else_help = true,
    after_help = "Local commands use `trigora dev`. `deploy`, `whoami`, and `secrets` use Trigora Cloud.\nAdd `--remote` to programs, executions, start, send, result, and cancel to use Trigora Cloud."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a project in the current directory
    Init {
        /// Overwrite files that already exist
        #[arg(short, long)]
        force: bool,
        /// `typescript`, `python`, or `rust`. Prompts when omitted.
        #[arg(long)]
        language: Option<String>,
        /// Project name. Defaults to the directory name.
        #[arg(long)]
        name: Option<String>,
        /// Create the example program
        #[arg(long)]
        example: bool,
        /// Skip the example program
        #[arg(long = "no-example")]
        no_example: bool,
    },
    /// Run programs on the local runtime
    Dev {
        /// Address to listen on. Defaults to 127.0.0.1.
        #[arg(long)]
        host: Option<String>,
        /// Port to listen on. Defaults to 3477.
        #[arg(long)]
        port: Option<String>,
    },
    /// Deploy this project to Trigora Cloud
    Deploy {
        /// Deploy one program. Defaults to every discovered program.
        #[arg(long)]
        program: Option<String>,
    },
    /// List programs
    Programs {
        /// Use Trigora Cloud instead of the local runtime
        #[arg(long)]
        remote: bool,
    },
    /// List executions
    Executions {
        /// Use Trigora Cloud instead of the local runtime
        #[arg(long, global = true)]
        remote: bool,
        #[command(subcommand)]
        action: Option<ExecutionsCommand>,
    },
    /// Start a program
    Start {
        /// Program name
        program: String,
        /// JSON value or a path to a JSON file. Defaults to {}.
        #[arg(long)]
        input: Option<String>,
        /// Use Trigora Cloud instead of the local runtime
        #[arg(long)]
        remote: bool,
    },
    /// Send an event to a waiting execution
    Send {
        /// Execution id
        execution: String,
        /// Event name
        event: String,
        /// JSON value or a path to a JSON file. Defaults to {}.
        #[arg(long)]
        payload: Option<String>,
        /// Use Trigora Cloud instead of the local runtime
        #[arg(long)]
        remote: bool,
    },
    /// Cancel an execution
    Cancel {
        /// Execution id
        execution: String,
        /// Use Trigora Cloud instead of the local runtime
        #[arg(long)]
        remote: bool,
    },
    /// Show an execution result
    Result {
        /// Execution id
        execution: String,
        /// Use Trigora Cloud instead of the local runtime
        #[arg(long)]
        remote: bool,
    },
    /// Measure one healthy local run
    Bench {
        /// Program name
        program: String,
        /// JSON value or a path to a JSON file. Defaults to {}.
        #[arg(long)]
        input: Option<String>,
        /// `live`, or a JSON file of effect results
        #[arg(long)]
        effects: Option<String>,
        /// JSON file mapping each event name to a payload, or to an array of payloads
        #[arg(long)]
        events: Option<String>,
        /// Write the JSON report to this file
        #[arg(long)]
        out: Option<String>,
    },
    /// Check that a local program recovers after a checkpoint
    Verify {
        /// Program name
        program: String,
        /// JSON value or a path to a JSON file. Defaults to {}.
        #[arg(long)]
        input: Option<String>,
        /// `sample` (default) checks a spread of checkpoints. `all` checks every checkpoint.
        #[arg(long, value_enum, default_value_t = crate::bench::Faults::Sample)]
        faults: crate::bench::Faults,
        /// `live`, or a JSON file of effect results
        #[arg(long)]
        effects: Option<String>,
        /// JSON file mapping each event name to a payload, or to an array of payloads
        #[arg(long)]
        events: Option<String>,
        /// Write the JSON report to this file
        #[arg(long)]
        out: Option<String>,
    },
    /// Show the signed-in Trigora Cloud workspace
    Whoami,
    /// Manage project secrets on Trigora Cloud
    Secrets {
        #[command(subcommand)]
        action: SecretsCommand,
    },
}

#[derive(Subcommand)]
enum ExecutionsCommand {
    /// Show an execution
    Inspect {
        /// Execution id
        execution: String,
        /// Use Trigora Cloud instead of the local runtime
        #[arg(long)]
        remote: bool,
    },
}

#[derive(Subcommand)]
enum SecretsCommand {
    /// List secret names
    List,
    /// Set a secret. The value is read from a prompt or stdin.
    Set { name: String },
    /// Delete a secret
    Delete { name: String },
}

pub fn parse_invocation(args: &[String]) -> Result<Invocation, CliError> {
    let mut full = vec!["trigora".to_string()];
    full.extend(args.iter().cloned());
    let cli = Cli::try_parse_from(full).map_err(|error| {
        if matches!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
        ) {
            print!("{error}");
            std::process::exit(0);
        }
        if error.kind() == clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand {
            print!("{error}");
            std::process::exit(2);
        }
        CliError::usage(error.to_string())
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
        Command::Result { execution, remote } => Ok(Invocation::Result { execution, remote }),
        Command::Bench {
            program,
            input,
            effects,
            events,
            out,
        } => Ok(Invocation::Bench {
            program,
            input,
            effects,
            events,
            out,
        }),
        Command::Verify {
            program,
            input,
            faults,
            effects,
            events,
            out,
        } => Ok(Invocation::Verify {
            program,
            input,
            faults,
            effects,
            events,
            out,
        }),
        Command::Whoami => Ok(Invocation::Whoami),
        Command::Secrets { action } => Ok(Invocation::Secrets {
            action: match action {
                SecretsCommand::List => crate::secrets::SecretsAction::List,
                SecretsCommand::Set { name } => crate::secrets::SecretsAction::Set { name },
                SecretsCommand::Delete { name } => crate::secrets::SecretsAction::Delete { name },
            },
        }),
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
