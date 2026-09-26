use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value as Json;

use crate::compile::{self, Helper};
use crate::error::CliError;
use crate::model::{Effect, Program};
use crate::paths::Tools;
use crate::versions;

pub struct CompileInput<'a> {
    pub root: &'a Path,
    pub relative: &'a str,
    pub source: &'a str,
    pub program_id: &'a str,
    pub file: &'a Path,
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

struct TypeScriptAdapter {
    helper_path: Option<PathBuf>,
    helper: Mutex<Option<Helper>>,
    reported: Mutex<Option<String>>,
}

impl TypeScriptAdapter {
    fn new(helper_path: Option<PathBuf>) -> Self {
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
            .ok_or_else(|| CliError::plain("The Node helper is missing. Reinstall trigora."))
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
            compile::compile_typescript(helper, input.source, input.relative, input.program_id)?;
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

    fn scaffold(&self, name: &str, example: bool) -> Option<Vec<(PathBuf, String)>> {
        Some(crate::init::typescript_scaffold(name, example))
    }

    fn shutdown(&self) {
        self.stop_helper();
    }
}

#[derive(Default)]
struct PythonAdapter {
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

struct RustAdapter {
    compiler: Option<PathBuf>,
}

impl RustAdapter {
    fn new(compiler: Option<PathBuf>) -> Self {
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
        .hint("Reinstall trigora. The Rust compiler is included with the CLI.")
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

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeAdapter;

    impl LanguageAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn aliases(&self) -> &'static [&'static str] {
            &["fake"]
        }
        fn language(&self) -> &'static str {
            "fake"
        }
        fn frontend_id(&self) -> &'static str {
            "fake"
        }
        fn semantics_version(&self) -> &'static str {
            "fake.subset.v1"
        }
        fn matches(&self, relative: &str) -> bool {
            extension(relative) == "fake"
        }
        fn available(&self) -> Result<(), CliError> {
            Ok(())
        }
        fn version(&self) -> Result<String, CliError> {
            Ok("0.0.0".into())
        }
        fn program_id(&self, stem: &str, _: &Path, _: &Path, _: &str) -> String {
            stem.to_string()
        }
        fn compile(&self, _: &CompileInput<'_>) -> Result<Compiled, CliError> {
            Ok(Compiled {
                artifact_json: r#"{"envelope":{"artifact_hash":"abc","frontend_version":"0.0.0"},"program":{"entry":0,"functions":[{"id":0,"name":"program"}]}}"#.into(),
                artifact_hash: "abc".into(),
                compiler_version: "0.0.0".into(),
                effects: Vec::new(),
            })
        }
        fn run_effect(&self, _: &Effect, _: &str, _: &Json) -> Result<Option<Json>, CliError> {
            Ok(None)
        }
        fn effect_bundle(&self, _: &Program, _: &Path) -> Result<Option<Json>, CliError> {
            Ok(None)
        }
        fn scaffold(&self, _: &str, _: bool) -> Option<Vec<(PathBuf, String)>> {
            None
        }
    }

    #[test]
    fn a_fake_extension_is_selected_without_a_language_branch() {
        let root =
            std::env::temp_dir().join(format!("trigora-fake-{}", crate::paths::random_token()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/program.fake"), "program\n").unwrap();
        let mut registry = Registry::new(&Tools {
            local_bin: PathBuf::from("missing"),
            node_helper: None,
            rust_compiler: None,
        });
        registry.push(Box::new(FakeAdapter));
        let programs =
            crate::compile::discover(&root, &[String::from("src/**/*.fake")], &registry).unwrap();
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].language, "fake");
        assert_eq!(programs[0].id, "program");
        assert!(registry
            .effect_bundle(&programs[0], &root)
            .unwrap()
            .is_none());
        assert!(FakeAdapter.scaffold("demo", true).is_none());
        assert!(FakeAdapter
            .run_effect(
                &Effect {
                    key: "n".into(),
                    handler_id: None,
                    value: None,
                    source: None,
                    binary: None,
                },
                "n",
                &Json::Null,
            )
            .unwrap()
            .is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    struct CountingAdapter {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl LanguageAdapter for CountingAdapter {
        fn id(&self) -> &'static str {
            "counting"
        }
        fn aliases(&self) -> &'static [&'static str] {
            &[]
        }
        fn language(&self) -> &'static str {
            "counting"
        }
        fn frontend_id(&self) -> &'static str {
            "counting"
        }
        fn semantics_version(&self) -> &'static str {
            "counting.subset.v1"
        }
        fn matches(&self, relative: &str) -> bool {
            extension(relative) == "never"
        }
        fn available(&self) -> Result<(), CliError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        fn version(&self) -> Result<String, CliError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok("0.0.0".into())
        }
        fn program_id(&self, stem: &str, _: &Path, _: &Path, _: &str) -> String {
            stem.to_string()
        }
        fn compile(&self, _: &CompileInput<'_>) -> Result<Compiled, CliError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(CliError::plain("counting adapter was compiled"))
        }
        fn run_effect(&self, _: &Effect, _: &str, _: &Json) -> Result<Option<Json>, CliError> {
            Ok(None)
        }
        fn effect_bundle(&self, _: &Program, _: &Path) -> Result<Option<Json>, CliError> {
            Ok(None)
        }
        fn scaffold(&self, _: &str, _: bool) -> Option<Vec<(PathBuf, String)>> {
            None
        }
    }

    #[test]
    fn only_the_matched_adapter_is_probed() {
        let root =
            std::env::temp_dir().join(format!("trigora-probe-{}", crate::paths::random_token()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/program.fake"), "program\n").unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut registry = Registry::new(&Tools {
            local_bin: PathBuf::from("missing"),
            node_helper: None,
            rust_compiler: None,
        });
        registry.push(Box::new(CountingAdapter {
            calls: std::sync::Arc::clone(&calls),
        }));
        registry.push(Box::new(FakeAdapter));
        crate::compile::discover(&root, &[String::from("src/**/*.fake")], &registry).unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    struct PinnedAdapter {
        id: &'static str,
        found: &'static str,
    }

    impl LanguageAdapter for PinnedAdapter {
        fn id(&self) -> &'static str {
            self.id
        }
        fn aliases(&self) -> &'static [&'static str] {
            &[]
        }
        fn language(&self) -> &'static str {
            self.id
        }
        fn frontend_id(&self) -> &'static str {
            self.id
        }
        fn semantics_version(&self) -> &'static str {
            "pinned.subset.v1"
        }
        fn matches(&self, _: &str) -> bool {
            false
        }
        fn available(&self) -> Result<(), CliError> {
            Ok(())
        }
        fn version(&self) -> Result<String, CliError> {
            Ok(self.found.to_string())
        }
        fn program_id(&self, stem: &str, _: &Path, _: &Path, _: &str) -> String {
            stem.to_string()
        }
        fn compile(&self, _: &CompileInput<'_>) -> Result<Compiled, CliError> {
            Err(CliError::plain("unused"))
        }
        fn run_effect(&self, _: &Effect, _: &str, _: &Json) -> Result<Option<Json>, CliError> {
            Ok(None)
        }
        fn effect_bundle(&self, _: &Program, _: &Path) -> Result<Option<Json>, CliError> {
            Ok(None)
        }
        fn scaffold(&self, _: &str, _: bool) -> Option<Vec<(PathBuf, String)>> {
            None
        }
    }

    #[test]
    fn version_compare_uses_the_cli_table() {
        let error = compare_version(
            &PinnedAdapter {
                id: "python",
                found: "9.9.9",
            },
            "src/program.py",
        )
        .unwrap_err();
        assert_eq!(error.title, "Python compiler version mismatch");
        let message = error.message.unwrap();
        assert!(
            message.contains(crate::versions::PYTHON_FRONTEND),
            "{message}"
        );
        assert!(compare_version(
            &PinnedAdapter {
                id: "python",
                found: crate::versions::PYTHON_FRONTEND,
            },
            "src/program.py",
        )
        .is_ok());
        assert!(compare_version(
            &PinnedAdapter {
                id: "fake",
                found: "0.0.0",
            },
            "src/program.fake",
        )
        .is_ok());
    }
}
