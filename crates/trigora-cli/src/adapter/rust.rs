use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use super::{extension, CompileInput, Compiled, LanguageAdapter};
use crate::compile;
use crate::error::CliError;
use crate::model::{Effect, Program};

pub(super) struct RustAdapter {
    compiler: Option<PathBuf>,
}

impl RustAdapter {
    pub(super) fn new(compiler: Option<PathBuf>) -> Self {
        Self { compiler }
    }
}

impl LanguageAdapter for RustAdapter {
    fn id(&self) -> &'static str {
        "rust"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["rust", "rs"]
    }

    fn language(&self) -> &'static str {
        "rust"
    }

    fn frontend_id(&self) -> &'static str {
        "rust"
    }

    fn semantics_version(&self) -> &'static str {
        "rust.subset.v1"
    }

    fn matches(&self, relative: &str) -> bool {
        extension(relative) == "rs"
    }

    fn available(&self) -> Result<(), CliError> {
        if self.compiler.as_ref().is_some_and(|path| path.is_file()) {
            Ok(())
        } else {
            Err(missing_rust_compiler())
        }
    }

    fn version(&self) -> Result<String, CliError> {
        let Some(compiler) = self.compiler.as_ref() else {
            return Err(missing_rust_compiler());
        };
        let output = crate::paths::command_output(compiler, &["--version"], "").ok();
        let Some(output) = output else {
            return Ok(String::new());
        };
        if !output.status.success() {
            return Ok(String::new());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn program_id(&self, stem: &str, file: &Path, root: &Path, _source: &str) -> String {
        compile::rust_program_id(stem, file, root)
    }

    fn compile(&self, input: &CompileInput<'_>) -> Result<Compiled, CliError> {
        let compiler = self.compiler.as_ref().ok_or_else(missing_rust_compiler)?;
        let (artifact_json, artifact_hash, compiler_version, effects) = compile::compile_rust(
            input.source,
            input.relative,
            input.program_id,
            input.root,
            compiler,
        )?;
        Ok(Compiled {
            artifact_json,
            artifact_hash,
            compiler_version,
            effects,
        })
    }

    fn run_effect(
        &self,
        effect: &Effect,
        key: &str,
        input: &Json,
    ) -> Result<Option<Json>, CliError> {
        let Some(binary) = &effect.binary else {
            return Ok(None);
        };
        let payload = serde_json::json!({ "key": key, "input": input }).to_string();
        let output = crate::paths::command_output(binary, &[], &payload)
            .map_err(|error| CliError::plain(error.to_string()))?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr);
            let message = if message.trim().is_empty() {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            } else {
                message.trim().to_string()
            };
            return Err(CliError::plain(if message.is_empty() {
                format!("Effect `{key}` failed.")
            } else {
                message
            }));
        }
        let parsed: Json = serde_json::from_slice(&output.stdout).unwrap_or(Json::Null);
        if let Some(error) = parsed.get("error").and_then(Json::as_str) {
            return Err(CliError::plain(error));
        }
        Ok(Some(parsed.get("result").cloned().unwrap_or(Json::Null)))
    }

    fn effect_bundle(&self, program: &Program, root: &Path) -> Result<Option<Json>, CliError> {
        Ok(Some(crate::deploy::rust_effect_bundle(program, root)?))
    }

    fn scaffold(&self, name: &str, example: bool) -> Option<Vec<(PathBuf, String)>> {
        Some(crate::init::rust_scaffold(name, example))
    }

    fn watch_paths(&self, root: &Path, programs: &[Program]) -> Vec<PathBuf> {
        if programs
            .iter()
            .any(|program| program.language == self.language())
        {
            vec![root.join("Cargo.toml"), root.join("Cargo.lock")]
        } else {
            Vec::new()
        }
    }
}

fn missing_rust_compiler() -> CliError {
    CliError::new("Rust compiler unavailable")
        .detail("Reason", "The Rust compiler binary was not found.")
        .hint("Reinstall the CLI with `npm install -g trigora`. The Rust compiler is included.")
}
