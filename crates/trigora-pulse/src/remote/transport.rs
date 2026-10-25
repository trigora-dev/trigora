use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde::Deserialize;

use super::HARD_CONTINUATION_MAX;
use crate::error::{Error, Result};

const IDLE_LIMIT: usize = 8;
const IDLE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_HEADER: usize = 64 * 1024;
const MAX_BODY: usize = HARD_CONTINUATION_MAX + 1024 * 1024;

struct IdleConnection {
    stream: Conn,
    parked_at: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scheme {
    Http,
    Https,
}

#[derive(Clone)]
pub(crate) enum Route {
    Authority { workspace: String },
    Gateway,
}

pub(crate) struct Origin {
    pub(crate) scheme: Scheme,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) base_path: String,
}

#[derive(Clone)]
pub(crate) struct Endpoint {
    pub(super) host: String,
    pub(super) port: u16,
    scheme: Scheme,
    route: Route,
    base_path: String,
    pub(super) token: String,
    tls: Option<Arc<ClientConfig>>,
    pool: Arc<Mutex<Vec<IdleConnection>>>,
    opened: Arc<AtomicUsize>,
}

enum Conn {
    Plain(TcpStream),
    Tls(StreamOwned<ClientConnection, TcpStream>),
}

pub(super) struct RawHttp {
    pub(super) status: u16,
    pub(super) body: Vec<u8>,
}

struct Response {
    status: u16,
    body: Vec<u8>,
    reusable: bool,
}

impl Conn {
    fn tcp(&self) -> &TcpStream {
        match self {
            Conn::Plain(stream) => stream,
            Conn::Tls(stream) => &stream.sock,
        }
    }

    fn tcp_mut(&mut self) -> &mut TcpStream {
        match self {
            Conn::Plain(stream) => stream,
            Conn::Tls(stream) => &mut stream.sock,
        }
    }

    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.tcp_mut().set_read_timeout(Some(timeout))?;
        self.tcp_mut().set_write_timeout(Some(timeout))?;
        Ok(())
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(stream) => stream.read(buf),
            Conn::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(stream) => stream.write(buf),
            Conn::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Conn::Plain(stream) => stream.flush(),
            Conn::Tls(stream) => stream.flush(),
        }
    }
}

impl Endpoint {
    pub(super) fn open(host: String, port: u16, workspace: String, token: String) -> Self {
        Self::from_parts(
            Origin {
                scheme: Scheme::Http,
                host,
                port,
                base_path: String::new(),
            },
            Route::Authority { workspace },
            token,
        )
    }

    pub(super) fn from_parts(origin: Origin, route: Route, token: String) -> Self {
        let tls = match origin.scheme {
            Scheme::Http => None,
            Scheme::Https => Some(tls_config()),
        };
        Self {
            host: origin.host,
            port: origin.port,
            scheme: origin.scheme,
            route,
            base_path: origin.base_path,
            token,
            tls,
            pool: Arc::new(Mutex::new(Vec::new())),
            opened: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(super) fn execution_path(&self, execution_id: &str, action: &str) -> String {
        let rest = match &self.route {
            Route::Authority { workspace } => {
                format!("v1/workspaces/{workspace}/executions/{execution_id}/{action}")
            }
            Route::Gateway => format!("v1/pulse/executions/{execution_id}/{action}"),
        };
        join_path(&self.base_path, &rest)
    }

    pub(super) fn connections_opened(&self) -> usize {
        self.opened.load(Ordering::SeqCst)
    }

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
            return Err(Error::new("authority URL is invalid"));
        }
        let large = body.is_some_and(|bytes| bytes.len() > 1024 * 1024)
            || path.contains("sql-commits")
            || path.contains("sql-restore")
            || path.contains("sql-claim");
        let timeout = if large {
            Duration::from_secs(120)
        } else {
            Duration::from_secs(30)
        };
        loop {
            let (mut stream, from_pool) = match self.checkout() {
                Some(stream) => (stream, true),
                None => (self.connect()?, false),
            };
            stream
                .set_timeout(timeout)
                .map_err(|err| Error::new(err.to_string()))?;
            let request = self.request_bytes(method, path, owner, body, content_type);
            match write_counted(&mut stream, &request) {
                Ok(()) => {}
                Err((written, _)) if from_pool && written == 0 => continue,
                Err((_, err)) => return Err(Error::new(format!("authority write: {err}"))),
            }
            let response = read_response(&mut stream)?;
            if response.reusable {
                self.release(stream);
            }
            return Ok(RawHttp {
                status: response.status,
                body: response.body,
            });
        }
    }

    fn connect(&self) -> Result<Conn> {
        let tcp = TcpStream::connect((self.host.as_str(), self.port))
            .map_err(|err| Error::new(format!("authority connect: {err}")))?;
        self.opened.fetch_add(1, Ordering::SeqCst);
        match &self.tls {
            None => Ok(Conn::Plain(tcp)),
            Some(config) => {
                let name = ServerName::try_from(self.host.clone())
                    .map_err(|_| Error::new("authority hostname is invalid"))?;
                let session = ClientConnection::new(Arc::clone(config), name)
                    .map_err(|err| Error::new(format!("tls: {err}")))?;
                Ok(Conn::Tls(StreamOwned::new(session, tcp)))
            }
        }
    }

    fn checkout(&self) -> Option<Conn> {
        loop {
            let candidate = {
                let mut pool = self.pool.lock().expect("pool");
                pool.retain(|idle| idle.parked_at.elapsed() <= IDLE_TIMEOUT);
                pool.pop()
            };
            let idle = candidate?;
            if socket_usable(&idle.stream) {
                return Some(idle.stream);
            }
        }
    }

    fn release(&self, stream: Conn) {
        let mut pool = self.pool.lock().expect("pool");
        if pool.len() >= IDLE_LIMIT {
            return;
        }
        pool.push(IdleConnection {
            stream,
            parked_at: Instant::now(),
        });
    }

    fn request_bytes(
        &self,
        method: &str,
        path: &str,
        owner: &str,
        body: Option<&[u8]>,
        content_type: &str,
    ) -> Vec<u8> {
        let host = if (self.scheme == Scheme::Https && self.port == 443)
            || (self.scheme == Scheme::Http && self.port == 80)
        {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        };
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\nConnection: keep-alive\r\n",
            self.token
        );
        if !owner.is_empty() {
            request.push_str(&format!("X-Pulse-Owner: {owner}\r\n"));
        }
        if let Some(body) = body {
            request.push_str(&format!(
                "Content-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            ));
            let mut bytes = request.into_bytes();
            bytes.extend_from_slice(body);
            bytes
        } else {
            request.push_str("\r\n");
            request.into_bytes()
        }
    }
}

fn socket_usable(stream: &Conn) -> bool {
    let stream = stream.tcp();
    if stream.set_nonblocking(true).is_err() {
        return false;
    }
    let mut buf = [0u8; 1];
    let open = matches!(
        stream.peek(&mut buf),
        Err(err) if err.kind() == io::ErrorKind::WouldBlock
    );
    let _ = stream.set_nonblocking(false);
    open
}

/// A write error reports how many bytes of this buffer were accepted.
/// Zero means the request was not transmitted.
fn write_counted(
    stream: &mut impl Write,
    bytes: &[u8],
) -> std::result::Result<(), (usize, io::Error)> {
    let mut sent = 0;
    while sent < bytes.len() {
        match stream.write(&bytes[sent..]) {
            Ok(0) => {
                return Err((
                    sent,
                    io::Error::new(io::ErrorKind::WriteZero, "authority closed the connection"),
                ));
            }
            Ok(count) => sent += count,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err((sent, err)),
        }
    }
    if let Err(err) = stream.flush() {
        return Err((sent, err));
    }
    Ok(())
}

fn read_response(stream: &mut impl Read) -> Result<Response> {
    let header = read_header(stream)?;
    let text = std::str::from_utf8(&header).map_err(|err| Error::new(err.to_string()))?;
    let mut lines = text.split("\r\n");
    let status = lines
        .next()
        .ok_or_else(|| Error::new("authority response is truncated"))?;
    let mut parts = status.split_whitespace();
    let version = parts
        .next()
        .ok_or_else(|| Error::new("authority response is truncated"))?;
    let code = parts
        .next()
        .ok_or_else(|| Error::new("authority response is truncated"))?
        .parse::<u16>()
        .map_err(|_| Error::new("authority response is truncated"))?;
    let mut length = None;
    let mut chunked = false;
    let mut close = !version.eq_ignore_ascii_case("HTTP/1.1");
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| Error::new("authority response is truncated"))?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
        {
            chunked = true;
        } else if name.eq_ignore_ascii_case("connection")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("close"))
        {
            close = true;
        }
    }
    let (body, delimited) = if chunked {
        (read_chunked(stream)?, true)
    } else if let Some(length) = length {
        (read_exact_body(stream, length)?, true)
    } else {
        let mut body = Vec::new();
        stream
            .read_to_end(&mut body)
            .map_err(|err| Error::new(format!("authority read: {err}")))?;
        if body.len() > MAX_BODY {
            return Err(Error::new("authority response is too large"));
        }
        (body, false)
    };
    Ok(Response {
        status: code,
        body,
        reusable: delimited && !close,
    })
}

fn read_header(stream: &mut impl Read) -> Result<Vec<u8>> {
    let mut header = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if header.len() >= MAX_HEADER {
            return Err(Error::new("authority response is too large"));
        }
        stream
            .read_exact(&mut byte)
            .map_err(|err| Error::new(format!("authority read: {err}")))?;
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            return Ok(header);
        }
    }
}

fn read_exact_body(stream: &mut impl Read, length: usize) -> Result<Vec<u8>> {
    if length > MAX_BODY {
        return Err(Error::new("authority response is too large"));
    }
    let mut body = vec![0u8; length];
    stream
        .read_exact(&mut body)
        .map_err(|err| Error::new(format!("authority read: {err}")))?;
    Ok(body)
}

fn read_chunked(stream: &mut impl Read) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line = read_crlf_line(stream)?;
        let size = std::str::from_utf8(&line)
            .map_err(|err| Error::new(err.to_string()))?
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let size = usize::from_str_radix(size, 16)
            .map_err(|_| Error::new("authority response is truncated"))?;
        if size == 0 {
            loop {
                if read_crlf_line(stream)?.is_empty() {
                    break;
                }
            }
            return Ok(body);
        }
        if body.len().saturating_add(size) > MAX_BODY {
            return Err(Error::new("authority response is too large"));
        }
        let mut chunk = vec![0u8; size];
        stream
            .read_exact(&mut chunk)
            .map_err(|err| Error::new(format!("authority read: {err}")))?;
        let mut crlf = [0u8; 2];
        stream
            .read_exact(&mut crlf)
            .map_err(|err| Error::new(format!("authority read: {err}")))?;
        if crlf != *b"\r\n" {
            return Err(Error::new("authority response is truncated"));
        }
        body.extend_from_slice(&chunk);
    }
}

fn read_crlf_line(stream: &mut impl Read) -> Result<Vec<u8>> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if line.len() >= MAX_HEADER {
            return Err(Error::new("authority response is too large"));
        }
        stream
            .read_exact(&mut byte)
            .map_err(|err| Error::new(format!("authority read: {err}")))?;
        if byte[0] == b'\n' && line.last() == Some(&b'\r') {
            line.pop();
            return Ok(line);
        }
        line.push(byte[0]);
    }
}

pub(super) fn parse_origin(base: &str) -> Result<Origin> {
    let (scheme, rest) = if let Some(rest) = base.strip_prefix("https://") {
        (Scheme::Https, rest)
    } else if let Some(rest) = base.strip_prefix("http://") {
        (Scheme::Http, rest)
    } else {
        return Err(Error::new("authority URL is invalid"));
    };
    if rest.is_empty() {
        return Err(Error::new("authority URL is invalid"));
    }
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(index) if rest.as_bytes()[index] != b'/' => {
            return Err(Error::new("authority URL is invalid"));
        }
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if path.contains(['?', '#']) {
        return Err(Error::new("authority URL is invalid"));
    }
    let (host, port) = split_authority(authority, scheme)?;
    Ok(Origin {
        scheme,
        host,
        port,
        base_path: path.trim_end_matches('/').to_string(),
    })
}

fn split_authority(authority: &str, scheme: Scheme) -> Result<(String, u16)> {
    let default = match scheme {
        Scheme::Https => 443,
        Scheme::Http => 80,
    };
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest
            .split_once(']')
            .ok_or_else(|| Error::new("authority URL is invalid"))?;
        if host.is_empty() {
            return Err(Error::new("authority URL is invalid"));
        }
        let port = match port.strip_prefix(':') {
            Some(port) if !port.is_empty() => port
                .parse()
                .map_err(|_| Error::new("authority URL is invalid"))?,
            Some(_) | None if port.is_empty() => default,
            _ => return Err(Error::new("authority URL is invalid")),
        };
        return Ok((format!("[{host}]"), port));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() || port.is_empty() || port.contains(':') {
            return Err(Error::new("authority URL is invalid"));
        }
        let port = port
            .parse()
            .map_err(|_| Error::new("authority URL is invalid"))?;
        return Ok((host.to_string(), port));
    }
    if authority.is_empty() {
        return Err(Error::new("authority URL is invalid"));
    }
    Ok((authority.to_string(), default))
}

fn join_path(base: &str, rest: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        format!("/{rest}")
    } else {
        format!("{base}/{rest}")
    }
}

fn tls_config() -> Arc<ClientConfig> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    Arc::clone(CONFIG.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Arc::new(config)
    }))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    fn endpoint(port: u16) -> Endpoint {
        Endpoint::open("127.0.0.1".into(), port, "ws".into(), "token".into())
    }

    fn read_request(stream: &mut TcpStream) -> io::Result<String> {
        let mut header = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte)?;
            header.push(byte[0]);
            if header.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let text = String::from_utf8_lossy(&header).into_owned();
        if let Some(length) = text.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        }) {
            let mut body = vec![0u8; length];
            stream.read_exact(&mut body)?;
        }
        Ok(text)
    }

    fn keep_alive(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn path_of(request: &str) -> String {
        request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn an_empty_owner_is_not_sent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let first = read_request(&mut stream).unwrap();
            stream.write_all(&keep_alive("ok")).unwrap();
            let second = read_request(&mut stream).unwrap();
            stream.write_all(&keep_alive("ok")).unwrap();
            (first, second)
        });
        let client = endpoint(port);
        client
            .call("GET", "/version", "", None, "text/plain")
            .unwrap();
        client
            .call("GET", "/claim", "owner-a", None, "text/plain")
            .unwrap();
        let (first, second) = server.join().unwrap();
        assert!(!first.to_ascii_lowercase().contains("x-pulse-owner"));
        assert!(second.contains("X-Pulse-Owner: owner-a"));
    }

    #[test]
    fn a_gateway_base_path_is_preserved() {
        let origin = parse_origin("https://api.trigora.dev/edge/").unwrap();
        assert_eq!(origin.port, 443);
        assert_eq!(origin.base_path, "/edge");
        let endpoint = Endpoint::from_parts(origin, Route::Gateway, "token".into());
        assert_eq!(
            endpoint.execution_path("exec:1", "sql-restore"),
            "/edge/v1/pulse/executions/exec:1/sql-restore"
        );
        let local = parse_origin("http://127.0.0.1:8787").unwrap();
        assert_eq!(local.port, 8787);
        assert_eq!(parse_origin("http://authority.internal").unwrap().port, 80);
        assert!(parse_origin("https://api.trigora.dev?x=1").is_err());
    }

    #[test]
    fn two_requests_reuse_one_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepts = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&accepts);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            count.fetch_add(1, Ordering::SeqCst);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            read_request(&mut stream).unwrap();
            stream.write_all(&keep_alive("one")).unwrap();
            read_request(&mut stream).unwrap();
            stream.write_all(&keep_alive("two")).unwrap();
        });
        let client = endpoint(port);
        let first = client.call("GET", "/one", "", None, "text/plain").unwrap();
        let second = client.call("GET", "/two", "", None, "text/plain").unwrap();
        assert_eq!(first.body, b"one");
        assert_eq!(second.body, b"two");
        server.join().unwrap();
        assert_eq!(accepts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_closed_idle_connection_is_replaced() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepts = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&accepts);
        let (closed_tx, closed_rx) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            count.fetch_add(1, Ordering::SeqCst);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            read_request(&mut stream).unwrap();
            stream.write_all(&keep_alive("up")).unwrap();
            stream.flush().unwrap();
            stream.shutdown(std::net::Shutdown::Both).unwrap();
            drop(stream);
            closed_tx.send(()).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            count.fetch_add(1, Ordering::SeqCst);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            read_request(&mut stream).unwrap();
            stream.write_all(&keep_alive("up")).unwrap();
        });
        let client = endpoint(port);
        assert_eq!(
            client
                .call("GET", "/a", "", None, "text/plain")
                .unwrap()
                .body,
            b"up"
        );
        closed_rx.recv().unwrap();
        assert_eq!(
            client
                .call("GET", "/b", "", None, "text/plain")
                .unwrap()
                .body,
            b"up"
        );
        server.join().unwrap();
        assert_eq!(accepts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn concurrent_requests_do_not_share_a_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepts = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&accepts);
        let gate = Arc::new((Mutex::new(0usize), std::sync::Condvar::new()));
        let server_gate = Arc::clone(&gate);
        let server = thread::spawn(move || {
            let mut handlers = Vec::new();
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                count.fetch_add(1, Ordering::SeqCst);
                let gate = Arc::clone(&server_gate);
                handlers.push(thread::spawn(move || {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let request = read_request(&mut stream).unwrap();
                    let path = path_of(&request);
                    let (lock, cv) = &*gate;
                    let mut arrived = lock.lock().unwrap();
                    *arrived += 1;
                    if *arrived == 2 {
                        cv.notify_all();
                    }
                    while *arrived < 2 {
                        arrived = cv.wait(arrived).unwrap();
                    }
                    drop(arrived);
                    stream.write_all(&keep_alive(&path)).unwrap();
                }));
            }
            for handler in handlers {
                handler.join().unwrap();
            }
        });
        let client = endpoint(port);
        let left = client.clone();
        let right = client.clone();
        let alpha = thread::spawn(move || left.call("GET", "/alpha", "", None, "text/plain"));
        let beta = thread::spawn(move || right.call("GET", "/beta", "", None, "text/plain"));
        let alpha = alpha.join().unwrap().unwrap();
        let beta = beta.join().unwrap().unwrap();
        server.join().unwrap();
        assert_eq!(accepts.load(Ordering::SeqCst), 2);
        let mut bodies = [alpha.body, beta.body];
        bodies.sort();
        assert_eq!(bodies, [b"/alpha".to_vec(), b"/beta".to_vec()]);
    }

    #[test]
    fn a_chunked_response_is_consumed_and_reused() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepts = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&accepts);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            count.fetch_add(1, Ordering::SeqCst);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            read_request(&mut stream).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ntransfer-encoding: Chunked\r\nConnection: keep-alive\r\n\r\n5\r\nhello\r\n0\r\nX-Trail: yes\r\n\r\n",
                )
                .unwrap();
            stream.flush().unwrap();
            read_request(&mut stream).unwrap();
            stream.write_all(&keep_alive("again")).unwrap();
        });
        let client = endpoint(port);
        let first = client
            .call("GET", "/chunk", "", None, "text/plain")
            .unwrap();
        let second = client.call("GET", "/next", "", None, "text/plain").unwrap();
        assert_eq!(first.body, b"hello");
        assert_eq!(second.body, b"again");
        server.join().unwrap();
        assert_eq!(accepts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_transmitted_request_is_not_retried() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepts = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&accepts);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            count.fetch_add(1, Ordering::SeqCst);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            read_request(&mut stream).unwrap();
            stream.write_all(&keep_alive("ok")).unwrap();
            stream.flush().unwrap();
            read_request(&mut stream).unwrap();
            drop(stream);
            listener.set_nonblocking(true).unwrap();
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(400) {
                if listener.accept().is_ok() {
                    count.fetch_add(1, Ordering::SeqCst);
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        let client = endpoint(port);
        client
            .call("GET", "/first", "", None, "text/plain")
            .unwrap();
        let err = client.call(
            "POST",
            "/commit",
            "owner",
            Some(b"checkpoint"),
            "text/plain",
        );
        assert!(err.is_err());
        server.join().unwrap();
        assert_eq!(accepts.load(Ordering::SeqCst), 1);
    }
}
