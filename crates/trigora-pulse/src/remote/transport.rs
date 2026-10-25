use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde::Deserialize;

use super::protocol::decode_chunks;
use crate::error::{Error, Result};

#[derive(Clone)]
pub(crate) struct Endpoint {
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) workspace: String,
    pub(super) token: String,
}

pub(super) struct RawHttp {
    pub(super) status: u16,
    pub(super) body: Vec<u8>,
}

impl Endpoint {
    pub(super) fn call_json<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        path: &str,
    ) -> Result<T> {
        let response = self.call(method, path, "", None, "application/json")?;
        serde_json::from_slice(&response.body).map_err(|err| Error::new(err.to_string()))
    }

    pub(super) fn post_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        owner: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let bytes = serde_json::to_vec(body).map_err(|err| Error::new(err.to_string()))?;
        let response = self.call("POST", path, owner, Some(&bytes), "application/json")?;
        serde_json::from_slice(&response.body).map_err(|err| Error::new(err.to_string()))
    }

    pub(super) fn post_bytes<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        owner: &str,
        body: &[u8],
    ) -> Result<T> {
        let response = self.call(
            "POST",
            path,
            owner,
            Some(body),
            "application/trigora-checkpoint",
        )?;
        serde_json::from_slice(&response.body).map_err(|err| Error::new(err.to_string()))
    }

    pub(super) fn call(
        &self,
        method: &str,
        path: &str,
        owner: &str,
        body: Option<&[u8]>,
        content_type: &str,
    ) -> Result<RawHttp> {
        let response = self.exchange(method, path, owner, body, content_type)?;
        if !(200..300).contains(&response.status) {
            let message = String::from_utf8_lossy(&response.body);
            let message = message.trim();
            if message.is_empty() {
                return Err(Error::new(format!("authority status {}", response.status)));
            }
            return Err(Error::new(message.to_string()));
        }
        Ok(response)
    }

    fn exchange(
        &self,
        method: &str,
        path: &str,
        owner: &str,
        body: Option<&[u8]>,
        content_type: &str,
    ) -> Result<RawHttp> {
        if self.port == 0 {
            return Err(Error::new(
                "the Pulse authority client speaks HTTP/1.1; terminate TLS at the deployment edge",
            ));
        }
        let mut stream = TcpStream::connect((self.host.as_str(), self.port))
            .map_err(|err| Error::new(format!("authority connect: {err}")))?;
        let timeout = if body.is_some_and(|bytes| bytes.len() > 1024 * 1024) {
            Duration::from_secs(120)
        } else {
            Duration::from_secs(30)
        };
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|err| Error::new(err.to_string()))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|err| Error::new(err.to_string()))?;
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: Bearer {}\r\nX-Pulse-Owner: {owner}\r\nConnection: close\r\n",
            self.host, self.port, self.token
        );
        if let Some(body) = body {
            request.push_str(&format!(
                "Content-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            ));
            stream
                .write_all(request.as_bytes())
                .and_then(|_| stream.write_all(body))
                .and_then(|_| stream.flush())
                .map_err(|err| Error::new(format!("authority write: {err}")))?;
        } else {
            request.push_str("\r\n");
            stream
                .write_all(request.as_bytes())
                .and_then(|_| stream.flush())
                .map_err(|err| Error::new(format!("authority write: {err}")))?;
        }
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .map_err(|err| Error::new(format!("authority read: {err}")))?;
        let (status, payload) = parse_http(&response)?;
        Ok(RawHttp {
            status,
            body: payload,
        })
    }
}

pub(super) fn split_base(base: &str) -> Result<(String, u16)> {
    let rest = base.strip_prefix("http://").ok_or_else(|| {
        Error::new(
            "the Pulse authority client speaks HTTP/1.1; terminate TLS at the deployment edge",
        )
    })?;
    let (host, port) = rest
        .split_once(':')
        .ok_or_else(|| Error::new("authority URL is missing a port"))?;
    if host.is_empty() || host.contains('/') {
        return Err(Error::new("authority URL is invalid"));
    }
    let port = port
        .trim_end_matches('/')
        .parse()
        .map_err(|_| Error::new("authority URL is invalid"))?;
    Ok((host.to_string(), port))
}

fn parse_http(bytes: &[u8]) -> Result<(u16, Vec<u8>)> {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| Error::new("authority response is truncated"))?;
    let header = std::str::from_utf8(&bytes[..split]).map_err(|err| Error::new(err.to_string()))?;
    let mut lines = header.lines();
    let status = lines
        .next()
        .ok_or_else(|| Error::new("authority response is truncated"))?;
    let code = status
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| Error::new("authority response is truncated"))?
        .parse()
        .map_err(|_| Error::new("authority response is truncated"))?;
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| Error::new("authority response is truncated"))?,
                );
            }
            if name.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
    }
    let body = &bytes[split + 4..];
    let body = if chunked {
        decode_chunks(body)?
    } else if let Some(length) = length {
        body.get(..length)
            .ok_or_else(|| Error::new("authority response is truncated"))?
            .to_vec()
    } else {
        body.to_vec()
    };
    Ok((code, body))
}
