use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::error::CliError;
use crate::model::Effect;

mod dependencies;
mod extract;
mod generate;

use dependencies::{copied_dependencies, find_cargo_toml};
use extract::extract;
use generate::harness_sources;

struct HarnessFiles {
    cargo_toml: String,
    main_rs: String,
    lib_rs: String,
    handlers_rs: String,
}

pub fn build_effects(
    root: &Path,
    program_id: &str,
    filename: &str,
    source: &str,
    wasm: bool,
) -> Result<(Vec<Effect>, Option<Vec<u8>>), CliError> {
    let closures = extract(source).map_err(|error| {
        CliError::new("Effect harness failed to build")
            .detail("Reason", error)
            .detail("Program", program_id)
    })?;
    if closures.is_empty() {
        return Ok((Vec::new(), None));
    }
    let harness_dir = root.join(".trigora").join("effects").join(program_id);
    let cargo_file = find_cargo_toml(&root.join(filename).parent().unwrap_or(root), root);
    let dependencies = cargo_file
        .as_ref()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map(|text| {
            copied_dependencies(
                &text,
                cargo_file.as_ref().unwrap().parent().unwrap_or(root),
                &harness_dir,
            )
        })
        .unwrap_or_default();
    let files = harness_sources(&closures, &dependencies);
    let binary = build_binary(&harness_dir, &files, program_id)?;
    let wasm_bytes = if wasm {
        Some(build_wasm(&harness_dir, &files, program_id)?)
    } else {
        None
    };
    let effects = closures
        .into_iter()
        .map(|closure| Effect {
            key: closure.key,
            handler_id: None,
            value: None,
            source: None,
            binary: Some(binary.clone()),
        })
        .collect();
    Ok((effects, wasm_bytes))
}

fn build_binary(
    harness_dir: &Path,
    files: &HarnessFiles,
    program_id: &str,
) -> Result<PathBuf, CliError> {
    let stamp = stamp(files);
    let binary = harness_dir.join("target").join("debug").join(binary_name());
    let stamp_path = harness_dir.join(".built");
    if binary.is_file()
        && std::fs::read_to_string(&stamp_path).ok().as_deref() == Some(stamp.as_str())
    {
        return Ok(binary);
    }
    write_harness(harness_dir, files)?;
    cargo(
        harness_dir,
        &["build", "--quiet", "--bin", "trigora-effects"],
        "Effect harness failed to build",
        program_id,
        None,
    )?;
    let _ = std::fs::write(stamp_path, stamp);
    Ok(binary)
}

fn build_wasm(
    harness_dir: &Path,
    files: &HarnessFiles,
    program_id: &str,
) -> Result<Vec<u8>, CliError> {
    write_harness(harness_dir, files)?;
    cargo(
        harness_dir,
        &[
            "build",
            "--quiet",
            "--lib",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
        ],
        "Effect worker failed to build",
        program_id,
        Some("Install the wasm32-unknown-unknown target with rustup, then retry."),
    )?;
    let path = harness_dir
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("trigora_effects.wasm");
    std::fs::read(&path).map_err(|error| {
        CliError::new("Effect worker failed to build")
            .detail("Reason", error.to_string())
            .detail("Program", program_id)
    })
}

fn cargo(
    harness_dir: &Path,
    args: &[&str],
    title: &str,
    program_id: &str,
    hint: Option<&str>,
) -> Result<(), CliError> {
    let manifest = harness_dir.join("Cargo.toml");
    let target = harness_dir.join("target");
    let mut command = Command::new("cargo");
    command
        .args(args)
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target);
    let output = command.output().map_err(|error| {
        let mut err = CliError::new(title)
            .detail("Reason", error.to_string())
            .detail("Program", program_id);
        if let Some(hint) = hint {
            err = err.hint(hint);
        }
        err
    })?;
    if output.status.success() {
        return Ok(());
    }
    let reason = String::from_utf8_lossy(&output.stderr);
    let reason = if reason.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        reason.trim().to_string()
    };
    let mut err = CliError::new(title)
        .detail(
            "Reason",
            if reason.is_empty() {
                "cargo build failed".into()
            } else {
                reason
            },
        )
        .detail("Program", program_id);
    if let Some(hint) = hint {
        err = err.hint(hint);
    }
    Err(err)
}

fn write_harness(harness_dir: &Path, files: &HarnessFiles) -> Result<(), CliError> {
    let src = harness_dir.join("src");
    std::fs::create_dir_all(&src).map_err(|error| CliError::plain(error.to_string()))?;
    std::fs::write(harness_dir.join("Cargo.toml"), &files.cargo_toml)
        .and_then(|_| std::fs::write(src.join("main.rs"), &files.main_rs))
        .and_then(|_| std::fs::write(src.join("lib.rs"), &files.lib_rs))
        .and_then(|_| std::fs::write(src.join("handlers.rs"), &files.handlers_rs))
        .map_err(|error| CliError::plain(error.to_string()))
}

fn stamp(files: &HarnessFiles) -> String {
    let mut hasher = Sha256::new();
    hasher.update(files.cargo_toml.as_bytes());
    hasher.update(files.main_rs.as_bytes());
    hasher.update(files.lib_rs.as_bytes());
    hasher.update(files.handlers_rs.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "trigora-effects.exe"
    } else {
        "trigora-effects"
    }
}
