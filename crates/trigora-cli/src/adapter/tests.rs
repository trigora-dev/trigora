use super::{extension, *};

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
    let root = std::env::temp_dir().join(format!("trigora-fake-{}", crate::paths::random_token()));
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
    let root = std::env::temp_dir().join(format!("trigora-probe-{}", crate::paths::random_token()));
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
