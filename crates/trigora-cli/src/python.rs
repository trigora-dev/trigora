use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{json, Value as Json};

use crate::error::CliError;
use crate::model::Effect;
use crate::versions::PYTHON_FRONTEND;

const VERSION_SCRIPT: &str = r#"
import json, sys
try:
    from tcc_engine import PACKAGE_VERSION
except ModuleNotFoundError:
    json.dump({"status": "missing"}, sys.stdout)
    raise SystemExit(0)
json.dump({"status": "ok", "version": PACKAGE_VERSION}, sys.stdout)
"#;

const COMPILE_SCRIPT: &str = r#"
import json, sys
from tcc_engine import CompileError, artifact_json, compile

filename = sys.argv[1]
source = sys.stdin.read()
try:
    artifact = compile(source, filename=filename)
    sys.stdout.write(artifact_json(artifact))
except CompileError as err:
    payload = {"ok": False, "message": str(err), "file": getattr(err, "filename", filename)}
    span = getattr(err, "span", None)
    if span:
        payload["span"] = span
    json.dump(payload, sys.stderr)
    sys.exit(2)
"#;

const EFFECT_SCRIPT: &str = r#"
import ast, json, sys
source = sys.stdin.read()
tree = ast.parse(source, filename=sys.argv[1])
aliases = {}
handlers = {}
for statement in tree.body:
    if isinstance(statement, ast.ImportFrom) and statement.module == "trigora":
        for alias in statement.names:
            aliases[alias.asname or alias.name] = alias.name

class Visitor(ast.NodeVisitor):
    def visit_Call(self, node):
        if isinstance(node.func, ast.Name) and aliases.get(node.func.id) == "effect":
            if (
                len(node.args) >= 2
                and isinstance(node.args[0], ast.Constant)
                and isinstance(node.args[0].value, str)
            ):
                key = node.args[0].value
                callback = node.args[1]
                if not isinstance(callback, ast.Lambda):
                    raise SystemExit(f"effect `{key}` must use a lambda callback")
                fn = eval(compile(ast.Expression(callback), "<effect>", "eval"), {"__builtins__": {}})
                handlers[key] = fn()
        self.generic_visit(node)

Visitor().visit(tree)
json.dump(handlers, sys.stdout)
"#;

const EFFECT_SOURCE_SCRIPT: &str = r#"
import ast, json, sys
source = sys.stdin.read()
tree = ast.parse(source, filename=sys.argv[1])
aliases = {}
handlers = {}
for statement in tree.body:
    if isinstance(statement, ast.ImportFrom) and statement.module == "trigora":
        for alias in statement.names:
            aliases[alias.asname or alias.name] = alias.name

class Visitor(ast.NodeVisitor):
    def visit_Call(self, node):
        if isinstance(node.func, ast.Name) and aliases.get(node.func.id) == "effect":
            if (
                len(node.args) >= 2
                and isinstance(node.args[0], ast.Constant)
                and isinstance(node.args[0].value, str)
            ):
                key = node.args[0].value
                callback = node.args[1]
                handlers[key] = ast.unparse(callback)
        self.generic_visit(node)

Visitor().visit(tree)
json.dump(handlers, sys.stdout)
"#;

pub struct PythonProgram {
    pub artifact_json: String,
    pub artifact_hash: String,
    pub compiler_version: String,
    pub effects: Vec<Effect>,
}

pub fn installed_version(filename: &str) -> Result<String, CliError> {
    let probed = run(VERSION_SCRIPT, filename, "")?;
    let payload: Json = serde_json::from_str(&probed.0).unwrap_or(Json::Null);
    if !probed.2 || payload.get("status").and_then(Json::as_str) == Some("missing") {
        return Err(missing(filename));
    }
    Ok(payload
        .get("version")
        .and_then(Json::as_str)
        .unwrap_or("unknown")
        .to_string())
}

pub fn compile(source: &str, filename: &str) -> Result<PythonProgram, CliError> {
    let found = installed_version(filename)?;
    check_version(true, Some("ok"), Some(&found), filename)?;
    let compiled = run(COMPILE_SCRIPT, filename, source)?;
    if !compiled.2 {
        let raw = if compiled.1.is_empty() {
            compiled.0.clone()
        } else {
            compiled.1.clone()
        };
        let payload: Json = serde_json::from_str(&raw).unwrap_or(Json::Null);
        let message = payload.get("message").and_then(Json::as_str).unwrap_or(
            if compiled.1.trim().is_empty() {
                "Python compilation failed."
            } else {
                compiled.1.trim()
            },
        );
        if message.contains("No module named") && message.contains("tcc_engine") {
            return Err(missing(filename));
        }
        return Err(crate::compile::compile_failure(
            "Compilation failed",
            filename,
            message,
        ));
    }
    let artifact: Json = serde_json::from_str(compiled.0.trim()).map_err(|_| {
        CliError::new("Invalid program artifact")
            .detail("File", filename)
            .detail("Reason", "The compiler did not return JSON.")
    })?;
    let extracted = run(EFFECT_SCRIPT, filename, source)?;
    if !extracted.2 {
        return Err(crate::compile::compile_failure(
            "Failed to load effect handlers",
            filename,
            if extracted.1.trim().is_empty() {
                "Could not evaluate effect callbacks."
            } else {
                extracted.1.trim()
            },
        ));
    }
    let values: Json = serde_json::from_str(if extracted.0.trim().is_empty() {
        "{}"
    } else {
        extracted.0.trim()
    })
    .unwrap_or(json!({}));
    let sources = run(EFFECT_SOURCE_SCRIPT, filename, source).ok();
    let source_map: Json = sources
        .as_ref()
        .and_then(|item| serde_json::from_str(item.0.trim()).ok())
        .unwrap_or(json!({}));
    let mut effects = Vec::new();
    if let Some(object) = values.as_object() {
        for (key, value) in object {
            effects.push(Effect {
                key: key.clone(),
                handler_id: None,
                value: Some(value.clone()),
                source: source_map
                    .get(key)
                    .and_then(Json::as_str)
                    .map(str::to_string),
                binary: None,
            });
        }
    }
    Ok(PythonProgram {
        artifact_json: compiled.0,
        artifact_hash: artifact
            .pointer("/envelope/artifact_hash")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
        compiler_version: artifact
            .pointer("/envelope/frontend_version")
            .and_then(Json::as_str)
            .unwrap_or(PYTHON_FRONTEND)
            .to_string(),
        effects,
    })
}

pub fn check_version(
    ok: bool,
    status: Option<&str>,
    version: Option<&str>,
    filename: &str,
) -> Result<(), CliError> {
    if !ok || status == Some("missing") {
        return Err(missing(filename));
    }
    let found = version.unwrap_or("unknown");
    if found != PYTHON_FRONTEND {
        return Err(CliError::new("Python compiler version mismatch")
            .message(format!(
                "This CLI requires Python compiler {PYTHON_FRONTEND}, installed with the trigora authoring package."
            ))
            .detail("File", filename)
            .detail("Found", found)
            .detail("Install", "python3 -m pip install trigora"));
    }
    Ok(())
}

fn missing(filename: &str) -> CliError {
    CliError::new("Python compiler unavailable")
        .message("Python compiler support is not installed.")
        .detail("File", filename)
        .detail("Install", "python3 -m pip install trigora")
}

fn run(script: &str, filename: &str, source: &str) -> Result<(String, String, bool), CliError> {
    let output = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(filename)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut input) = child.stdin.take() {
                input.write_all(source.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map_err(|_| missing(filename))?;
    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_pythonpath<T>(path: &std::path::Path, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().expect("python env");
        let previous = std::env::var("PYTHONPATH").ok();
        std::env::set_var("PYTHONPATH", path);
        let result = body();
        match previous {
            Some(value) => std::env::set_var("PYTHONPATH", value),
            None => std::env::remove_var("PYTHONPATH"),
        }
        result
    }

    #[test]
    fn missing_tcc_engine_names_the_install_command() {
        let dir = std::env::temp_dir().join(format!("trigora-py-missing-{}", std::process::id()));
        let package = dir.join("tcc_engine");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("__init__.py"),
            "raise ModuleNotFoundError(\"No module named 'tcc_engine'\")\n",
        )
        .unwrap();
        let error = with_pythonpath(&dir, || {
            match compile("async def program():\n    return 1\n", "src/program.py") {
                Err(error) => error,
                Ok(_) => panic!("expected a missing compiler"),
            }
        });
        let text = format!("{:?} {:?}", error.message, error.details);
        assert!(text.contains("python3 -m pip install trigora"), "{text}");
        assert_eq!(error.title, "Python compiler unavailable");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn version_mismatch_names_the_expected_frontend() {
        let dir = std::env::temp_dir().join(format!("trigora-py-mismatch-{}", std::process::id()));
        let package = dir.join("tcc_engine");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("__init__.py"), "PACKAGE_VERSION = \"9.9.9\"\n").unwrap();
        let error = with_pythonpath(&dir, || {
            match compile("async def program():\n    return 1\n", "src/program.py") {
                Err(error) => error,
                Ok(_) => panic!("expected a version mismatch"),
            }
        });
        let text = format!("{:?} {:?}", error.message, error.details);
        assert!(text.contains(PYTHON_FRONTEND), "{text}");
        assert_eq!(error.title, "Python compiler version mismatch");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn constant_effect_value_comes_from_python() {
        let engine = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tcc-engine/bindings/python/python");
        let source = "from trigora import effect, program\n\n@program\nasync def program():\n    return await effect(\"n\", lambda: 7)\n";
        let compiled = with_pythonpath(&engine, || compile(source, "src/program.py").unwrap());
        assert_eq!(compiled.compiler_version, PYTHON_FRONTEND);
        assert_eq!(compiled.effects.len(), 1);
        assert_eq!(compiled.effects[0].key, "n");
        assert_eq!(compiled.effects[0].value, Some(json!(7)));
    }
}
