use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::HARD_CONTINUATION_MAX;
use crate::error::{Error, Result};

const IDLE_LIMIT: usize = 8;
const IDLE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_HEADER: usize = 64 * 1024;
const MAX_BODY: usize = HARD_CONTINUATION_MAX + 1024 * 1024;

struct IdleConnection {
    stream: TcpStream,
    parked_at: Instant,
}

#[derive(Clone)]
pub(crate) struct Endpoint {
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) workspace: String,
    pub(super) token: String,
    pool: Arc<Mutex<Vec<IdleConnection>>>,
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

impl Endpoint {
    pub(super) fn open(host: String, port: u16, workspace: String, token: String) -> Self {
        Self {
            host,
            port,
            workspace,
            token,
            pool: Arc::new(Mutex::new(Vec::new())),
        }
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
            return Err(Error::new(
                "the Pulse authority client speaks HTTP/1.1; terminate TLS at the deployment edge",
            ));
        }
        let timeout = if body.is_some_and(|bytes| bytes.len() > 1024 * 1024) {
            Duration::from_secs(120)
        } else {
            Duration::from_secs(30)
        };
        loop {
            let (mut stream, from_pool) = match self.checkout() {
                Some(stream) => (stream, true),
                None => (connect(&self.host, self.port)?, false),
            };
            stream
                .set_read_timeout(Some(timeout))
                .map_err(|err| Error::new(err.to_string()))?;
            stream
                .set_write_timeout(Some(timeout))
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

    fn checkout(&self) -> Option<TcpStream> {
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

    fn release(&self, stream: TcpStream) {
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
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: Bearer {}\r\nX-Pulse-Owner: {owner}\r\nConnection: keep-alive\r\n",
            self.host, self.port, self.token
        );
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

fn connect(host: &str, port: u16) -> Result<TcpStream> {
    TcpStream::connect((host, port)).map_err(|err| Error::new(format!("authority connect: {err}")))
}

fn socket_usable(stream: &TcpStream) -> bool {
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
    stream: &mut TcpStream,
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

fn read_response(stream: &mut TcpStream) -> Result<Response> {
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

fn read_header(stream: &mut TcpStream) -> Result<Vec<u8>> {
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

fn read_exact_body(stream: &mut TcpStream, length: usize) -> Result<Vec<u8>> {
    if length > MAX_BODY {
        return Err(Error::new("authority response is too large"));
    }
    let mut body = vec![0u8; length];
    stream
        .read_exact(&mut body)
        .map_err(|err| Error::new(format!("authority read: {err}")))?;
    Ok(body)
}

fn read_chunked(stream: &mut TcpStream) -> Result<Vec<u8>> {
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

fn read_crlf_line(stream: &mut TcpStream) -> Result<Vec<u8>> {
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
