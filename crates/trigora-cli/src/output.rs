use crate::error::CliError;

pub fn render_error(error: &CliError) {
    eprintln!();
    eprintln!("✖ {}", error.title);
    if let Some(message) = &error.message {
        eprintln!();
        eprintln!("{message}");
    }
    if !error.details.is_empty() || error.hint.is_some() {
        eprintln!();
    }
    let width = error
        .details
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);
    for (label, value) in &error.details {
        eprintln!("  {}  {value}", format!("{label:<width$}"));
    }
    if let Some(hint) = &error.hint {
        eprintln!();
        eprintln!("  {hint}");
    }
}

pub fn programs_table(programs: &[(String, String, String)]) -> String {
    if programs.is_empty() {
        return "\nNo programs loaded. Is `trigora dev` running against this project?\n"
            .to_string();
    }
    rows(
        &programs
            .iter()
            .map(|(id, language, file)| vec![id.clone(), language.clone(), file.clone()])
            .collect::<Vec<_>>(),
    )
}

pub fn executions_table(rows_in: &[Vec<String>]) -> String {
    if rows_in.is_empty() {
        return "\nNo executions found.\n".to_string();
    }
    rows(rows_in)
}

fn rows(records: &[Vec<String>]) -> String {
    let width = records[0].len();
    let mut widths = vec![0; width];
    for record in records {
        for (index, cell) in record.iter().enumerate() {
            widths[index] = widths[index].max(cell.len());
        }
    }
    let mut out = String::from("\n");
    for record in records {
        let line = record
            .iter()
            .enumerate()
            .map(|(index, cell)| format!("{cell:<width$}", width = widths[index]))
            .collect::<Vec<_>>()
            .join("  ");
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

pub fn execution_record(fields: &[(String, String)]) -> String {
    let width = fields
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);
    let mut out = String::from("\n");
    for (label, value) in fields {
        out.push_str(&format!("  {}  {value}\n", format!("{label:<width$}")));
    }
    out
}

pub fn format_wait(wait: Option<&serde_json::Value>) -> String {
    let Some(wait) = wait else {
        return String::new();
    };
    match wait.get("type").and_then(|value| value.as_str()) {
        Some("event") => format!(
            "event:{}",
            wait.get("event")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        ),
        Some("timer") => format!(
            "timer:{}",
            wait.get("wakeAt")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        ),
        Some("child") => format!(
            "child:{}",
            wait.get("executionId")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        ),
        _ => String::new(),
    }
}
