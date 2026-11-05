use super::extract::{Capture, Closure};
use super::HarnessFiles;

pub(in crate::rust_harness) fn harness_sources(
    effects: &[Closure],
    dependencies: &str,
) -> HarnessFiles {
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
{dependencies}
[workspace]
"#
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
