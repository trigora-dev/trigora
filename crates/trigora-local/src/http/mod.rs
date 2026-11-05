use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::Value as Json;

use crate::error::LocalError;
use crate::runtime::{FileProgram, LocalRuntime};

mod protocol;

use protocol::{decode_component, json_str, parse_body, read_request, write_json, Request};

pub struct Listeners {
    pub port: u16,
    pub control_port: u16,
    pub control_token: String,
    shutdown: Arc<AtomicBool>,
    joins: Vec<thread::JoinHandle<()>>,
}

impl Listeners {
    pub fn spawn(runtime: Arc<LocalRuntime>, host: &str, port: u16) -> Result<Self, LocalError> {
        let (public_listener, port) = bind_public(host, port)?;
        let control_listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|err| LocalError::message(err.to_string()))?;
        let control_port = control_listener
            .local_addr()
            .map_err(|err| LocalError::message(err.to_string()))?
            .port();
        let control_token = uuid::Uuid::new_v4().to_string();
        public_listener
            .set_nonblocking(true)
            .map_err(|err| LocalError::message(err.to_string()))?;
        control_listener
            .set_nonblocking(true)
            .map_err(|err| LocalError::message(err.to_string()))?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let public_shutdown = Arc::clone(&shutdown);
        let control_shutdown = Arc::clone(&shutdown);
        let public_runtime = Arc::clone(&runtime);
        let control_runtime = Arc::clone(&runtime);
        let token = control_token.clone();
        let joins = vec![
            thread::spawn(move || {
                accept_loop(
                    public_listener,
                    public_shutdown,
                    Arc::new(move |stream| handle_public(stream, &public_runtime)),
                );
            }),
            thread::spawn(move || {
                accept_loop(
                    control_listener,
                    control_shutdown,
                    Arc::new(move |stream| handle_control(stream, &control_runtime, &token)),
                );
            }),
        ];
        Ok(Self {
            port,
            control_port,
            control_token,
            shutdown,
            joins,
        })
    }

    pub fn block(&self) {
        while !self.shutdown.load(Ordering::SeqCst) {
            thread::park_timeout(Duration::from_secs(3600));
        }
    }
}

impl Drop for Listeners {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        for handle in self.joins.drain(..) {
            let _ = handle.join();
        }
    }
}

fn bind_public(host: &str, mut port: u16) -> Result<(TcpListener, u16), LocalError> {
    loop {
        match TcpListener::bind((host, port)) {
            Ok(listener) => {
                let actual = listener
                    .local_addr()
                    .map_err(|err| LocalError::message(err.to_string()))?
                    .port();
                return Ok((listener, actual));
            }
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse && port != 0 => {
                port = port.saturating_add(1);
            }
            Err(err) => return Err(LocalError::message(err.to_string())),
        }
    }
}

fn accept_loop(
    listener: TcpListener,
    shutdown: Arc<AtomicBool>,
    handle: Arc<dyn Fn(TcpStream) + Send + Sync>,
) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let handle = Arc::clone(&handle);
                thread::spawn(move || handle(stream));
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) if shutdown.load(Ordering::SeqCst) => break,
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn handle_public(mut stream: TcpStream, runtime: &LocalRuntime) {
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            let _ = write_json(&mut stream, error.status, &error.body());
            return;
        }
    };
    let result = dispatch_public(runtime, &request);
    match result {
        Ok((status, body)) => {
            let _ = write_json(&mut stream, status, &body);
        }
        Err(error) => {
            let _ = write_json(&mut stream, error.status, &error.body());
        }
    }
}

fn handle_control(mut stream: TcpStream, runtime: &LocalRuntime, token: &str) {
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            let _ = write_json(&mut stream, error.status, &error.body());
            return;
        }
    };
    let authorized = request
        .header("authorization")
        .is_some_and(|value| value == format!("Bearer {token}"));
    if !authorized {
        let _ = write_json(
            &mut stream,
            401,
            &serde_json::json!({
                "error": { "code": "unauthorized", "message": "Control token was rejected." }
            }),
        );
        return;
    }
    if request.method != "POST" || request.path != "/programs" {
        let _ = write_json(
            &mut stream,
            404,
            &serde_json::json!({"error": {"code": "not_found", "message": "Not found."}}),
        );
        return;
    }
    let body = match parse_body(&request.body) {
        Ok(body) => body,
        Err(error) => {
            let _ = write_json(&mut stream, error.status, &error.body());
            return;
        }
    };
    let programs = body
        .get("programs")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    let mut files = Vec::new();
    for program in programs {
        let id = json_str(&program, "id");
        if id.is_empty() {
            let error = LocalError::invalid("`id` is required.");
            let _ = write_json(&mut stream, error.status, &error.body());
            return;
        }
        files.push(FileProgram {
            id,
            artifact_json: json_str(&program, "artifactJson"),
            artifact_hash: json_str(&program, "artifactHash"),
            language: json_str(&program, "language"),
            frontend_id: json_str(&program, "frontendId"),
            frontend_version: json_str(&program, "frontendVersion"),
            language_semantics_version: json_str(&program, "languageSemanticsVersion"),
            engine_format_version: program
                .get("engineFormatVersion")
                .and_then(Json::as_i64)
                .unwrap_or(1),
        });
    }
    match runtime.replace_files(files) {
        Ok(()) => {
            let _ = write_json(&mut stream, 200, &serde_json::json!({"ok": true}));
        }
        Err(error) => {
            let _ = write_json(&mut stream, error.status, &error.body());
        }
    }
}

fn dispatch_public(runtime: &LocalRuntime, request: &Request) -> Result<(u16, Json), LocalError> {
    let method = request.method.as_str();
    let path = request.path.as_str();
    if method == "GET" && path == "/v1/health" {
        return Ok((200, serde_json::json!({"ok": true})));
    }
    if method == "GET" && path == "/v1/projects" {
        return Ok((200, runtime.list_projects()?));
    }
    if method == "POST" && path == "/v1/projects" {
        return Ok((201, runtime.create_project(&parse_body(&request.body)?)?));
    }
    if method == "POST" && path == "/v1/programs/deploy" {
        return Ok((201, runtime.deploy(&parse_body(&request.body)?)?));
    }
    if method == "GET" && path == "/v1/programs" {
        return Ok((200, runtime.list_programs()?));
    }
    if method == "POST" && path == "/v1/executions" {
        return Ok((201, runtime.start(&parse_body(&request.body)?)?));
    }
    if method == "GET" && path == "/v1/executions" {
        return Ok((200, runtime.list_executions()?));
    }
    if let Some(id) = path
        .strip_prefix("/v1/programs/")
        .and_then(|rest| rest.strip_suffix("/versions"))
    {
        if method == "GET" {
            return Ok((200, runtime.list_versions(&decode_component(id))?));
        }
    }
    if let Some(id) = path.strip_prefix("/v1/programs/") {
        if method == "GET" && !id.is_empty() && !id.contains('/') {
            return Ok((200, runtime.get_program(&decode_component(id))?));
        }
    }
    if let Some(id) = path
        .strip_prefix("/v1/executions/")
        .and_then(|rest| rest.strip_suffix("/result"))
    {
        if method == "GET" {
            return Ok((200, runtime.result(&decode_component(id))?));
        }
    }
    if let Some(id) = path
        .strip_prefix("/v1/executions/")
        .and_then(|rest| rest.strip_suffix("/events"))
    {
        if method == "POST" {
            return Ok((
                200,
                runtime.send(&decode_component(id), &parse_body(&request.body)?)?,
            ));
        }
    }
    if let Some(id) = path
        .strip_prefix("/v1/executions/")
        .and_then(|rest| rest.strip_suffix("/cancel"))
    {
        if method == "POST" {
            return Ok((200, runtime.cancel(&decode_component(id))?));
        }
    }
    if let Some(id) = path.strip_prefix("/v1/executions/") {
        if method == "GET" && !id.is_empty() && !id.contains('/') {
            return Ok((200, runtime.get_execution(&decode_component(id))?));
        }
    }
    Ok((
        404,
        serde_json::json!({"error": {"code": "not_found", "message": "Not found."}}),
    ))
}
