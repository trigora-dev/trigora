use super::*;

struct HelperIo {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    stderr: Arc<Mutex<String>>,
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
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                CliError::plain(format!(
                    "The TypeScript compiler is missing. Reinstall the CLI with `npm install -g trigora`. {error}"
                ))
            })?;
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let stderr = Arc::new(Mutex::new(String::new()));
        let captured = Arc::clone(&stderr);
        let pipe = child.stderr.take().expect("stderr");
        std::thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let mut log = captured.lock().expect("helper stderr");
                        log.push_str(&line);
                        if log.len() > 4_000 {
                            let extra = log.len() - 4_000;
                            log.drain(..extra);
                        }
                    }
                }
            }
        });
        Ok(Self {
            inner: Mutex::new(HelperIo {
                child,
                stdin,
                stdout,
                stderr,
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
        serde_json::from_str(line.trim()).map_err(|error| {
            let extra = inner.stderr.lock().expect("helper stderr").clone();
            let extra = extra.trim();
            if extra.is_empty() {
                CliError::plain(format!(
                    "The TypeScript compiler returned invalid JSON: {error}"
                ))
            } else {
                CliError::plain(format!(
                    "The TypeScript compiler returned invalid JSON: {error}\n{extra}"
                ))
            }
        })
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

pub(crate) fn compile_typescript(
    helper: &Helper,
    source: &str,
    filename: &str,
    program_id: &str,
    pulse: bool,
) -> Result<(String, String, String, Vec<Effect>), CliError> {
    let response = helper.call(json!({
        "op": "compile",
        "source": source,
        "filename": filename,
        "programId": program_id,
        "pulse": pulse,
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
                .hint("Reinstall the CLI with `npm install -g trigora`. The Rust compiler is included.")
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

pub(in crate::compile) fn export_name(artifact_json: &str, file: &str) -> Result<String, CliError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejected_rust_program_does_not_get_a_harness() {
        let root =
            std::env::temp_dir().join(format!("trigora-reject-{}", crate::paths::random_token()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        let compiler = failing_compiler(&root);
        let source = "use trigora::effect;\npub async fn main() -> Result<String, String> {\n    effect(\"charge\", || 1);\n    Ok(String::from(\"ok\"))\n}\n";
        let error =
            compile_rust(source, "src/program.rs", "program", &root, &compiler).unwrap_err();
        assert_eq!(error.title, "Compilation failed");
        assert!(!root.join(".trigora").join("effects").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    fn failing_compiler(root: &std::path::Path) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let compiler = root.join("fail-compile");
        std::fs::write(&compiler, "#!/bin/sh\nexit 1\n").unwrap();
        let mut permissions = std::fs::metadata(&compiler).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&compiler, permissions).unwrap();
        compiler
    }

    #[cfg(windows)]
    fn failing_compiler(root: &std::path::Path) -> std::path::PathBuf {
        let compiler = root.join("fail-compile.cmd");
        std::fs::write(&compiler, "@echo off\r\nexit /b 1\r\n").unwrap();
        compiler
    }
}
