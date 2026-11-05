use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use crate::error::CliError;
use crate::model::{Effect, Program};
use crate::paths::Tools;
use crate::versions;

mod python;
mod rust;
#[cfg(test)]
mod tests;
mod typescript;

use python::PythonAdapter;
use rust::RustAdapter;
use typescript::TypeScriptAdapter;

pub struct CompileInput<'a> {
    pub root: &'a Path,
    pub relative: &'a str,
    pub source: &'a str,
    pub program_id: &'a str,
    pub file: &'a Path,
    pub pulse: bool,
}

pub struct Compiled {
    pub artifact_json: String,
    pub artifact_hash: String,
    pub compiler_version: String,
    pub effects: Vec<Effect>,
}

pub trait LanguageAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn aliases(&self) -> &'static [&'static str];
    fn language(&self) -> &'static str;
    fn frontend_id(&self) -> &'static str;
    fn semantics_version(&self) -> &'static str;
    fn matches(&self, relative: &str) -> bool;
    fn available(&self) -> Result<(), CliError>;
    fn version(&self) -> Result<String, CliError>;
    fn program_id(&self, stem: &str, file: &Path, root: &Path, source: &str) -> String;
    fn compile(&self, input: &CompileInput<'_>) -> Result<Compiled, CliError>;
    fn run_effect(
        &self,
        effect: &Effect,
        key: &str,
        input: &Json,
    ) -> Result<Option<Json>, CliError>;
    fn effect_bundle(&self, program: &Program, root: &Path) -> Result<Option<Json>, CliError>;
    fn handler_module(
        &self,
        source: &str,
        filename: &str,
        file: &Path,
    ) -> Result<Option<Json>, CliError> {
        let _ = (source, filename, file);
        Ok(None)
    }
    fn scaffold(&self, name: &str, example: bool) -> Option<Vec<(PathBuf, String)>>;
    fn watch_paths(&self, root: &Path, programs: &[Program]) -> Vec<PathBuf> {
        let _ = (root, programs);
        Vec::new()
    }

    fn shutdown(&self) {}
}

pub struct Registry {
    adapters: Vec<Box<dyn LanguageAdapter>>,
}

impl Registry {
    pub fn new(tools: &Tools) -> Self {
        Self {
            adapters: vec![
                Box::new(TypeScriptAdapter::new(tools.node_helper.clone())),
                Box::new(PythonAdapter::default()),
                Box::new(RustAdapter::new(tools.rust_compiler.clone())),
            ],
        }
    }

    pub fn push(&mut self, adapter: Box<dyn LanguageAdapter>) {
        self.adapters.push(adapter);
    }

    pub fn matches_file(&self, name: &str) -> bool {
        self.adapters.iter().any(|adapter| adapter.matches(name))
    }

    pub fn resolve(&self, relative: &str) -> Option<&dyn LanguageAdapter> {
        self.adapters
            .iter()
            .find(|adapter| adapter.matches(relative))
            .map(|adapter| adapter.as_ref())
    }

    pub fn by_id(&self, name: &str) -> Option<&dyn LanguageAdapter> {
        let name = name.trim();
        self.adapters
            .iter()
            .find(|adapter| adapter.id() == name || adapter.aliases().contains(&name))
            .map(|adapter| adapter.as_ref())
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.adapters.iter().map(|adapter| adapter.id()).collect()
    }

    pub fn effect_bundle(&self, program: &Program, root: &Path) -> Result<Option<Json>, CliError> {
        let Some(adapter) = self.by_id(&program.language) else {
            return Ok(None);
        };
        adapter.effect_bundle(program, root)
    }

    pub fn handler_module(&self, program: &Program, file: &Path) -> Result<Option<Json>, CliError> {
        let Some(adapter) = self.by_id(&program.language) else {
            return Ok(None);
        };
        adapter.handler_module(&program.source, &program.file, file)
    }

    pub fn run_effect(
        &self,
        language: &str,
        effect: &Effect,
        key: &str,
        input: &Json,
    ) -> Result<Option<Json>, CliError> {
        let Some(adapter) = self.by_id(language) else {
            return Ok(None);
        };
        adapter.run_effect(effect, key, input)
    }

    pub fn watch_paths(&self, root: &Path, programs: &[Program]) -> Vec<PathBuf> {
        self.adapters
            .iter()
            .flat_map(|adapter| adapter.watch_paths(root, programs))
            .collect()
    }

    pub fn shutdown(&self) {
        for adapter in &self.adapters {
            adapter.shutdown();
        }
    }
}

fn extension(relative: &str) -> &str {
    relative.rsplit('.').next().unwrap_or("")
}

pub fn compare_version(adapter: &dyn LanguageAdapter, file: &str) -> Result<(), CliError> {
    let found = adapter.version()?;
    if found.is_empty() {
        return Ok(());
    }
    let Some(expected) = versions::expected(adapter.id()) else {
        return Ok(());
    };
    if found != expected {
        return Err(versions::mismatch(adapter.id(), &found, file));
    }
    Ok(())
}
