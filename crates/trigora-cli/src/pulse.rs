use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

use crate::adapter::Registry;
use crate::compile::discover_pulse;
use crate::config;
use crate::error::CliError;
use crate::model::Program;
use crate::paths;

const PULSE_VERSION: &str = "0.1.0";
const PULSE_PROTOCOL: u32 = 1;
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(10);

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
    let loaded = config::load_config(&root)?;
    let tools = paths::tools()?;
    let registry = Registry::new(&tools);
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
    let dir = root.join(".trigora").join("pulse");
    fs::create_dir_all(dir.join("handlers")).map_err(|error| CliError::plain(error.to_string()))?;
    fs::create_dir_all(dir.join("artifacts"))
        .map_err(|error| CliError::plain(error.to_string()))?;
    let mut listed = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let file = root.join(&program.file);
        let built = registry.handler_module(program, &file)?.ok_or_else(|| {
            CliError::new("Pulse supports TypeScript programs").detail("File", program.file.clone())
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
        let module = dir.join("handlers").join(format!("{index}.mjs"));
        let artifact = dir.join("artifacts").join(format!("{index}.json"));
        fs::write(&module, module_source).map_err(|error| CliError::plain(error.to_string()))?;
        fs::write(&artifact, &program.artifact_json)
            .map_err(|error| CliError::plain(error.to_string()))?;
        listed.push(json!({
            "module": module,
            "artifact": artifact,
            "hash": program.artifact_hash,
            "keys": keys,
        }));
    }
    let manifest = dir.join("manifest.json");
    fs::write(
        &manifest,
        serde_json::to_vec_pretty(&json!({ "programs": listed })).unwrap(),
    )
    .map_err(|error| CliError::plain(error.to_string()))?;
    let artifact = dir.join("artifacts").join("0.json");
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
    let mut adapter_cmd = Command::new("node");
    adapter_cmd
        .arg("--experimental-strip-types")
        .arg(&adapter)
        .arg(&dir)
        .arg(&manifest)
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut adapter_child = match adapter_cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = signal(&host, libc::SIGKILL);
            let _ = host.wait();
            return Err(CliError::plain(error.to_string()));
        }
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = ctrlc::set_handler(move || {
        let _ = tx.send(());
    });
    let ready = wait_until_ready(&mut host, &mut adapter_child, &rx, &dir);
    registry.shutdown();
    if let Err(error) = ready {
        let _ = signal(&host, libc::SIGTERM);
        if wait_child(&mut host, SHUTDOWN_DEADLINE).is_none() {
            let _ = signal(&host, libc::SIGKILL);
            let _ = host.wait();
        }
        let _ = signal(&adapter_child, libc::SIGTERM);
        let _ = adapter_child.wait();
        return Err(error);
    }
    loop {
        match host.try_wait() {
            Ok(Some(status)) => {
                let _ = signal(&adapter_child, libc::SIGTERM);
                let _ = adapter_child.wait();
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
            return Err(CliError::new("Pulse shutdown exceeded 10 seconds.").with_code(1));
        }
    }
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

pub(crate) fn resolve_pulse_bin(cwd: &Path, exe: &Path) -> Result<PathBuf, CliError> {
    if let Some(configured) = std::env::var_os("TRIGORA_PULSE_BIN") {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Ok(path);
        }
        return Err(CliError::new("The Pulse host is not installed")
            .detail("TRIGORA_PULSE_BIN", path.display().to_string())
            .hint("Point TRIGORA_PULSE_BIN at a trigora-pulse binary."));
    }
    if let Some(dir) = exe.parent() {
        let sibling = dir.join(pulse_file_name());
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    if let Some(root) = pulse_workspace(cwd) {
        let release = root.join("target").join("release").join(pulse_file_name());
        if release.is_file() {
            return Ok(release);
        }
    }
    Err(CliError::new("The Pulse host is not installed").hint(
        "Looked for TRIGORA_PULSE_BIN, trigora-pulse beside the CLI, and target/release/trigora-pulse in this repository.",
    ))
}

fn pulse_workspace(cwd: &Path) -> Option<PathBuf> {
    let mut current = Some(cwd);
    while let Some(dir) = current {
        let manifest = dir.join("crates").join("trigora-pulse").join("Cargo.toml");
        if manifest.is_file() {
            let text = fs::read_to_string(&manifest).unwrap_or_default();
            if text
                .lines()
                .any(|line| line.trim() == "name = \"trigora-pulse\"")
            {
                return Some(dir.to_path_buf());
            }
        }
        current = dir.parent();
    }
    None
}

fn pulse_file_name() -> &'static str {
    if cfg!(windows) {
        "trigora-pulse.exe"
    } else {
        "trigora-pulse"
    }
}

fn host_version(binary: &Path) -> Result<(String, u32), CliError> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|error| CliError::plain(error.to_string()))?;
    if !output.status.success() {
        return Err(CliError::new("Incompatible Pulse host").detail(
            "Reason",
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.split_whitespace();
    let name = parts.next().unwrap_or("");
    let version = parts.next().unwrap_or("").to_string();
    let word = parts.next().unwrap_or("");
    let protocol = parts.next().unwrap_or("").parse::<u32>().unwrap_or(0);
    if name != "trigora-pulse" || word != "protocol" || version.is_empty() || protocol == 0 {
        return Err(CliError::new("Incompatible Pulse host")
            .detail("Output", text.trim().to_string())
            .hint("The host did not report a Pulse version and adapter protocol."));
    }
    Ok((version, protocol))
}

fn wait_until_ready(
    host: &mut Child,
    adapter: &mut Child,
    signal: &std::sync::mpsc::Receiver<()>,
    dir: &Path,
) -> Result<(), CliError> {
    let ready = dir.join("ready.json");
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        if ready.is_file() {
            return Ok(());
        }
        if signal.try_recv().is_ok() {
            return Err(CliError::new("Pulse shutdown exceeded 10 seconds.").with_code(1));
        }
        if let Some(status) = host
            .try_wait()
            .map_err(|error| CliError::plain(error.to_string()))?
        {
            return Err(CliError::new("Pulse host exited")
                .detail("Status", status.code().unwrap_or(1).to_string())
                .with_code(status.code().unwrap_or(1)));
        }
        if let Some(status) = adapter
            .try_wait()
            .map_err(|error| CliError::plain(error.to_string()))?
        {
            return Err(CliError::new("Pulse adapter exited")
                .detail("Status", status.code().unwrap_or(1).to_string())
                .with_code(status.code().unwrap_or(1)));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(CliError::plain("Pulse did not become ready."))
}

fn wait_child(child: &mut Child, limit: Duration) -> Option<std::process::ExitStatus> {
    let started = Instant::now();
    while started.elapsed() < limit {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.try_wait().ok().flatten()
}

fn signal(child: &Child, sig: i32) -> Result<(), CliError> {
    let pid = child.id() as i32;
    let result = unsafe { libc::kill(pid, sig) };
    if result == 0 {
        Ok(())
    } else {
        Err(CliError::plain("could not signal the Pulse host"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unrelated_release_binary_is_not_selected() {
        let previous = std::env::var_os("TRIGORA_PULSE_BIN");
        std::env::remove_var("TRIGORA_PULSE_BIN");
        let root = std::env::temp_dir().join(format!("trigora-unrelated-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let release = root.join("target").join("release");
        fs::create_dir_all(&release).unwrap();
        let planted = release.join(pulse_file_name());
        fs::write(&planted, b"#!/bin/sh\necho planted\n").unwrap();
        let exe_dir = root.join("bin");
        fs::create_dir_all(&exe_dir).unwrap();
        let exe = exe_dir.join("trigora");
        fs::write(&exe, b"cli").unwrap();
        let error = resolve_pulse_bin(&root, &exe).unwrap_err();
        let rendered = format!("{} {}", error.title, error.hint.unwrap_or_default());
        assert!(rendered.contains("not installed") || rendered.contains("trigora-pulse"));
        assert!(!rendered.contains(&planted.display().to_string()));
        match previous {
            Some(value) => std::env::set_var("TRIGORA_PULSE_BIN", value),
            None => std::env::remove_var("TRIGORA_PULSE_BIN"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_release_binary_in_this_repository_is_selected() {
        let previous = std::env::var_os("TRIGORA_PULSE_BIN");
        std::env::remove_var("TRIGORA_PULSE_BIN");
        let root = std::env::temp_dir().join(format!("trigora-repo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let crate_dir = root.join("crates").join("trigora-pulse");
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(crate_dir.join("Cargo.toml"), "name = \"trigora-pulse\"\n").unwrap();
        let release = root.join("target").join("release");
        fs::create_dir_all(&release).unwrap();
        let binary = release.join(pulse_file_name());
        fs::write(&binary, b"host").unwrap();
        let exe = root.join("trigora");
        fs::write(&exe, b"cli").unwrap();
        let project = root.join("app");
        fs::create_dir_all(&project).unwrap();
        assert_eq!(resolve_pulse_bin(&project, &exe).unwrap(), binary);
        match previous {
            Some(value) => std::env::set_var("TRIGORA_PULSE_BIN", value),
            None => std::env::remove_var("TRIGORA_PULSE_BIN"),
        }
        let _ = fs::remove_dir_all(root);
    }

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
