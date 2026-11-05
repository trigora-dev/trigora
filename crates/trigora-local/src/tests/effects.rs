use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{json, Value as Json};
use tcc_ir::Artifact;

use crate::{EffectEndpoint, LocalRuntime};

use super::common::*;

#[test]
fn effect_callback_receives_program_and_execution_ids() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_thread = Arc::clone(&seen);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 2048];
        loop {
            let read = stream.read(&mut chunk).unwrap_or(0);
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&buffer);
                let length = header.lines().find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().to_string())
                });
                if let Some(length) = length {
                    let length: usize = length.parse().unwrap_or(0);
                    let body_at = buffer
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .unwrap()
                        + 4;
                    if buffer.len() >= body_at + length {
                        seen_thread.lock().unwrap().push(
                            String::from_utf8_lossy(&buffer[body_at..body_at + length]).to_string(),
                        );
                        break;
                    }
                }
            }
        }
        let body = br#"{"value":42}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    });

    let path = temp_db();
    let runtime = LocalRuntime::open(
        &path,
        Some(EffectEndpoint {
            url: format!("http://127.0.0.1:{port}/effects"),
            secret: "effect-secret".to_string(),
        }),
        quiet(),
    )
    .unwrap();
    runtime
        .replace_files(vec![file_program(
            "approval",
            &Artifact::sdk_first_example("approval-hash"),
        )])
        .unwrap();
    let started = runtime
        .start(&json!({"programId": "approval", "input": {}}))
        .unwrap();
    assert_eq!(started["execution"]["status"], "waiting");
    assert_eq!(
        started["execution"]["wait"],
        json!({"type": "event", "event": "approved"})
    );
    let body: Json = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(body["programId"], "approval");
    assert_eq!(body["executionId"], started["execution"]["id"]);
    assert_eq!(body["key"], "generate");

    let id = started["execution"]["id"].as_str().unwrap().to_string();
    let resumed = runtime
        .send(&id, &json!({"name": "approved", "payload": "ok"}))
        .unwrap();
    assert_eq!(resumed["execution"]["status"], "completed");
    assert_eq!(
        resumed["execution"]["result"],
        json!({"approval": "ok", "result": 42})
    );

    let reopened = LocalRuntime::open(&path, None, quiet()).unwrap();
    reopened
        .replace_files(vec![file_program(
            "approval",
            &Artifact::sdk_first_example("approval-hash"),
        )])
        .unwrap();
    assert_eq!(reopened.restored_waiting().unwrap(), 0);
    let listed = reopened.list_executions().unwrap();
    assert_eq!(listed["executions"][0]["id"], id);
    assert_eq!(listed["executions"][0]["status"], "completed");
}
