use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value as Json};

use crate::adapter::Registry;
use crate::compile::discover_pulse;
use crate::config;
use crate::error::CliError;
use crate::model::Program;
use crate::paths;

mod binary;
mod process;

use binary::{host_version, resolve_pulse_bin, PULSE_PROTOCOL, PULSE_VERSION};
use process::{signal, wait_adapter, wait_child, wait_until_ready, SHUTDOWN_DEADLINE};

pub fn start(ephemeral: bool) -> Result<(), CliError> {
    let root = std::env::current_dir().map_err(|error| CliError::plain(error.to_string()))?;
    if !ephemeral {
        require_token()?;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let binary = resolve_pulse_bin(&root, &exe)?;
    let (version, protocol) = host_version(&binary)?;
    if version != PULSE_VERSION || protocol != PULSE_PROTOCOL {
        return Err(CliError::new("Incompatible Pulse host")
            .detail("Host", format!("{version} protocol {protocol}"))
            .detail("CLI", format!("{PULSE_VERSION} protocol {PULSE_PROTOCOL}"))
            .hint("Use the trigora-pulse binary built with this CLI."));
    }
    let prepared = prepare(&root)?;
    let dir = prepared.work.clone();
    let artifact = prepared.artifact.clone();
    let _ = fs::remove_file(dir.join("ready.json"));
    let mut host_cmd = Command::new(&binary);
    if ephemeral {
        host_cmd.arg("--ephemeral");
    }
    host_cmd
        .arg(&dir)
        .arg(&artifact)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut host = host_cmd
        .spawn()
        .map_err(|error| CliError::plain(error.to_string()))?;
    let mut adapter_child = match spawn_adapter(&prepared, &dir, &root) {
        Ok(child) => child,
        Err(error) => {
            let _ = signal(&host, libc::SIGKILL);
            let _ = host.wait();
            return Err(error);
        }
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = ctrlc::set_handler(move || {
        let _ = tx.send(());
    });
    let ready = wait_until_ready(&mut host, &mut adapter_child, &rx, &dir);
    if let Err(error) = ready {
        let _ = signal(&host, libc::SIGTERM);
        if wait_child(&mut host, SHUTDOWN_DEADLINE).is_none() {
            let _ = signal(&host, libc::SIGKILL);
            let _ = host.wait();
        }
        let _ = signal(&adapter_child, libc::SIGTERM);
        let _ = adapter_child.wait();
        clear_ready(&dir);
        return Err(error);
    }
    loop {
        match host.try_wait() {
            Ok(Some(status)) => {
                let _ = signal(&adapter_child, libc::SIGTERM);
                let _ = adapter_child.wait();
                clear_ready(&dir);
                if status.success() {
                    return Ok(());
                }
                return Err(CliError::new("Pulse host exited")
                    .detail("Status", status.code().unwrap_or(1).to_string())
                    .with_code(status.code().unwrap_or(1)));
            }
            Ok(None) => {}
            Err(error) => return Err(CliError::plain(error.to_string())),
        }
        if rx.recv_timeout(Duration::from_millis(200)).is_ok() {
            let _ = signal(&host, libc::SIGTERM);
            if let Some(status) = wait_child(&mut host, SHUTDOWN_DEADLINE) {
                let _ = signal(&adapter_child, libc::SIGTERM);
                let _ = adapter_child.wait();
                clear_ready(&dir);
                if status.success() {
                    return Ok(());
                }
                let code = status.code().unwrap_or(1);
                return Err(CliError::new("Pulse host exited")
                    .detail("Status", code.to_string())
                    .with_code(code));
            }
            let _ = signal(&host, libc::SIGKILL);
            let _ = host.wait();
            let _ = signal(&adapter_child, libc::SIGKILL);
            let _ = adapter_child.wait();
            clear_ready(&dir);
            return Err(CliError::new("Pulse shutdown exceeded 10 seconds.").with_code(1));
        }
    }
}

pub fn connect(dir: PathBuf, prepare_only: bool) -> Result<(), CliError> {
    let root = std::env::current_dir().map_err(|error| CliError::plain(error.to_string()))?;
    let prepared = prepare(&root)?;
    if prepare_only {
        println!("{}", prepared.manifest.display());
        return Ok(());
    }
    clear_ready(&dir);
    let mut adapter = spawn_adapter(&prepared, &dir, &root)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = ctrlc::set_handler(move || {
        let _ = tx.send(());
    });
    if let Err(error) = wait_adapter(&mut adapter, &rx, &dir) {
        let _ = signal(&adapter, libc::SIGTERM);
        let _ = adapter.wait();
        clear_ready(&dir);
        return Err(error);
    }
    loop {
        match adapter.try_wait() {
            Ok(Some(status)) => {
                clear_ready(&dir);
                if status.success() {
                    return Ok(());
                }
                if status.code() == Some(3) {
                    return Err(incompatible_host());
                }
                return Err(CliError::new("Pulse adapter exited")
                    .detail("Status", status.code().unwrap_or(1).to_string())
                    .with_code(status.code().unwrap_or(1)));
            }
            Ok(None) => {}
            Err(error) => return Err(CliError::plain(error.to_string())),
        }
        if rx.recv_timeout(Duration::from_millis(200)).is_ok() {
            let _ = signal(&adapter, libc::SIGTERM);
            let _ = adapter.wait();
            clear_ready(&dir);
            return Ok(());
        }
    }
}

struct Prepared {
    work: PathBuf,
    manifest: PathBuf,
    adapter: PathBuf,
    artifact: PathBuf,
}

fn prepare(root: &Path) -> Result<Prepared, CliError> {
    let loaded = config::load_config(root)?;
    let tools = paths::tools()?;
    let registry = Registry::new(&tools);
    let prepared = (|| {
        let programs = discover_pulse(&loaded.root, &loaded.programs, &registry)?;
        ensure_typescript(&programs)?;
        let helper = tools.node_helper.as_deref().ok_or_else(|| {
            CliError::plain(
            "The TypeScript compiler is missing. Reinstall the CLI with `npm install -g trigora`.",
        )
        })?;
        let adapter = helper
            .parent()
            .map(|dir| dir.join("pulse-adapter.mjs"))
            .filter(|path| path.is_file())
            .ok_or_else(|| CliError::plain("The Pulse Node adapter is missing."))?;
        let work = root.join(".trigora").join("pulse");
        fs::create_dir_all(work.join("handlers"))
            .map_err(|error| CliError::plain(error.to_string()))?;
        fs::create_dir_all(work.join("artifacts"))
            .map_err(|error| CliError::plain(error.to_string()))?;
        let mut listed = Vec::new();
        for (index, program) in programs.iter().enumerate() {
            let file = root.join(&program.file);
            let built = registry.handler_module(program, &file)?.ok_or_else(|| {
                CliError::new("Pulse supports TypeScript programs")
                    .detail("File", program.file.clone())
            })?;
            let module_source = built
                .get("moduleSource")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string();
            let keys = built
                .get("keys")
                .and_then(Json::as_array)
                .map(|keys| {
                    keys.iter()
                        .filter_map(Json::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let module = work.join("handlers").join(format!("{index}.mjs"));
            let artifact = work.join("artifacts").join(format!("{index}.json"));
            fs::write(&module, module_source)
                .map_err(|error| CliError::plain(error.to_string()))?;
            fs::write(&artifact, &program.artifact_json)
                .map_err(|error| CliError::plain(error.to_string()))?;
            listed.push(json!({
                "file": program.file,
                "module": module,
                "artifact": artifact,
                "hash": program.artifact_hash,
                "keys": keys,
            }));
        }
        let manifest = work.join("manifest.json");
        fs::write(
            &manifest,
            serde_json::to_vec_pretty(&json!({ "programs": listed })).unwrap(),
        )
        .map_err(|error| CliError::plain(error.to_string()))?;
        Ok(Prepared {
            artifact: work.join("artifacts").join("0.json"),
            work,
            manifest,
            adapter,
        })
    })();
    registry.shutdown();
    prepared
}

fn spawn_adapter(prepared: &Prepared, socket: &Path, root: &Path) -> Result<Child, CliError> {
    let mut command = Command::new("node");
    command
        .arg("--experimental-strip-types")
        .arg(&prepared.adapter)
        .arg(socket)
        .arg(&prepared.manifest)
        .current_dir(root)
        .env("TRIGORA_PULSE_EXPECT_VERSION", PULSE_VERSION)
        .env("TRIGORA_PULSE_EXPECT_PROTOCOL", PULSE_PROTOCOL.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
        .spawn()
        .map_err(|error| CliError::plain(error.to_string()))
}

fn clear_ready(dir: &Path) {
    let _ = fs::remove_file(dir.join("ready.json"));
}

fn incompatible_host() -> CliError {
    CliError::new("Incompatible Pulse host")
        .detail("CLI", format!("{PULSE_VERSION} protocol {PULSE_PROTOCOL}"))
        .hint("Use the trigora-pulse binary built with this CLI.")
}

fn require_token() -> Result<(), CliError> {
    let token = std::env::var("TRIGORA_TOKEN").ok();
    let missing = token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none();
    if missing {
        return Err(CliError::new("Durability is not configured")
            .detail("Reason", "TRIGORA_TOKEN is missing.")
            .hint("Set TRIGORA_TOKEN, or pass --ephemeral to run without saving execution state.")
            .with_code(2));
    }
    Ok(())
}

fn ensure_typescript(programs: &[Program]) -> Result<(), CliError> {
    for program in programs {
        if program.language != "typescript" {
            return Err(CliError::new("Pulse supports TypeScript programs")
                .detail("File", program.file.clone())
                .detail("Language", program.language.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typescript_is_required() {
        let program = Program {
            id: "py".into(),
            export_name: "main".into(),
            file: "src/program.py".into(),
            language: "python".into(),
            frontend_id: "python".into(),
            semantics_version: "py.subset.v1".into(),
            source: String::new(),
            artifact_json: String::new(),
            artifact_hash: String::new(),
            compiler_version: String::new(),
            effects: Vec::new(),
        };
        let error = ensure_typescript(&[program]).unwrap_err();
        assert!(error.title.contains("TypeScript"));
        assert_eq!(error.details[0].1, "src/program.py");
    }
}
