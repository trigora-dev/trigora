const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
];

#[derive(Clone)]
pub(in crate::rust_harness) struct Capture {
    pub(in crate::rust_harness) name: String,
    pub(in crate::rust_harness) ty: String,
}

#[derive(Clone)]
pub(in crate::rust_harness) struct Closure {
    pub(in crate::rust_harness) key: String,
    pub(in crate::rust_harness) body: String,
    pub(in crate::rust_harness) captures: Vec<Capture>,
}

pub(in crate::rust_harness) fn extract(source: &str) -> Result<Vec<Closure>, String> {
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
        if chars[index] == '"' {
            index = scan_string_captures(&chars, index, &local, &mut found);
            continue;
        }
        if let Some(end) = end_of_char_literal(&chars, index) {
            index = end;
            continue;
        }
        let Some(ident) = read_ident(&chars, index) else {
            index += 1;
            continue;
        };
        let ident_len = ident.chars().count();
        let previous = if index == 0 {
            None
        } else {
            chars.get(index - 1).copied()
        };
        let after = skip_space(&chars, index + ident_len);
        let next = chars.get(after).copied();
        if matches!(ident.as_str(), "b" | "r" | "br" | "c") && matches!(next, Some('"') | Some('#'))
        {
            index += ident_len;
            continue;
        }
        consider_capture(&ident, previous, next, &local, &mut found);
        index += ident_len;
    }
    found
}

fn consider_capture(
    ident: &str,
    previous: Option<char>,
    next: Option<char>,
    local: &[String],
    found: &mut Vec<String>,
) {
    let call = next == Some('(') || next == Some('!');
    let path = next == Some(':');
    let field = previous == Some('.');
    if !field
        && !call
        && !path
        && !KEYWORDS.contains(&ident)
        && !local.iter().any(|name| name == ident)
        && !found.iter().any(|name| name == ident)
    {
        found.push(ident.to_string());
    }
}

fn scan_string_captures(
    chars: &[char],
    open: usize,
    local: &[String],
    found: &mut Vec<String>,
) -> usize {
    let mut index = open + 1;
    while index < chars.len() {
        if chars[index] == '\\' {
            index += 2;
            continue;
        }
        if chars[index] == '"' {
            return index + 1;
        }
        if chars[index] == '{' && chars.get(index + 1) == Some(&'{') {
            index += 2;
            continue;
        }
        if chars[index] == '{' {
            if let Some(ident) = read_ident(chars, index + 1) {
                let after = index + 1 + ident.chars().count();
                if matches!(chars.get(after).copied(), Some('}') | Some(':')) {
                    consider_capture(&ident, Some('{'), None, local, found);
                    index = after;
                    continue;
                }
            }
        }
        index += 1;
    }
    chars.len()
}

fn end_of_char_literal(chars: &[char], open: usize) -> Option<usize> {
    if chars.get(open) != Some(&'\'') {
        return None;
    }
    let mut index = open + 1;
    if chars.get(index) == Some(&'\\') {
        index += 1;
        if chars.get(index) == Some(&'u') && chars.get(index + 1) == Some(&'{') {
            index += 2;
            while chars.get(index).is_some_and(|char| *char != '}') {
                index += 1;
            }
            index += 1;
        } else {
            index += 1;
        }
    } else if chars.get(index).is_some_and(|char| *char != '\'') {
        index += 1;
    } else {
        return None;
    }
    if chars.get(index) == Some(&'\'') {
        Some(index + 1)
    } else {
        None
    }
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
        let files = crate::rust_harness::generate::harness_sources(&closures, "");
        assert!(files.cargo_toml.contains("\n[workspace]\n"));
        assert!(files.handlers_rs.contains("require_f64(input, \"amount\")"));
        assert!(files.handlers_rs.contains("format!(\"{amount}\")"));
    }

    #[test]
    fn string_literals_are_not_captures() {
        assert!(captures_in(r#"String::from("hello")"#).is_empty());
        assert_eq!(
            captures_in(r#"format!("{amount}")"#),
            vec!["amount".to_string()]
        );
        assert!(captures_in("'h'").is_empty());
    }

    #[test]
    fn a_program_without_effects_builds_no_harness_source() {
        let source = "pub async fn main() -> Result<String, String> { Ok(String::from(\"ok\")) }\n";
        let closures = extract(source).expect("extract");
        assert!(closures.is_empty());
    }
}
