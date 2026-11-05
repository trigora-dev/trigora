use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use crate::Listeners;

use super::common::*;

#[test]
fn control_requests_without_a_token_are_rejected() {
    let path = temp_db();
    let runtime = Arc::new(runtime(&path));
    let listeners = Listeners::spawn(Arc::clone(&runtime), "127.0.0.1", 0).unwrap();
    assert_ne!(listeners.port, listeners.control_port);

    let rejected = http(
        listeners.control_port,
        "POST /programs HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
    );
    assert!(rejected.starts_with("HTTP/1.1 401"), "{rejected}");

    let accepted = http(
        listeners.control_port,
        &format!(
            "POST /programs HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{{\"programs\":[]}}",
            listeners.control_token
        ),
    );
    assert!(accepted.starts_with("HTTP/1.1 200"), "{accepted}");

    let health = http(
        listeners.port,
        "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(health.contains("\"ok\":true"), "{health}");
    drop(listeners);
}

fn http(port: u16, request: &str) -> String {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}
