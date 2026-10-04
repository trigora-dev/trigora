use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::adapter::Registry;
use crate::error::CliError;
use crate::paths::Tools;

pub fn package_name(cwd: &Path, requested: Option<&str>) -> String {
    let base = requested.unwrap_or_else(|| {
        cwd.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("app")
    });
    let slug: String = base
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.starts_with(|ch: char| ch.is_ascii_lowercase()) {
        slug
    } else {
        format!("trigora-{}", if slug.is_empty() { "app" } else { &slug })
    }
}

pub fn init(
    force: bool,
    language: Option<String>,
    name: Option<String>,
    example: Option<bool>,
) -> Result<(), CliError> {
    let cwd = std::env::current_dir().map_err(|error| CliError::plain(error.to_string()))?;
    init_at(&cwd, force, language, name, example)
}

pub(crate) fn init_at(
    cwd: &Path,
    force: bool,
    language: Option<String>,
    name: Option<String>,
    example: Option<bool>,
) -> Result<(), CliError> {
    let interactive = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    let language = match language.as_deref() {
        None | Some("") if interactive => prompt_language()?,
        None | Some("") => "typescript".to_string(),
        Some(language) => parse_language(language)?,
    };
    let name = match name.as_deref() {
        None | Some("") if interactive => Some(prompt_name(&cwd)?),
        other => other.map(str::to_string),
    };
    let example = match example {
        Some(example) => example,
        None if interactive => prompt_example()?,
        None => true,
    };
    let resolved = package_name(&cwd, name.as_deref());
    let registry = language_registry();
    let adapter = registry.by_id(&language).ok_or_else(|| {
        CliError::plain("`--language` must be `typescript`, `python`, or `rust`.")
    })?;
    let files = adapter
        .scaffold(&resolved, example)
        .ok_or_else(|| CliError::plain(format!("`{language}` has no project scaffold.")))?;
    let mut created = Vec::new();
    let mut updated = Vec::new();
    let mut skipped = Vec::new();
    for (relative, contents) in files {
        let path = cwd.join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| CliError::plain(error.to_string()))?;
        }
        let exists = path.exists();
        if exists && !force {
            skipped.push(relative);
            continue;
        }
        fs::write(&path, contents).map_err(|error| CliError::plain(error.to_string()))?;
        if exists {
            updated.push(relative);
        } else {
            created.push(relative);
        }
    }
    println!();
    println!("✔ Project initialized");
    println!();
    print_group("Created", &created);
    print_group("Updated", &updated);
    print_group("Skipped", &skipped);
    println!("Next steps");
    println!("  1. trigora dev");
    if example {
        println!("  2. start program, then send the greeted event");
    } else {
        println!("  2. add a program entry under src/");
    }
    Ok(())
}

fn language_registry() -> Registry {
    Registry::new(&Tools {
        local_bin: PathBuf::from("unused"),
        node_helper: None,
        rust_compiler: None,
    })
}

pub(crate) fn typescript_scaffold(name: &str, example: bool) -> Vec<(PathBuf, String)> {
    let mut files = vec![
        (
            PathBuf::from("trigora.toml"),
            toml_config(
                name,
                &[r#"src/**/*.ts"#, r#"src/**/*.js"#, r#"src/**/*.mjs"#],
            ),
        ),
        (PathBuf::from("package.json"), package_json(name)),
        (PathBuf::from(".env.example"), ENV_EXAMPLE.to_string()),
    ];
    if example {
        files.push((
            PathBuf::from("src/program.ts"),
            TYPESCRIPT_PROGRAM.to_string(),
        ));
    }
    files
}

pub(crate) fn python_scaffold(name: &str, example: bool) -> Vec<(PathBuf, String)> {
    let mut files = vec![
        (
            PathBuf::from("trigora.toml"),
            toml_config(name, &[r#"src/**/*.py"#]),
        ),
        (PathBuf::from("pyproject.toml"), pyproject(name)),
        (PathBuf::from(".env.example"), ENV_EXAMPLE.to_string()),
    ];
    if example {
        files.push((PathBuf::from("src/program.py"), PYTHON_PROGRAM.to_string()));
    }
    files
}

pub(crate) fn rust_scaffold(name: &str, example: bool) -> Vec<(PathBuf, String)> {
    let mut files = vec![
        (
            PathBuf::from("trigora.toml"),
            toml_config(name, &[r#"src/**/*.rs"#]),
        ),
        (PathBuf::from("Cargo.toml"), cargo(name)),
        (PathBuf::from(".env.example"), ENV_EXAMPLE.to_string()),
    ];
    if example {
        files.push((PathBuf::from("src/lib.rs"), RUST_PROGRAM.to_string()));
    }
    files
}

fn toml_config(name: &str, globs: &[&str]) -> String {
    let programs = globs
        .iter()
        .map(|pattern| format!("\"{pattern}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[project]\nname = \"{name}\"\nprograms = [{programs}]\n")
}

fn package_json(name: &str) -> String {
    format!("{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"type\": \"module\"\n}}\n")
}

fn pyproject(name: &str) -> String {
    format!("[project]\nname = \"{name}\"\nversion = \"1.0.0\"\nrequires-python = \">=3.9\"\ndependencies = [\"trigora\"]\n")
}

fn cargo(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\nedition = \"2021\"\n\n[dependencies]\n# Unpublished: trigora = {{ path = \"../trigora-rust/crates/trigora\" }}\ntrigora = \"1.0.0\"\n")
}

pub fn parse_language(language: &str) -> Result<String, CliError> {
    let language = language.trim().to_ascii_lowercase();
    match language_registry().by_id(&language) {
        Some(adapter) => Ok(adapter.id().to_string()),
        None => Err(CliError::plain(
            "`--language` must be `typescript`, `python`, or `rust`.",
        )),
    }
}

fn prompt_language() -> Result<String, CliError> {
    let answer = prompt("Language (typescript/python/rust) [typescript]: ")?;
    if answer.trim().is_empty() {
        Ok("typescript".to_string())
    } else {
        parse_language(&answer)
    }
}

fn prompt_name(cwd: &Path) -> Result<String, CliError> {
    let fallback = package_name(cwd, None);
    let answer = prompt(&format!("Project name [{fallback}]: "))?;
    let answer = answer.trim();
    Ok(if answer.is_empty() {
        fallback
    } else {
        answer.to_string()
    })
}

fn prompt_example() -> Result<bool, CliError> {
    let answer = prompt("Create an example program? [Y/n]: ")?;
    let answer = answer.trim().to_ascii_lowercase();
    Ok(answer.is_empty() || answer == "y" || answer == "yes")
}

fn prompt(label: &str) -> Result<String, CliError> {
    print!("{label}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|error| CliError::plain(error.to_string()))?;
    Ok(line)
}

fn print_group(title: &str, paths: &[PathBuf]) {
    if paths.is_empty() {
        return;
    }
    println!("{title}");
    for path in paths {
        println!("  {}", path.display());
    }
    println!();
}

const ENV_EXAMPLE: &str = "# Local runtime used by the Trigora client while `trigora dev` is running.\nTRIGORA_RUNTIME_URL=http://127.0.0.1:3477\n\n# Cloud API token (optional). When set, CLI commands talk to Trigora Cloud.\n# TRIGORA_TOKEN=\n";

const TYPESCRIPT_PROGRAM: &str = "import { effect, waitForEvent } from '@trigora/sdk';\n\nexport default async function program() {\n  const greeting = await effect('greet', () => 'hello');\n  const who = await waitForEvent('greeted');\n\n  return {\n    greeting,\n    from: who,\n  };\n}\n";

const PYTHON_PROGRAM: &str = "from trigora import effect, program, wait_for_event\n\n@program\nasync def program():\n    greeting = await effect(\"greet\", lambda: \"hello\")\n    who = await wait_for_event(\"greeted\")\n    return {\"greeting\": greeting, \"from\": who}\n";

const RUST_PROGRAM: &str = "use trigora::{effect, wait_for_event};\n\npub async fn main() -> Result<String, String> {\n    let greeting: String = effect(\"greet\", || String::from(\"hello\")).await?;\n    let _who: String = wait_for_event(\"greeted\").await?;\n    Ok(greeting)\n}\n";

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "trigora-init-{}-{}",
            std::process::id(),
            crate::paths::random_token()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scaffolds_rust_python_and_typescript() {
        let rust = temp_project();
        init_at(&rust, false, Some("rust".into()), None, Some(true)).unwrap();
        let config = fs::read_to_string(rust.join("trigora.toml")).unwrap();
        let manifest = fs::read_to_string(rust.join("Cargo.toml")).unwrap();
        let program = fs::read_to_string(rust.join("src/lib.rs")).unwrap();
        assert!(config.contains("programs = [\"src/**/*.rs\"]"));
        assert!(manifest.contains("trigora = \"1.0.0\""));
        assert!(manifest.contains("path = \"../trigora-rust/crates/trigora\""));
        assert!(program.contains("pub async fn main()"));
        assert!(program.contains("wait_for_event(\"greeted\")"));

        let python = temp_project();
        init_at(
            &python,
            false,
            Some("python".into()),
            Some("approval".into()),
            Some(true),
        )
        .unwrap();
        let config = fs::read_to_string(python.join("trigora.toml")).unwrap();
        let manifest = fs::read_to_string(python.join("pyproject.toml")).unwrap();
        let program = fs::read_to_string(python.join("src/program.py")).unwrap();
        assert!(config.contains("name = \"approval\""));
        assert!(config.contains("programs = [\"src/**/*.py\"]"));
        assert!(manifest.contains("name = \"approval\""));
        assert!(program.contains("@program"));
        assert!(program.contains("async def program()"));

        let typescript = temp_project();
        init_at(
            &typescript,
            false,
            Some("typescript".into()),
            None,
            Some(true),
        )
        .unwrap();
        let config = fs::read_to_string(typescript.join("trigora.toml")).unwrap();
        let manifest = fs::read_to_string(typescript.join("package.json")).unwrap();
        let program = fs::read_to_string(typescript.join("src/program.ts")).unwrap();
        let env_example = fs::read_to_string(typescript.join(".env.example")).unwrap();
        assert!(config.contains("\"src/**/*.ts\""));
        assert!(config.contains("\"src/**/*.js\""));
        assert!(config.contains("\"src/**/*.mjs\""));
        assert!(manifest.contains("\"type\": \"module\""));
        assert!(program.contains("export default async function program"));
        assert!(program.contains("waitForEvent('greeted')"));
        assert!(env_example.contains("TRIGORA_RUNTIME_URL=http://127.0.0.1:3477"));
        let _ = fs::remove_dir_all(rust);
        let _ = fs::remove_dir_all(python);
        let _ = fs::remove_dir_all(typescript);
    }

    #[test]
    fn skips_the_example_and_preserves_files_unless_forced() {
        let bare = temp_project();
        init_at(&bare, false, Some("typescript".into()), None, Some(false)).unwrap();
        assert!(!bare.join("src/program.ts").exists());
        assert!(bare.join("trigora.toml").is_file());

        let kept = temp_project();
        fs::create_dir_all(kept.join("src")).unwrap();
        fs::write(kept.join("trigora.toml"), "custom config").unwrap();
        fs::write(kept.join("src/program.ts"), "custom program").unwrap();
        fs::write(kept.join(".env.example"), "custom env").unwrap();
        init_at(&kept, false, Some("typescript".into()), None, Some(true)).unwrap();
        assert_eq!(
            fs::read_to_string(kept.join("trigora.toml")).unwrap(),
            "custom config"
        );
        assert_eq!(
            fs::read_to_string(kept.join("src/program.ts")).unwrap(),
            "custom program"
        );
        assert_eq!(
            fs::read_to_string(kept.join(".env.example")).unwrap(),
            "custom env"
        );

        init_at(&kept, true, Some("typescript".into()), None, Some(true)).unwrap();
        let config = fs::read_to_string(kept.join("trigora.toml")).unwrap();
        let program = fs::read_to_string(kept.join("src/program.ts")).unwrap();
        let env_example = fs::read_to_string(kept.join(".env.example")).unwrap();
        assert!(config.contains("programs = ["));
        assert!(program.contains("export default async function program"));
        assert!(env_example.contains("TRIGORA_RUNTIME_URL"));
        let _ = fs::remove_dir_all(bare);
        let _ = fs::remove_dir_all(kept);
    }
}
