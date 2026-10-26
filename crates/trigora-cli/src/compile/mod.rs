use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value as Json};

use crate::adapter::Registry;
use crate::error::CliError;
use crate::model::{Effect, Program};
use crate::paths;
use crate::versions::RUST_FRONTEND;

mod discovery;
mod frontend;

pub(crate) use frontend::{
    compile_failure, compile_rust, compile_typescript, default_export, rust_program_id,
};
pub use frontend::{compiler_version, rust_effect_wasm, Helper};

use discovery::{glob_files, path_relative};
use frontend::export_name;

pub fn discover(
    root: &Path,
    globs: &[String],
    registry: &Registry,
) -> Result<Vec<Program>, CliError> {
    discover_in(root, globs, registry, false)
}

pub fn discover_pulse(
    root: &Path,
    globs: &[String],
    registry: &Registry,
) -> Result<Vec<Program>, CliError> {
    discover_in(root, globs, registry, true)
}

fn discover_in(
    root: &Path,
    globs: &[String],
    registry: &Registry,
    pulse: bool,
) -> Result<Vec<Program>, CliError> {
    let files = glob_files(root, globs, registry);
    if files.is_empty() {
        return Err(CliError::new("No programs found")
            .detail("Globs", globs.join(", "))
            .detail("Root", root.display().to_string())
            .hint("TypeScript default-exports an async program entry, Python marks one with `@program`, and Rust uses `pub async fn main`. Point `[project].programs` at those files in trigora.toml."));
    }
    let mut discovered = Vec::new();
    let mut seen: Vec<(String, String)> = Vec::new();
    for file in files {
        let relative = path_relative(root, &file);
        let Some(adapter) = registry.resolve(&relative) else {
            return Err(CliError::new("No adapter for program")
                .detail("File", relative.clone())
                .hint("Install the toolchain for this language, or point `[project].programs` at files an installed adapter understands."));
        };
        adapter.available()?;
        crate::adapter::compare_version(adapter, &relative)?;
        let source =
            std::fs::read_to_string(&file).map_err(|error| CliError::plain(error.to_string()))?;
        let stem = file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("program");
        let id = adapter.program_id(stem, &file, root, &source);
        let compiled = adapter.compile(&crate::adapter::CompileInput {
            root,
            relative: &relative,
            source: &source,
            program_id: &id,
            file: &file,
            pulse,
        })?;
        let program = Program {
            id: id.clone(),
            export_name: export_name(&compiled.artifact_json, &relative)?,
            file: relative.clone(),
            language: adapter.language().to_string(),
            frontend_id: adapter.frontend_id().to_string(),
            semantics_version: adapter.semantics_version().to_string(),
            source,
            artifact_json: compiled.artifact_json,
            artifact_hash: compiled.artifact_hash,
            compiler_version: compiled.compiler_version,
            effects: compiled.effects,
        };
        if let Some((_, first)) = seen.iter().find(|(seen_id, _)| seen_id == &id) {
            return Err(CliError::new("Duplicate program id")
                .detail("Program", id)
                .detail("First", first.clone())
                .detail("Second", relative)
                .hint("Use a unique default-export function name or file name for each program."));
        }
        seen.push((id, relative));
        discovered.push(program);
    }
    discovered.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(discovered)
}
