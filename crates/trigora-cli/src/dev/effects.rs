use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value as Json};

use crate::adapter::Registry;
use crate::error::CliError;
use crate::model::Program;

pub(super) fn serve_effects(
    programs: Arc<Mutex<Vec<Program>>>,
    secret: String,
    registry: Arc<Registry>,
    shutdown: Arc<AtomicBool>,
) -> Result<u16, CliError> {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).map_err(|error| CliError::plain(error.to_string()))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| CliError::plain(error.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|error| CliError::plain(error.to_string()))?
        .port();
    std::thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let programs = Arc::clone(&programs);
                    let secret = secret.clone();
                    let registry = Arc::clone(&registry);
                    std::thread::spawn(move || {
                        handle_effect(stream, &programs, &secret, &registry)
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    Ok(port)
}

fn handle_effect(
    mut stream: TcpStream,
    programs: &Mutex<Vec<Program>>,
    secret: &str,
    registry: &Registry,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let Ok((method, path, headers, body)) = read_request(&mut stream) else {
        return;
    };
    let authorized = headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("authorization") && value == &format!("Bearer {secret}")
    });
    if method != "POST" || path != "/effects" || !authorized {
        let _ = write_json(
            &mut stream,
            401,
            &json!({"error": "Effect secret was rejected."}),
        );
        return;
    }
    let parsed: Json = match serde_json::from_slice(&body) {
        Ok(parsed) => parsed,
        Err(_) => {
            let _ = write_json(
                &mut stream,
                400,
                &json!({"error": "Request body must be valid JSON."}),
            );
            return;
        }
    };
    let program_id = parsed.get("programId").and_then(Json::as_str).unwrap_or("");
    let key = parsed.get("key").and_then(Json::as_str).unwrap_or("");
    let input = parsed.get("input").cloned().unwrap_or(Json::Null);
    let found = {
        let programs = programs.lock().expect("programs");
        programs
            .iter()
            .find(|program| program.id == program_id)
            .and_then(|program| {
                program
                    .effects
                    .iter()
                    .find(|effect| effect.key == key)
                    .cloned()
                    .map(|effect| (program.language.clone(), effect))
            })
    };
    let Some((language, effect)) = found else {
        let _ = write_json(
            &mut stream,
            200,
            &json!({"error": format!("No effect handler for `{key}`.")}),
        );
        return;
    };
    let response = match registry.run_effect(&language, &effect, key, &input) {
        Ok(Some(value)) => json!({"value": value}),
        Ok(None) => json!({"error": format!("No effect handler for `{key}`.")}),
        Err(error) => json!({"error": error.to_string()}),
    };
    let _ = write_json(&mut stream, 200, &response);
}

fn read_request(
    stream: &mut TcpStream,
) -> std::io::Result<(String, String, Vec<(String, String)>, Vec<u8>)> {
    let mut reader = BufReader::new(stream);
    let mut start = String::new();
    reader.read_line(&mut start)?;
    let mut parts = start.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
            headers.push((name.to_string(), value.trim().to_string()));
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok((method, path, headers, body))
}

fn write_json(stream: &mut TcpStream, status: u16, body: &Json) -> std::io::Result<()> {
    let payload = body.to_string();
    let reason = if status == 200 {
        "OK"
    } else if status == 400 {
        "Bad Request"
    } else {
        "Unauthorized"
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
        payload.len()
    )?;
    stream.flush()
}
