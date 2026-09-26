use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;

use serde_json::{json, Value as Json};

use crate::adapter::Registry;
use crate::error::CliError;
use crate::model::{Effect, Program};
use crate::paths;
use crate::versions::RUST_FRONTEND;

struct HelperIo {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next: u64,
}

pub struct Helper {
    inner: Mutex<HelperIo>,
}

impl Helper {
    pub fn spawn(path: &Path) -> Result<Self, CliError> {
        let mut child = Command::new("node")
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                CliError::plain(format!(
                    "The Node helper is missing. Reinstall trigora. {error}"
                ))
            })?;
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Ok(Self {
            inner: Mutex::new(HelperIo {
                child,
                stdin,
                stdout,
                next: 0,
            }),
        })
    }

    pub fn call(&self, mut body: Json) -> Result<Json, CliError> {
        let mut inner = self.inner.lock().expect("helper");
        inner.next += 1;
        let id = inner.next;
        body["id"] = Json::from(id);
        writeln!(inner.stdin, "{body}").map_err(|error| CliError::plain(error.to_string()))?;
        let _ = inner.stdin.flush();
        let mut line = String::new();
        inner
            .stdout
            .read_line(&mut line)
            .map_err(|error| CliError::plain(error.to_string()))?;
        serde_json::from_str(line.trim())
            .map_err(|error| CliError::plain(format!("Node helper returned invalid JSON: {error}")))
    }

    pub fn shutdown(&self) {
        let mut inner = self.inner.lock().expect("helper");
        let _ = writeln!(inner.stdin, r#"{{"id":0,"op":"shutdown"}}"#);
        let _ = inner.stdin.flush();
        let _ = inner.child.wait();
    }
}

pub fn compiler_version(helper: &Helper) -> Result<String, CliError> {
    let response = helper.call(json!({"op": "version"}))?;
    Ok(response
        .get("compilerVersion")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string())
}

pub fn discover(
    root: &Path,
    globs: &[String],
    registry: &Registry,
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

pub(crate) fn compile_typescript(
    helper: &Helper,
    source: &str,
    filename: &str,
    program_id: &str,
) -> Result<(String, String, String, Vec<Effect>), CliError> {
    let response = helper.call(json!({
        "op": "compile",
        "source": source,
        "filename": filename,
        "programId": program_id,
    }))?;
    if response.get("ok").and_then(Json::as_bool) == Some(false) {
        return Err(compile_failure(
            "Compilation failed",
            filename,
            response
                .get("error")
                .and_then(Json::as_str)
                .unwrap_or("Compilation failed."),
        ));
    }
    let effects = response
        .get("effects")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|effect| Effect {
            key: effect
                .get("key")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string(),
            handler_id: effect.get("id").and_then(Json::as_str).map(str::to_string),
            value: None,
            source: effect
                .get("source")
                .and_then(Json::as_str)
                .map(str::to_string),
            binary: None,
        })
        .collect();
    Ok((
        response
            .get("artifactJson")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
        response
            .get("artifactHash")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
        response
            .get("compilerVersion")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
        effects,
    ))
}

pub(crate) fn compile_rust(
    source: &str,
    filename: &str,
    program_id: &str,
    root: &Path,
    compiler: &Path,
) -> Result<(String, String, String, Vec<Effect>), CliError> {
    let file = paths::temp_file("program.rs", source)
        .map_err(|error| CliError::plain(error.to_string()))?;
    let output =
        paths::command_output(compiler, &[file.to_str().unwrap_or("")], "").map_err(|error| {
            CliError::new("Rust compiler unavailable")
                .detail("File", filename)
                .detail("Reason", error.to_string())
                .hint("Reinstall trigora. The Rust compiler is included with the CLI.")
        })?;
    let _ = std::fs::remove_file(file);
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        let message = message.trim();
        return Err(compile_failure(
            "Compilation failed",
            filename,
            if message.is_empty() {
                "Rust compilation failed."
            } else {
                message
            },
        ));
    }
    let artifact_json = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let artifact: Json = serde_json::from_str(&artifact_json).map_err(|_| {
        CliError::new("Invalid program artifact")
            .detail("File", filename)
            .detail("Reason", "The compiler did not return JSON.")
    })?;
    let version = artifact
        .pointer("/envelope/frontend_version")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string();
    if !version.is_empty() && version != RUST_FRONTEND {
        return Err(CliError::new("Rust compiler version mismatch")
            .message(format!("This CLI requires Rust compiler {RUST_FRONTEND}."))
            .detail("File", filename)
            .detail("Found", version));
    }
    let (effects, _) =
        crate::rust_harness::build_effects(root, program_id, filename, source, false)?;
    let hash = artifact
        .pointer("/envelope/artifact_hash")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string();
    Ok((artifact_json, hash, version, effects))
}

pub fn rust_effect_wasm(root: &Path, program: &Program) -> Result<Option<Vec<u8>>, CliError> {
    if program.effects.is_empty() {
        return Ok(None);
    }
    let (_, wasm) = crate::rust_harness::build_effects(
        root,
        &program.id,
        &program.file,
        &program.source,
        true,
    )?;
    Ok(wasm)
}

pub(crate) fn compile_failure(title: &str, file: &str, message: &str) -> CliError {
    let mut error = CliError::new(title).message(message);
    if !file.is_empty() {
        error = error.detail("File", file);
    }
    error.detail("Reason", message)
}

pub(crate) fn default_export(source: &str) -> String {
    for line in source.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("export default async function ") {
            let name: String = rest
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '$')
                .collect();
            if !name.is_empty() {
                return name;
            }
        }
    }
    "default".to_string()
}

fn export_name(artifact_json: &str, file: &str) -> Result<String, CliError> {
    let artifact: Json = serde_json::from_str(artifact_json).map_err(|_| {
        CliError::new("Invalid program artifact")
            .detail("File", file)
            .detail("Reason", "The compiler did not return JSON.")
    })?;
    let entry = artifact.pointer("/program/entry");
    let functions = artifact
        .pointer("/program/functions")
        .and_then(Json::as_array);
    let function = functions.and_then(|functions| {
        functions
            .iter()
            .find(|function| match (function.get("id"), entry) {
                (Some(id), Some(entry)) => id == entry,
                _ => false,
            })
            .or_else(|| {
                entry
                    .and_then(Json::as_u64)
                    .and_then(|index| functions.get(index as usize))
            })
    });
    let name = function
        .and_then(|function| function.get("name"))
        .and_then(Json::as_str)
        .unwrap_or("");
    if entry.is_none() || name.is_empty() {
        return Err(CliError::new("Invalid program artifact")
            .detail("File", file)
            .detail("Reason", "program.entry does not name a function."));
    }
    Ok(name.to_string())
}

pub(crate) fn rust_program_id(stem: &str, file: &Path, root: &Path) -> String {
    if stem != "lib" && stem != "main" {
        return stem.to_string();
    }
    let mut dir = file.parent().unwrap_or(root);
    loop {
        let cargo = dir.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(source) = std::fs::read_to_string(cargo) {
                for line in source.lines() {
                    let line = line.trim();
                    if let Some(rest) = line.strip_prefix("name") {
                        let rest = rest.trim().trim_start_matches('=').trim().trim_matches('"');
                        if !rest.is_empty() {
                            return rest.to_string();
                        }
                    }
                }
            }
        }
        if dir == root {
            break;
        }
        let Some(parent) = dir.parent() else { break };
        if !parent.starts_with(root) && parent != root {
            break;
        }
        dir = parent;
    }
    stem.to_string()
}

fn glob_files(root: &Path, patterns: &[String], registry: &Registry) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(root, &mut files, registry);
    let mut matches = Vec::new();
    for pattern in patterns {
        let pattern = pattern.trim().trim_start_matches("./");
        if !pattern.contains('*') && !pattern.contains('?') {
            let absolute = root.join(pattern);
            if absolute.is_file() {
                matches.push(absolute);
            }
            continue;
        }
        let regex = glob_to_regex(pattern);
        for file in &files {
            let relative = path_relative(root, file);
            if regex_match(&regex, &relative) {
                matches.push(file.clone());
            }
        }
    }
    matches.sort();
    matches.dedup();
    matches
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>, registry: &Registry) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(
                name.as_ref(),
                "node_modules" | "dist" | ".git" | "target" | ".trigora"
            ) {
                continue;
            }
            walk(&path, files, registry);
        } else if path.is_file() && registry.matches_file(&name) {
            files.push(path);
        }
    }
}

fn glob_to_regex(pattern: &str) -> String {
    let mut expression = String::from("^");
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '*' && chars.get(index + 1) == Some(&'*') {
            if chars.get(index + 2) == Some(&'/') {
                expression.push_str("(?:.*/)?");
                index += 3;
            } else {
                expression.push_str(".*");
                index += 2;
            }
            continue;
        }
        if ch == '*' {
            expression.push_str("[^/]*");
            index += 1;
            continue;
        }
        if ch == '?' {
            expression.push_str("[^/]");
            index += 1;
            continue;
        }
        if " .+^${}()|[]\\".contains(ch) {
            expression.push('\\');
        }
        expression.push(ch);
        index += 1;
    }
    expression.push('$');
    expression
}

fn regex_match(pattern: &str, value: &str) -> bool {
    regex::Regex::new(pattern).is_ok_and(|regex| regex.is_match(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejected_rust_program_does_not_get_a_harness() {
        let root =
            std::env::temp_dir().join(format!("trigora-reject-{}", crate::paths::random_token()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        let compiler = root.join("fail-compile");
        std::fs::write(&compiler, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&compiler).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&compiler, permissions).unwrap();
        }
        let source = "use trigora::effect;\npub async fn main() -> Result<String, String> {\n    effect(\"charge\", || 1);\n    Ok(String::from(\"ok\"))\n}\n";
        let error =
            compile_rust(source, "src/program.rs", "program", &root, &compiler).unwrap_err();
        assert_eq!(error.title, "Compilation failed");
        assert!(!root.join(".trigora").join("effects").exists());
        let _ = std::fs::remove_dir_all(root);
    }
}

fn path_relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}
