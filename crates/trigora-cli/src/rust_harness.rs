use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::error::CliError;
use crate::model::Effect;

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
];

#[derive(Clone)]
struct Capture {
    name: String,
    ty: String,
}

#[derive(Clone)]
struct Closure {
    key: String,
    body: String,
    captures: Vec<Capture>,
}

struct HarnessFiles {
    cargo_toml: String,
    main_rs: String,
    lib_rs: String,
    handlers_rs: String,
}

pub fn build_effects(
    root: &Path,
    program_id: &str,
    filename: &str,
    source: &str,
    wasm: bool,
) -> Result<(Vec<Effect>, Option<Vec<u8>>), CliError> {
    let closures = extract(source).map_err(|error| {
        CliError::new("Effect harness failed to build")
            .detail("Reason", error)
            .detail("Program", program_id)
    })?;
    if closures.is_empty() {
        return Ok((Vec::new(), None));
    }
    let harness_dir = root.join(".trigora").join("effects").join(program_id);
    let cargo_file = find_cargo_toml(&root.join(filename).parent().unwrap_or(root), root);
    let dependencies = cargo_file
        .as_ref()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map(|text| {
            copied_dependencies(
                &text,
                cargo_file.as_ref().unwrap().parent().unwrap_or(root),
                &harness_dir,
            )
        })
        .unwrap_or_default();
    let files = harness_sources(&closures, &dependencies);
    let binary = build_binary(&harness_dir, &files, program_id)?;
    let wasm_bytes = if wasm {
        Some(build_wasm(&harness_dir, &files, program_id)?)
    } else {
        None
    };
    let effects = closures
        .into_iter()
        .map(|closure| Effect {
            key: closure.key,
            handler_id: None,
            value: None,
            source: None,
            binary: Some(binary.clone()),
        })
        .collect();
    Ok((effects, wasm_bytes))
}

fn build_binary(
    harness_dir: &Path,
    files: &HarnessFiles,
    program_id: &str,
) -> Result<PathBuf, CliError> {
    let stamp = stamp(files);
    let binary = harness_dir.join("target").join("debug").join(binary_name());
    let stamp_path = harness_dir.join(".built");
    if binary.is_file()
        && std::fs::read_to_string(&stamp_path).ok().as_deref() == Some(stamp.as_str())
    {
        return Ok(binary);
    }
    write_harness(harness_dir, files)?;
    cargo(
        harness_dir,
        &["build", "--quiet", "--bin", "trigora-effects"],
        "Effect harness failed to build",
        program_id,
        None,
    )?;
    let _ = std::fs::write(stamp_path, stamp);
    Ok(binary)
}

fn build_wasm(
    harness_dir: &Path,
    files: &HarnessFiles,
    program_id: &str,
) -> Result<Vec<u8>, CliError> {
    write_harness(harness_dir, files)?;
    cargo(
        harness_dir,
        &[
            "build",
            "--quiet",
            "--lib",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
        ],
        "Effect worker failed to build",
        program_id,
        Some("Install the wasm32-unknown-unknown target with rustup, then retry."),
    )?;
    let path = harness_dir
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("trigora_effects.wasm");
    std::fs::read(&path).map_err(|error| {
        CliError::new("Effect worker failed to build")
            .detail("Reason", error.to_string())
            .detail("Program", program_id)
    })
}

fn cargo(
    harness_dir: &Path,
    args: &[&str],
    title: &str,
    program_id: &str,
    hint: Option<&str>,
) -> Result<(), CliError> {
    let manifest = harness_dir.join("Cargo.toml");
    let target = harness_dir.join("target");
    let mut command = Command::new("cargo");
    command
        .args(args)
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target);
    let output = command.output().map_err(|error| {
        let mut err = CliError::new(title)
            .detail("Reason", error.to_string())
            .detail("Program", program_id);
        if let Some(hint) = hint {
            err = err.hint(hint);
        }
        err
    })?;
    if output.status.success() {
        return Ok(());
    }
    let reason = String::from_utf8_lossy(&output.stderr);
    let reason = if reason.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        reason.trim().to_string()
    };
    let mut err = CliError::new(title)
        .detail(
            "Reason",
            if reason.is_empty() {
                "cargo build failed".into()
            } else {
                reason
            },
        )
        .detail("Program", program_id);
    if let Some(hint) = hint {
        err = err.hint(hint);
    }
    Err(err)
}

fn write_harness(harness_dir: &Path, files: &HarnessFiles) -> Result<(), CliError> {
    let src = harness_dir.join("src");
    std::fs::create_dir_all(&src).map_err(|error| CliError::plain(error.to_string()))?;
    std::fs::write(harness_dir.join("Cargo.toml"), &files.cargo_toml)
        .and_then(|_| std::fs::write(src.join("main.rs"), &files.main_rs))
        .and_then(|_| std::fs::write(src.join("lib.rs"), &files.lib_rs))
        .and_then(|_| std::fs::write(src.join("handlers.rs"), &files.handlers_rs))
        .map_err(|error| CliError::plain(error.to_string()))
}

fn stamp(files: &HarnessFiles) -> String {
    let mut hasher = Sha256::new();
    hasher.update(files.cargo_toml.as_bytes());
    hasher.update(files.main_rs.as_bytes());
    hasher.update(files.lib_rs.as_bytes());
    hasher.update(files.handlers_rs.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "trigora-effects.exe"
    } else {
        "trigora-effects"
    }
}

fn extract(source: &str) -> Result<Vec<Closure>, String> {
    let masked = mask(source);
    let aliases = aliases(&masked);
    let mut effects = Vec::new();
    let mut seen = Vec::new();
    let chars: Vec<char> = masked.chars().collect();
    let source_chars: Vec<char> = source.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let Some(ident) = read_ident(&chars, index) else {
            index += 1;
            continue;
        };
        if !aliases.iter().any(|alias| alias == &ident)
            || ident_char(chars.get(index.wrapping_sub(1)).copied())
        {
            index += ident.chars().count();
            continue;
        }
        let open = skip_space(&chars, index + ident.chars().count());
        if chars.get(open) != Some(&'(') {
            index += ident.chars().count();
            continue;
        }
        let call = split_arguments(&source_chars, open)?;
        let key = call.args.first().and_then(|arg| string_literal(arg));
        let Some(key) = key else {
            index = call.end;
            continue;
        };
        let Some(body_arg) = call.args.get(1) else {
            index = call.end;
            continue;
        };
        if seen.iter().any(|seen_key| seen_key == &key) {
            return Err(format!("Duplicate effect key `{key}`."));
        }
        seen.push(key.clone());
        let body = parse_closure(body_arg)?;
        let mut captures = Vec::new();
        for name in captures_in(&body) {
            captures.push(Capture {
                name: name.clone(),
                ty: capture_type(source, &name, open)?,
            });
        }
        effects.push(Closure {
            key,
            body,
            captures,
        });
        index = call.end;
    }
    Ok(effects)
}

fn harness_sources(effects: &[Closure], dependencies: &str) -> HarnessFiles {
    let functions = effects
        .iter()
        .enumerate()
        .map(|(index, effect)| {
            let bindings = effect
                .captures
                .iter()
                .map(|capture| format!("    {}", binder(capture)))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "fn effect_{index}(input: &serde_json::Value) -> Result<serde_json::Value, String> {{\n{bindings}\n    let value = {{ {} }};\n    serde_json::to_value(value).map_err(|error| error.to_string())\n}}",
                effect.body
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let arms = effects
        .iter()
        .enumerate()
        .map(|(index, effect)| {
            format!(
                "        {} => effect_{index}(&input),",
                rust_string(&effect.key)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let handlers_rs = format!(
        r#"use serde_json::Value;

fn require_f64(input: &Value, name: &str) -> Result<f64, String> {{
    input.get(name).and_then(Value::as_f64).ok_or_else(|| format!("missing {{name}}"))
}}

fn require_bool(input: &Value, name: &str) -> Result<bool, String> {{
    input.get(name).and_then(Value::as_bool).ok_or_else(|| format!("missing {{name}}"))
}}

fn require_string(input: &Value, name: &str) -> Result<String, String> {{
    input
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing {{name}}"))
}}

{functions}

pub fn dispatch(key: &str, input: &Value) -> Result<Value, String> {{
    match key {{
{arms}
        _ => Err(format!("unknown effect {{key}}")),
    }}
}}
"#
    );
    HarnessFiles {
        cargo_toml: format!(
            r#"[package]
name = "trigora-effects"
version = "0.0.0"
edition = "2021"
publish = false

[lib]
crate-type = ["cdylib"]
path = "src/lib.rs"

[[bin]]
name = "trigora-effects"
path = "src/main.rs"

[profile.release]
lto = true
opt-level = "s"
panic = "abort"

[dependencies]
serde_json = "1"
{dependencies}"#
        ),
        main_rs: r#"mod handlers;

use serde_json::Value;

fn main() {
    let request: Value = serde_json::from_reader(std::io::stdin()).unwrap_or(serde_json::json!({}));
    let key = request.get("key").and_then(Value::as_str).unwrap_or("");
    let input = request.get("input").cloned().unwrap_or(serde_json::json!({}));
    match handlers::dispatch(key, &input) {
        Ok(value) => println!("{}", serde_json::json!({"result": value})),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}
"#
        .to_string(),
        lib_rs: include_str_lib(),
        handlers_rs,
    }
}

fn include_str_lib() -> String {
    r#"mod handlers;

static mut LAST_OUT: *mut u8 = std::ptr::null_mut();
static mut LAST_OUT_LEN: usize = 0;
static mut LAST_OUT_CAP: usize = 0;
static mut RESPONSE_LEN: usize = 0;
static mut ALLOC_PTR: *mut u8 = std::ptr::null_mut();
static mut ALLOC_CAP: usize = 0;

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    unsafe { buf.set_len(len); }
    let ptr = buf.as_mut_ptr();
    unsafe {
        ALLOC_PTR = ptr;
        ALLOC_CAP = buf.capacity();
    }
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub extern "C" fn out_len() -> usize {
    unsafe { RESPONSE_LEN }
}

#[no_mangle]
pub extern "C" fn handle(ptr: *mut u8, len: usize) -> *mut u8 {
    let cap = unsafe { if ptr == ALLOC_PTR { ALLOC_CAP } else { len } };
    let input = unsafe { Vec::from_raw_parts(ptr, len, cap) };
    let response = match serde_json::from_slice::<serde_json::Value>(&input) {
        Ok(request) => {
            let _secret_env = SecretEnvGuard::apply(&request);
            let key = request.get("key").and_then(serde_json::Value::as_str);
            let payload = request.get("input");
            match (key, payload) {
                (Some(key), Some(payload)) => match handlers::dispatch(key, payload) {
                    Ok(value) => serde_json::json!({ "result": value }),
                    Err(message) => serde_json::json!({ "error": message }),
                },
                _ => serde_json::json!({ "error": "effect request requires key and input" }),
            }
        }
        Err(error) => serde_json::json!({ "error": error.to_string() }),
    };
    let bytes = serde_json::to_vec(&response).unwrap_or_else(|error| {
        serde_json::to_vec(&serde_json::json!({ "error": error.to_string() })).unwrap()
    });
    remember_response(bytes)
}

struct SecretEnvGuard {
    previous: Vec<(String, Option<String>)>,
}

impl SecretEnvGuard {
    fn apply(request: &serde_json::Value) -> Self {
        let mut previous = Vec::new();
        if let Some(env) = request.get("secretEnv").and_then(|value| value.as_object()) {
            for (key, value) in env {
                let Some(value) = value.as_str() else {
                    continue;
                };
                let prior = std::env::var(key).ok();
                std::env::set_var(key, value);
                previous.push((key.clone(), prior));
            }
        }
        Self { previous }
    }
}

impl Drop for SecretEnvGuard {
    fn drop(&mut self) {
        for (key, prior) in self.previous.drain(..) {
            match prior {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn remember_response(bytes: Vec<u8>) -> *mut u8 {
    unsafe {
        if !LAST_OUT.is_null() {
            drop(Vec::from_raw_parts(LAST_OUT, LAST_OUT_LEN, LAST_OUT_CAP));
        }
    }
    let ptr = bytes.as_ptr() as *mut u8;
    unsafe {
        LAST_OUT = ptr;
        LAST_OUT_LEN = bytes.len();
        LAST_OUT_CAP = bytes.capacity();
        RESPONSE_LEN = bytes.len();
    }
    std::mem::forget(bytes);
    ptr
}
"#
    .to_string()
}

fn binder(capture: &Capture) -> String {
    match capture.ty.as_str() {
        "f64" => format!(
            "let {}: f64 = require_f64(input, {})?;",
            capture.name,
            rust_string(&capture.name)
        ),
        "bool" => format!(
            "let {}: bool = require_bool(input, {})?;",
            capture.name,
            rust_string(&capture.name)
        ),
        _ => format!(
            "let {}: String = require_string(input, {})?;",
            capture.name,
            rust_string(&capture.name)
        ),
    }
}

fn rust_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn mask(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut output = String::new();
    let mut index = 0;
    while index < chars.len() {
        let current = chars[index];
        let next = chars.get(index + 1).copied();
        if current == '/' && next == Some('/') {
            output.push_str("  ");
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                output.push(' ');
                index += 1;
            }
            continue;
        }
        if current == '/' && next == Some('*') {
            output.push_str("  ");
            index += 2;
            while index < chars.len()
                && !(chars[index] == '*' && chars.get(index + 1) == Some(&'/'))
            {
                output.push(if chars[index] == '\n' { '\n' } else { ' ' });
                index += 1;
            }
            output.push_str("  ");
            index += 2;
            continue;
        }
        if current == '"' {
            output.push(' ');
            index += 1;
            while index < chars.len() && chars[index] != '"' {
                if chars[index] == '\\' {
                    output.push_str("  ");
                    index += 2;
                    continue;
                }
                output.push(if chars[index] == '\n' { '\n' } else { ' ' });
                index += 1;
            }
            output.push(' ');
            index += 1;
            continue;
        }
        output.push(current);
        index += 1;
    }
    output
}

fn aliases(masked: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (start, _) in masked.match_indices("use ") {
        let rest = &masked[start + 4..];
        let Some(end) = rest.find(';') else { continue };
        let statement = rest[..end].trim();
        let Some((crate_name, tree)) = statement.split_once("::") else {
            continue;
        };
        if crate_name != "trigora" && crate_name != "tcc_rust_prelude" {
            continue;
        }
        let tree = tree.trim();
        let parts: Vec<&str> = if let Some(inner) = tree.strip_prefix('{') {
            inner
                .rsplit_once('}')
                .map(|(body, _)| body.split(',').collect())
                .unwrap_or_default()
        } else {
            vec![tree]
        };
        for part in parts {
            let mut pieces = part.trim().split(" as ");
            let name = pieces.next().unwrap_or("").trim();
            let alias = pieces.next().unwrap_or(name).trim();
            if name == "effect" {
                found.push(alias.to_string());
            }
        }
    }
    found
}

fn ident_char(char: Option<char>) -> bool {
    char.is_some_and(|char| char.is_ascii_alphanumeric() || char == '_')
}

fn read_ident(chars: &[char], index: usize) -> Option<String> {
    let first = *chars.get(index)?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let mut end = index + 1;
    while ident_char(chars.get(end).copied()) {
        end += 1;
    }
    Some(chars[index..end].iter().collect())
}

fn skip_space(chars: &[char], mut index: usize) -> usize {
    while chars.get(index).is_some_and(|char| char.is_whitespace()) {
        index += 1;
    }
    index
}

struct Call {
    args: Vec<String>,
    end: usize,
}

fn split_arguments(chars: &[char], open: usize) -> Result<Call, String> {
    let mut args = Vec::new();
    let mut start = open + 1;
    let mut depth = 1;
    let mut index = open + 1;
    while index < chars.len() {
        let char = chars[index];
        if char == '(' || char == '[' || char == '{' {
            depth += 1;
            index += 1;
            continue;
        }
        if char == ')' || char == ']' || char == '}' {
            depth -= 1;
            if depth == 0 {
                let last: String = chars[start..index]
                    .iter()
                    .collect::<String>()
                    .trim()
                    .to_string();
                if !last.is_empty() {
                    args.push(last);
                }
                return Ok(Call { args, end: index });
            }
            index += 1;
            continue;
        }
        if char == ',' && depth == 1 {
            args.push(
                chars[start..index]
                    .iter()
                    .collect::<String>()
                    .trim()
                    .to_string(),
            );
            start = index + 1;
        }
        index += 1;
    }
    Err("Unclosed effect call.".into())
}

fn parse_closure(argument: &str) -> Result<String, String> {
    let mut body = argument.trim();
    if let Some(rest) = body.strip_prefix("move") {
        body = rest.trim();
    }
    if !body.starts_with('|') {
        return Err("An effect callback must be a closure.".into());
    }
    let Some(close) = body[1..].find('|') else {
        return Err("An effect callback must be a closure.".into());
    };
    if !body[1..close + 1].trim().is_empty() {
        return Err("Effect closures take captured bindings, not parameters.".into());
    }
    Ok(body[close + 2..].trim().to_string())
}

fn string_literal(argument: &str) -> Option<String> {
    let argument = argument.trim();
    let inner = argument.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn captures_in(body: &str) -> Vec<String> {
    let local = declared_in_body(body);
    let chars: Vec<char> = body.chars().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let Some(ident) = read_ident(&chars, index) else {
            index += 1;
            continue;
        };
        let previous = if index == 0 {
            None
        } else {
            chars.get(index - 1).copied()
        };
        let after = skip_space(&chars, index + ident.chars().count());
        let next = chars.get(after).copied();
        let call = next == Some('(') || next == Some('!');
        let path = next == Some(':');
        let field = previous == Some('.');
        if !field
            && !call
            && !path
            && !KEYWORDS.contains(&ident.as_str())
            && !local.iter().any(|name| name == &ident)
            && !found.iter().any(|name| name == &ident)
        {
            found.push(ident.clone());
        }
        index += ident.chars().count();
    }
    found
}

fn declared_in_body(body: &str) -> Vec<String> {
    let mut names = Vec::new();
    for (start, _) in body.match_indices("let ") {
        let rest = body[start + 4..].trim_start();
        let rest = rest.strip_prefix("mut ").unwrap_or(rest);
        let name: String = rest
            .chars()
            .take_while(|char| char.is_ascii_alphanumeric() || *char == '_')
            .collect();
        if !name.is_empty()
            && (name
                .chars()
                .next()
                .is_some_and(|char| char.is_ascii_alphabetic() || char == '_'))
        {
            names.push(name);
        }
    }
    names
}

fn capture_type(source: &str, name: &str, before: usize) -> Result<String, String> {
    let prelude: String = source.chars().take(before).collect();
    let pattern = format!(r"(?:let\s+(?:mut\s+)?|[(,]\s*){name}\s*:\s*([A-Za-z0-9_:<>,\s]+)");
    let regex = regex::Regex::new(&pattern).map_err(|error| error.to_string())?;
    let mut ty = None;
    for captures in regex.captures_iter(&prelude) {
        ty = captures.get(1).map(|item| item.as_str().trim().to_string());
    }
    let ty = ty.map(|ty| {
        if ty == "std::string::String" {
            "String".into()
        } else {
            ty
        }
    });
    match ty.as_deref() {
        Some("f64" | "bool" | "String") => Ok(ty.unwrap()),
        Some(other) => Err(format!(
            "Effect capture `{name}` has type `{other}`, which the harness cannot bind. Use f64, bool, or String."
        )),
        None => Err(format!(
            "Effect capture `{name}` needs a type ascription (`let {name}: f64`, `bool`, or `String`)."
        )),
    }
}

fn find_cargo_toml(start: &Path, root: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if !current.starts_with(root) && current != root {
            return None;
        }
        let candidate = current.join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn copied_dependencies(cargo_toml: &str, from_dir: &Path, harness_dir: &Path) -> String {
    let lines: Vec<&str> = cargo_toml.lines().collect();
    let Some(start) = lines
        .iter()
        .position(|line| line.trim() == "[dependencies]")
    else {
        return String::new();
    };
    let mut copied = Vec::new();
    for line in lines.iter().skip(start + 1) {
        if line.trim_start().starts_with('[') {
            break;
        }
        if line.trim().is_empty() || line.trim().starts_with('#') {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("trigora")
            || trimmed.starts_with("tcc-rust-prelude")
            || trimmed.starts_with("serde_json")
        {
            continue;
        }
        copied.push(rewrite_paths(line, from_dir, harness_dir));
    }
    if copied.is_empty() {
        String::new()
    } else {
        format!("{}\n", copied.join("\n"))
    }
}

fn rewrite_paths(line: &str, from_dir: &Path, harness_dir: &Path) -> String {
    let mut output = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("path") {
        output.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(eq) = after.find('=') else {
            output.push_str(after);
            break;
        };
        let quoted = after[eq + 1..].trim_start();
        if !quoted.starts_with('"') {
            output.push_str(after);
            break;
        }
        let Some(end) = quoted[1..].find('"') else {
            output.push_str(after);
            break;
        };
        let relative = &quoted[1..end + 1];
        let absolute = from_dir.join(relative);
        let mut next = pathdiff(harness_dir, &absolute);
        if !next.starts_with('.') {
            next = format!("./{next}");
        }
        output.push_str("path = \"");
        output.push_str(&next.replace('\\', "/"));
        output.push('"');
        rest = &quoted[end + 2..];
    }
    if !rest.is_empty() && !output.ends_with(rest) {
        output.push_str(rest);
    }
    output
}

fn pathdiff(from: &Path, to: &Path) -> String {
    let from = std::fs::canonicalize(from).unwrap_or_else(|_| from.to_path_buf());
    let to = std::fs::canonicalize(to).unwrap_or_else(|_| to.to_path_buf());
    let mut ups = 0;
    let mut base = from.as_path();
    while !to.starts_with(base) {
        ups += 1;
        match base.parent() {
            Some(parent) => base = parent,
            None => break,
        }
    }
    let mut relative = std::path::PathBuf::new();
    for _ in 0..ups {
        relative.push("..");
    }
    if let Ok(rest) = to.strip_prefix(base) {
        relative.push(rest);
    }
    relative.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifts_an_f64_capture_into_the_harness() {
        let source = r#"
use trigora::effect;

pub async fn main(amount: f64) -> Result<String, String> {
    effect("charge", move || {
        let label = format!("{amount}");
        label
    });
    Ok(String::from("ok"))
}
"#;
        let closures = extract(source).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            closures
                .iter()
                .flat_map(|closure| closure.captures.iter().map(|capture| capture.name.as_str()))
                .collect::<Vec<_>>(),
            vec!["amount"]
        );
        assert_eq!(closures.len(), 1);
        assert_eq!(closures[0].captures[0].ty, "f64");
        let files = harness_sources(&closures, "");
        assert!(files.handlers_rs.contains("require_f64(input, \"amount\")"));
        assert!(files.handlers_rs.contains("format!(\"{amount}\")"));
    }

    #[test]
    fn a_program_without_effects_builds_no_harness_source() {
        let source = "pub async fn main() -> Result<String, String> { Ok(String::from(\"ok\")) }\n";
        let closures = extract(source).expect("extract");
        assert!(closures.is_empty());
    }
}
