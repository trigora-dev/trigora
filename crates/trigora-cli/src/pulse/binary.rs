use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::CliError;

pub(super) const PULSE_VERSION: &str = "0.1.0";
pub(super) const PULSE_PROTOCOL: u32 = 1;

pub(super) fn resolve_pulse_bin(cwd: &Path, exe: &Path) -> Result<PathBuf, CliError> {
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

pub(super) fn host_version(binary: &Path) -> Result<(String, u32), CliError> {
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
}
