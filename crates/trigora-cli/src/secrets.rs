use std::io::{IsTerminal, Read, Write};

use serde_json::{json, Value as Json};

use crate::commands::remote_endpoint;
use crate::error::CliError;
use crate::http::{self, cloud_failure};

#[derive(Debug, PartialEq, Eq)]
pub enum SecretsAction {
    List,
    Set { name: String },
    Delete { name: String },
}

pub fn secrets(action: SecretsAction) -> Result<(), CliError> {
    let endpoint = remote_endpoint()?;
    let project_id = endpoint
        .project_id
        .clone()
        .ok_or_else(|| CliError::new("Project not found"))?;
    match action {
        SecretsAction::List => list(&endpoint, &project_id),
        SecretsAction::Set { name } => set(&endpoint, &project_id, &name),
        SecretsAction::Delete { name } => delete(&endpoint, &project_id, &name),
    }
}

fn list(endpoint: &crate::http::Endpoint, project_id: &str) -> Result<(), CliError> {
    let listed = http::request(
        endpoint,
        "GET",
        &format!("/v1/projects/{project_id}/secrets"),
        None,
    )
    .map_err(|error| cloud_failure(error, "Listing secrets"))?;
    let secrets = listed
        .get("secrets")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    if secrets.is_empty() {
        println!("\nNo secrets.\n");
        return Ok(());
    }
    println!();
    for secret in secrets {
        let name = secret.get("name").and_then(Json::as_str).unwrap_or("");
        let updated = secret.get("updatedAt").and_then(Json::as_str).unwrap_or("");
        println!("{name}  {updated}");
    }
    println!();
    Ok(())
}

fn set(endpoint: &crate::http::Endpoint, project_id: &str, name: &str) -> Result<(), CliError> {
    reject_inline_value(name)?;
    let value = read_secret_value()?;
    let _response = http::request(
        endpoint,
        "PUT",
        &format!("/v1/projects/{project_id}/secrets/{name}"),
        Some(&json!({ "value": value })),
    )
    .map_err(|error| cloud_failure(error, "Setting secret"))?;
    println!("Set {name}");
    Ok(())
}

fn delete(endpoint: &crate::http::Endpoint, project_id: &str, name: &str) -> Result<(), CliError> {
    reject_inline_value(name)?;
    let _response = http::request(
        endpoint,
        "DELETE",
        &format!("/v1/projects/{project_id}/secrets/{name}"),
        None,
    )
    .map_err(|error| cloud_failure(error, "Deleting secret"))?;
    println!("Deleted {name}");
    Ok(())
}

fn reject_inline_value(name: &str) -> Result<(), CliError> {
    if name.contains('=') {
        return Err(CliError::plain(
            "Pass the secret name only. The value is read from a prompt or stdin.",
        ));
    }
    Ok(())
}

fn read_secret_value() -> Result<String, CliError> {
    let mut stdin = std::io::stdin();
    let raw = if stdin.is_terminal() {
        read_hidden_line()?
    } else {
        let mut raw = String::new();
        stdin
            .read_to_string(&mut raw)
            .map_err(|error| CliError::plain(error.to_string()))?;
        raw
    };
    let value = strip_trailing_newline(raw);
    if value.is_empty() {
        return Err(CliError::plain("Secret value is empty."));
    }
    Ok(value)
}

fn strip_trailing_newline(mut value: String) -> String {
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    value
}

fn read_hidden_line() -> Result<String, CliError> {
    eprint!("Value: ");
    let _ = std::io::stderr().flush();
    let value = read_hidden_unix()?;
    eprintln!();
    Ok(value)
}

#[cfg(unix)]
fn read_hidden_unix() -> Result<String, CliError> {
    let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
    let fd = libc::STDIN_FILENO;
    if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
        return Err(CliError::plain(
            "Could not hide secret input. Pipe the value on stdin.",
        ));
    }
    let mut hidden = original;
    hidden.c_lflag &= !libc::ECHO;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &hidden) } != 0 {
        return Err(CliError::plain(
            "Could not hide secret input. Pipe the value on stdin.",
        ));
    }
    let _restore = EchoRestore { fd, original };
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|error| CliError::plain(error.to_string()))?;
    Ok(strip_trailing_newline(line))
}

#[cfg(not(unix))]
fn read_hidden_unix() -> Result<String, CliError> {
    Err(CliError::plain(
        "Could not hide secret input. Pipe the value on stdin.",
    ))
}

#[cfg(unix)]
struct EchoRestore {
    fd: i32,
    original: libc::termios,
}

#[cfg(unix)]
impl Drop for EchoRestore {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}
