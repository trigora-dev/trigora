use std::path::PathBuf;

use serde_json::Value as Json;

#[derive(Clone, Debug)]
pub struct Effect {
    pub key: String,
    pub handler_id: Option<String>,
    pub value: Option<Json>,
    pub source: Option<String>,
    pub binary: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub id: String,
    pub export_name: String,
    pub file: String,
    pub language: String,
    pub frontend_id: String,
    pub semantics_version: String,
    pub source: String,
    pub artifact_json: String,
    pub artifact_hash: String,
    pub compiler_version: String,
    pub effects: Vec<Effect>,
}

pub fn program_slug(id: &str) -> Result<String, crate::error::CliError> {
    let chars: Vec<char> = id.chars().collect();
    let mut split = String::new();
    for (index, ch) in chars.iter().enumerate() {
        if index > 0
            && ch.is_ascii_uppercase()
            && (chars[index - 1].is_ascii_lowercase() || chars[index - 1].is_ascii_digit())
        {
            split.push('-');
        }
        split.push(*ch);
    }
    let mut slug = String::new();
    let mut dash = false;
    for ch in split.chars() {
        if ch.is_ascii_alphanumeric() {
            if dash && !slug.is_empty() {
                slug.push('-');
            }
            dash = false;
            slug.push(ch.to_ascii_lowercase());
        } else {
            dash = true;
        }
    }
    let slug = slug.chars().take(64).collect::<String>();
    let slug = slug.trim_end_matches('-').to_string();
    if !slug.starts_with(|ch: char| ch.is_ascii_lowercase())
        || !slug
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(crate::error::CliError::new("Invalid program name")
            .detail("Program", id)
            .detail(
                "Reason",
                "Cloud program names must be lowercase slugs starting with a letter.",
            ));
    }
    Ok(slug)
}
