use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value as Json;

use super::{extension, CompileInput, Compiled, LanguageAdapter};
use crate::compile::{self, Helper};
use crate::error::CliError;
use crate::model::{Effect, Program};

pub(super) struct TypeScriptAdapter {
    helper_path: Option<PathBuf>,
    helper: Mutex<Option<Helper>>,
    reported: Mutex<Option<String>>,
}

impl TypeScriptAdapter {
    pub(super) fn new(helper_path: Option<PathBuf>) -> Self {
        Self {
            helper_path,
            helper: Mutex::new(None),
            reported: Mutex::new(None),
        }
    }

    fn ensure(&self) -> Result<&Path, CliError> {
        self.helper_path
            .as_deref()
            .filter(|path| path.is_file())
            .ok_or_else(|| {
                CliError::plain(
                    "The TypeScript compiler is missing. Reinstall the CLI with `npm install -g trigora`.",
                )
            })
    }

    fn helper(&self) -> Result<std::sync::MutexGuard<'_, Option<Helper>>, CliError> {
        let path = self.ensure()?.to_path_buf();
        let mut guard = self.helper.lock().expect("typescript helper");
        if guard.is_none() {
            *guard = Some(Helper::spawn(&path)?);
        }
        Ok(guard)
    }

    fn stop_helper(&self) {
        if let Some(helper) = self.helper.lock().expect("typescript helper").take() {
            helper.shutdown();
        }
    }
}

impl LanguageAdapter for TypeScriptAdapter {
    fn id(&self) -> &'static str {
        "typescript"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["typescript", "ts"]
    }

    fn language(&self) -> &'static str {
        "typescript"
    }

    fn frontend_id(&self) -> &'static str {
        "typescript"
    }

    fn semantics_version(&self) -> &'static str {
        "ts.subset.v1"
    }

    fn matches(&self, relative: &str) -> bool {
        matches!(extension(relative), "ts" | "mts" | "js" | "mjs")
    }

    fn available(&self) -> Result<(), CliError> {
        self.ensure().map(|_| ())
    }

    fn version(&self) -> Result<String, CliError> {
        if let Some(version) = self.reported.lock().expect("version").clone() {
            return Ok(version);
        }
        let guard = self.helper()?;
        let helper = guard.as_ref().expect("helper");
        let version = compile::compiler_version(helper)?;
        *self.reported.lock().expect("version") = Some(version.clone());
        Ok(version)
    }

    fn program_id(&self, stem: &str, _file: &Path, _root: &Path, source: &str) -> String {
        let export = compile::default_export(source);
        if export == "default" {
            stem.to_string()
        } else {
            export
        }
    }

    fn compile(&self, input: &CompileInput<'_>) -> Result<Compiled, CliError> {
        let guard = self.helper()?;
        let helper = guard.as_ref().expect("helper");
        let (artifact_json, artifact_hash, compiler_version, effects) =
            compile::compile_typescript(
                helper,
                input.source,
                input.relative,
                input.program_id,
                input.pulse,
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
        _key: &str,
        input: &Json,
    ) -> Result<Option<Json>, CliError> {
        let Some(handler_id) = &effect.handler_id else {
            return Ok(None);
        };
        let guard = self.helper()?;
        let helper = guard.as_ref().expect("helper");
        let response = helper.call(serde_json::json!({
            "op": "effect",
            "handlerId": handler_id,
            "input": input,
        }))?;
        if response.get("ok").and_then(Json::as_bool) == Some(false) {
            let message = response
                .get("error")
                .and_then(Json::as_str)
                .unwrap_or("effect failed");
            return Err(CliError::plain(message));
        }
        Ok(Some(response.get("value").cloned().unwrap_or(Json::Null)))
    }

    fn effect_bundle(&self, program: &Program, _root: &Path) -> Result<Option<Json>, CliError> {
        Ok(Some(crate::deploy::typescript_effect_bundle(program)))
    }

    fn handler_module(
        &self,
        source: &str,
        filename: &str,
        file: &Path,
    ) -> Result<Option<Json>, CliError> {
        let guard = self.helper()?;
        let helper = guard.as_ref().expect("helper");
        let response = helper.call(serde_json::json!({
            "op": "handlerModule",
            "source": source,
            "filename": filename,
            "file": file,
        }))?;
        if response.get("ok").and_then(Json::as_bool) == Some(false) {
            let message = response
                .get("error")
                .and_then(Json::as_str)
                .unwrap_or("effect handler failed");
            return Err(CliError::new("Effect handler cannot be loaded").detail("Reason", message));
        }
        Ok(Some(response))
    }

    fn scaffold(&self, name: &str, example: bool) -> Option<Vec<(PathBuf, String)>> {
        Some(crate::init::typescript_scaffold(name, example))
    }

    fn shutdown(&self) {
        self.stop_helper();
    }
}
