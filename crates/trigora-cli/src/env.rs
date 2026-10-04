use std::fs;
use std::path::Path;

use crate::error::CliError;

pub const DEFAULT_RUNTIME_URL: &str = "http://127.0.0.1:3477";
pub const DEFAULT_CLOUD_URL: &str = "https://api.trigora.dev";

pub fn load_project_env(root: &Path) {
    load_env_file(&root.join(".env"));
    load_env_file(&root.join(".env.local"));
}

fn load_env_file(path: &Path) {
    let Ok(source) = fs::read_to_string(path) else {
        return;
    };
    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || std::env::var_os(key).is_some() {
            continue;
        }
        let mut value = value.trim().to_string();
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            value = value[1..value.len() - 1].to_string();
        }
        std::env::set_var(key, value);
    }
}

pub fn api_token() -> Option<String> {
    let value = std::env::var("TRIGORA_TOKEN").ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn runtime_url() -> String {
    std::env::var("TRIGORA_RUNTIME_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_RUNTIME_URL.to_string())
}

pub fn cloud_url() -> String {
    std::env::var("TRIGORA_API_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_CLOUD_URL.to_string())
}

pub fn token_missing(title: &str) -> CliError {
    CliError::new(title)
        .detail("Reason", "TRIGORA_TOKEN is not set.")
        .hint("Set TRIGORA_TOKEN. Create a token at https://cloud.trigora.dev.")
}
