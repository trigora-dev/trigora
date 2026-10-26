use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value as Json;

use super::{extension, CompileInput, Compiled, LanguageAdapter};
use crate::error::CliError;
use crate::model::{Effect, Program};

#[derive(Default)]
pub(super) struct PythonAdapter {
    reported: Mutex<Option<String>>,
}

impl LanguageAdapter for PythonAdapter {
    fn id(&self) -> &'static str {
        "python"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["python", "py"]
    }

    fn language(&self) -> &'static str {
        "python"
    }

    fn frontend_id(&self) -> &'static str {
        "python"
    }

    fn semantics_version(&self) -> &'static str {
        "py.subset.v1"
    }

    fn matches(&self, relative: &str) -> bool {
        extension(relative) == "py"
    }

    fn available(&self) -> Result<(), CliError> {
        self.version().map(|_| ())
    }

    fn version(&self) -> Result<String, CliError> {
        if let Some(version) = self.reported.lock().expect("python version").clone() {
            return Ok(version);
        }
        let version = crate::python::installed_version("program.py")?;
        *self.reported.lock().expect("python version") = Some(version.clone());
        Ok(version)
    }

    fn program_id(&self, stem: &str, _file: &Path, _root: &Path, _source: &str) -> String {
        stem.to_string()
    }

    fn compile(&self, input: &CompileInput<'_>) -> Result<Compiled, CliError> {
        let compiled = crate::python::compile(input.source, input.relative)?;
        Ok(Compiled {
            artifact_json: compiled.artifact_json,
            artifact_hash: compiled.artifact_hash,
            compiler_version: compiled.compiler_version,
            effects: compiled.effects,
        })
    }

    fn run_effect(
        &self,
        effect: &Effect,
        _key: &str,
        _input: &Json,
    ) -> Result<Option<Json>, CliError> {
        Ok(effect.value.clone())
    }

    fn effect_bundle(&self, program: &Program, _root: &Path) -> Result<Option<Json>, CliError> {
        Ok(Some(crate::deploy::python_effect_bundle(program)))
    }

    fn scaffold(&self, name: &str, example: bool) -> Option<Vec<(PathBuf, String)>> {
        Some(crate::init::python_scaffold(name, example))
    }
}
