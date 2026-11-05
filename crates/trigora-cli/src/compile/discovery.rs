use super::*;

pub(in crate::compile) fn glob_files(
    root: &Path,
    patterns: &[String],
    registry: &Registry,
) -> Vec<PathBuf> {
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

pub(in crate::compile) fn path_relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}
