use std::path::{Path, PathBuf};

pub(in crate::rust_harness) fn find_cargo_toml(start: &Path, root: &Path) -> Option<PathBuf> {
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

pub(in crate::rust_harness) fn copied_dependencies(
    cargo_toml: &str,
    from_dir: &Path,
    harness_dir: &Path,
) -> String {
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
