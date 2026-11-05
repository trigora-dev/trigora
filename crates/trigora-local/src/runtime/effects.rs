use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use tcc_host::HostError;
use tcc_host_sqlite::SqliteHost;

use super::{EffectEndpoint, LocalRuntime};
use crate::value::{plain_to_value, value_to_plain};

impl LocalRuntime {
    pub(in crate::runtime) fn install_provider(
        &self,
        host: &mut SqliteHost,
        program_id: &str,
        execution_id: &str,
    ) {
        let Some(endpoint) = self.effect.clone() else {
            return;
        };
        let program_id = program_id.to_string();
        let execution_id = execution_id.to_string();
        let events = Arc::clone(&self.events);
        host.set_effect_provider(Box::new(move |key, input| {
            events(&json!({
                "type": "effect",
                "name": key,
                "execution": {
                    "id": execution_id,
                    "programId": program_id,
                    "status": "running",
                }
            }));
            call_effect(&endpoint, &program_id, &execution_id, key, input)
        }));
    }
}

fn call_effect(
    endpoint: &EffectEndpoint,
    program_id: &str,
    execution_id: &str,
    key: &str,
    input: &tcc_state::Value,
) -> Result<tcc_state::Value, HostError> {
    let body = json!({
        "programId": program_id,
        "executionId": execution_id,
        "key": key,
        "input": value_to_plain(input),
    })
    .to_string();
    let (host, port, path) = parse_http_url(&endpoint.url).map_err(HostError::Message)?;
    let mut stream = TcpStream::connect(format!("{host}:{port}"))
        .map_err(|err| HostError::Message(format!("effect callback failed: {err}")))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|err| HostError::Message(err.to_string()))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        endpoint.secret,
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| HostError::Message(err.to_string()))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|err| HostError::Message(err.to_string()))?;
    let payload = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");
    let parsed: Json = serde_json::from_str(payload.trim()).map_err(|err| {
        HostError::Message(format!("effect callback returned invalid JSON: {err}"))
    })?;
    if let Some(error) = parsed.get("error").and_then(Json::as_str) {
        return Err(HostError::Message(error.to_string()));
    }
    Ok(plain_to_value(parsed.get("value").unwrap_or(&Json::Null)))
}

fn parse_http_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("effect callback URL must be http: {url}"))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = format!("/{path}");
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| format!("effect callback URL is missing a port: {url}"))?;
    let port = port
        .parse()
        .map_err(|_| format!("effect callback port is invalid: {url}"))?;
    Ok((host.to_string(), port, path))
}
