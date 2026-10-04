/// Bundled frontend versions. Adapters report what they loaded; the CLI compares.
pub const TYPESCRIPT_FRONTEND: &str = "26.10.1";
pub const PYTHON_FRONTEND: &str = "26.10.1";
pub const RUST_FRONTEND: &str = "26.10.1";

pub fn expected(adapter_id: &str) -> Option<&'static str> {
    match adapter_id {
        "python" => Some(PYTHON_FRONTEND),
        "rust" => Some(RUST_FRONTEND),
        "typescript" => Some(TYPESCRIPT_FRONTEND),
        _ => None,
    }
}

pub fn mismatch(adapter_id: &str, found: &str, file: &str) -> crate::error::CliError {
    let expected = expected(adapter_id).unwrap_or(TYPESCRIPT_FRONTEND);
    match adapter_id {
        "python" => crate::error::CliError::new("Python compiler version mismatch")
            .message(format!(
                "This CLI requires Python compiler {expected}, installed with the trigora authoring package."
            ))
            .detail("File", file)
            .detail("Found", found)
            .detail("Install", "python3 -m pip install trigora"),
        "rust" => crate::error::CliError::new("Rust compiler version mismatch")
            .message(format!("This CLI requires Rust compiler {expected}."))
            .detail("File", file)
            .detail("Found", found),
        _ => crate::error::CliError::new("TypeScript compiler version mismatch")
            .message(format!("This CLI requires TypeScript compiler {expected}."))
            .detail("Found", found),
    }
}
