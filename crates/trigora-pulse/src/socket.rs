use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Frame {
    #[serde(rename = "auth")]
    Auth { bearer: String },
    #[serde(rename = "authOk")]
    AuthOk,
    #[serde(rename = "authError")]
    AuthError { message: String },
    #[serde(rename = "register")]
    Register {
        #[serde(rename = "artifactHash")]
        artifact_hash: String,
        keys: Vec<String>,
    },
    #[serde(rename = "registered")]
    Registered,
    #[serde(rename = "start")]
    Start {
        #[serde(rename = "executionId")]
        execution_id: String,
    },
    #[serde(rename = "started")]
    Started {
        #[serde(rename = "executionId")]
        execution_id: String,
    },
    #[serde(rename = "startError")]
    StartError {
        #[serde(rename = "executionId")]
        execution_id: String,
        message: String,
    },
    #[serde(rename = "effect")]
    Effect {
        #[serde(rename = "executionId")]
        execution_id: String,
        #[serde(rename = "attemptId")]
        attempt_id: String,
        key: String,
        input: String,
    },
    #[serde(rename = "effectResult")]
    EffectResult {
        #[serde(rename = "executionId")]
        execution_id: String,
        #[serde(rename = "attemptId")]
        attempt_id: String,
        value: String,
    },
    #[serde(rename = "effectError")]
    EffectError {
        #[serde(rename = "executionId")]
        execution_id: String,
        #[serde(rename = "attemptId")]
        attempt_id: String,
        message: String,
    },
}

pub(crate) fn uid_allowed(peer: u32, owner: u32) -> bool {
    peer == owner
}

pub struct Listener {
    dir: PathBuf,
    bearer: String,
    listener: UnixListener,
}

impl Listener {
    pub fn bind(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)
            .map_err(|err| Error::new(format!("create {}: {err}", dir.display())))?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|err| Error::new(format!("directory mode: {err}")))?;
        let socket_path = dir.join("pulse.sock");
        prepare_socket_path(&socket_path)?;
        let listener = UnixListener::bind(&socket_path)
            .map_err(|err| Error::new(format!("bind {}: {err}", socket_path.display())))?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
            .map_err(|err| Error::new(format!("socket mode: {err}")))?;
        let bearer = uuid::Uuid::new_v4().to_string();
        let bearer_path = dir.join("bearer");
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&bearer_path)
            .map_err(|err| Error::new(format!("bearer file: {err}")))?;
        file.write_all(bearer.as_bytes())
            .map_err(|err| Error::new(format!("write bearer: {err}")))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            bearer,
            listener,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn bearer(&self) -> &str {
        &self.bearer
    }

    pub fn socket_path(&self) -> PathBuf {
        self.dir.join("pulse.sock")
    }

    pub(crate) fn accept(&self) -> io::Result<(UnixStream, std::os::unix::net::SocketAddr)> {
        self.listener.accept()
    }
}

fn prepare_socket_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(Error::new(
            "refusing to replace a symlink at the socket path",
        )),
        Ok(meta) if meta.file_type().is_socket() => {
            fs::remove_file(path).map_err(|err| Error::new(format!("remove stale socket: {err}")))
        }
        Ok(_) => Err(Error::new(
            "refusing to replace a non-socket at the socket path",
        )),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::new(format!("socket path: {err}"))),
    }
}

pub(crate) fn peer_allowed(stream: &UnixStream) -> bool {
    match peer_uid(stream) {
        Ok(uid) => uid_allowed(uid, current_uid()),
        Err(_) => false,
    }
}

fn current_uid() -> u32 {
    unsafe { libc::geteuid() }
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let fd = stream.as_raw_fd();
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd"
    ))]
    {
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(uid)
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut libc::ucred as *mut libc::c_void,
                &mut len,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(cred.uid)
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "linux"
    )))]
    {
        let _ = fd;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "peer credentials are unavailable",
        ))
    }
}

pub(crate) fn read_frame(stream: &mut impl Read) -> io::Result<Frame> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub(crate) fn write_frame(stream: &mut impl Write, frame: &Frame) -> io::Result<()> {
    write_frame_timed(stream, frame).map(|_| ())
}

/// Serialize first, then time only the socket write of the finished frame.
fn write_frame_timed(stream: &mut impl Write, frame: &Frame) -> io::Result<Duration> {
    let buf =
        serde_json::to_vec(frame).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let len = u32::try_from(buf.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame too large"))?;
    let started = Instant::now();
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(&buf)?;
    stream.flush()?;
    Ok(started.elapsed())
}

fn wait_readable(stream: &UnixStream) -> io::Result<()> {
    let fd = stream.as_raw_fd();
    loop {
        let mut pollfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let rc = unsafe { libc::poll(&mut pollfd, 1, -1) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        return Ok(());
    }
}

enum Inbound {
    Value(String),
    Failed(String),
    Gone,
}

pub(crate) enum HostCommand {
    Start(String),
}

pub(crate) struct Connection {
    pub artifact_hash: String,
    pub keys: Vec<String>,
    writer: Mutex<UnixStream>,
    pending: Mutex<HashMap<(String, String), std::sync::mpsc::Sender<Inbound>>>,
    reads: Mutex<HashMap<(String, String), Duration>>,
    commands: Mutex<Option<std::sync::mpsc::Sender<HostCommand>>>,
    alive: AtomicBool,
}

impl Connection {
    pub(crate) fn handshake(
        mut stream: UnixStream,
        bearer: &str,
    ) -> Result<(Arc<Self>, UnixStream)> {
        if !peer_allowed(&stream) {
            return Err(Error::new("socket peer was rejected"));
        }
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|err| Error::new(err.to_string()))?;
        let frame =
            read_frame(&mut stream).map_err(|err| Error::new(format!("auth frame: {err}")))?;
        let Frame::Auth { bearer: presented } = frame else {
            let _ = write_frame(
                &mut stream,
                &Frame::AuthError {
                    message: "authentication is required".to_string(),
                },
            );
            return Err(Error::new("authentication is required"));
        };
        if presented != bearer {
            let _ = write_frame(
                &mut stream,
                &Frame::AuthError {
                    message: "bearer was rejected".to_string(),
                },
            );
            return Err(Error::new("bearer was rejected"));
        }
        write_frame(&mut stream, &Frame::AuthOk).map_err(|err| Error::new(err.to_string()))?;
        let frame =
            read_frame(&mut stream).map_err(|err| Error::new(format!("register frame: {err}")))?;
        let Frame::Register {
            artifact_hash,
            keys,
        } = frame
        else {
            return Err(Error::new("registration is required after authentication"));
        };
        write_frame(&mut stream, &Frame::Registered).map_err(|err| Error::new(err.to_string()))?;
        stream
            .set_read_timeout(None)
            .map_err(|err| Error::new(err.to_string()))?;
        let reader = stream
            .try_clone()
            .map_err(|err| Error::new(err.to_string()))?;
        let connection = Arc::new(Self {
            artifact_hash,
            keys,
            writer: Mutex::new(stream),
            pending: Mutex::new(HashMap::new()),
            reads: Mutex::new(HashMap::new()),
            commands: Mutex::new(None),
            alive: AtomicBool::new(true),
        });
        Ok((connection, reader))
    }

    pub(crate) fn set_commands(&self, sender: std::sync::mpsc::Sender<HostCommand>) {
        *self.commands.lock().expect("commands") = Some(sender);
    }

    pub(crate) fn spawn_reader(self: &Arc<Self>, reader: UnixStream) {
        let inbound = Arc::clone(self);
        std::thread::spawn(move || inbound.read_loop(reader));
    }

    pub(crate) fn reply_start(&self, execution_id: &str, result: Result<()>) {
        let frame = match result {
            Ok(()) => Frame::Started {
                execution_id: execution_id.to_string(),
            },
            Err(err) => Frame::StartError {
                execution_id: execution_id.to_string(),
                message: err.to_string(),
            },
        };
        if let Ok(mut stream) = self.writer.lock() {
            let _ = write_frame(&mut *stream, &frame);
        }
    }

    pub(crate) fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub(crate) fn close(&self) {
        if let Ok(stream) = self.writer.lock() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    pub(crate) fn dispatch(
        &self,
        execution_id: &str,
        attempt_id: &str,
        key: &str,
        input: &str,
        timeout: Duration,
    ) -> TransportSample {
        if !self.alive() || !self.keys.iter().any(|known| known == key) {
            return TransportSample::failed(DispatchError::Unavailable);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending
            .lock()
            .expect("pending")
            .insert((execution_id.to_string(), attempt_id.to_string()), tx);
        let write = write_frame_timed(
            &mut *self.writer.lock().expect("writer"),
            &Frame::Effect {
                execution_id: execution_id.to_string(),
                attempt_id: attempt_id.to_string(),
                key: key.to_string(),
                input: input.to_string(),
            },
        );
        let Ok(socket_write) = write else {
            self.fail_all();
            return TransportSample::failed(DispatchError::Disconnected);
        };
        let sent = Instant::now();
        let outcome = match rx.recv_timeout(timeout) {
            Ok(Inbound::Value(value)) => Ok(value),
            Ok(Inbound::Failed(message)) => Err(DispatchError::Failed(message)),
            Ok(Inbound::Gone) => Err(DispatchError::Disconnected),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                self.pending
                    .lock()
                    .expect("pending")
                    .remove(&(execution_id.to_string(), attempt_id.to_string()));
                Err(DispatchError::Timeout)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(DispatchError::Disconnected)
            }
        };
        let key = (execution_id.to_string(), attempt_id.to_string());
        let socket_read = self.reads.lock().expect("reads").remove(&key);
        TransportSample {
            outcome,
            socket_write,
            socket_read: socket_read.unwrap_or_default(),
            round_trip: sent.elapsed(),
        }
    }

    fn read_loop(self: Arc<Self>, mut reader: UnixStream) {
        loop {
            if wait_readable(&reader).is_err() {
                self.fail_all();
                return;
            }
            let started = Instant::now();
            match read_frame(&mut reader) {
                Ok(frame) => {
                    let read = started.elapsed();
                    match frame {
                        Frame::EffectResult {
                            execution_id,
                            attempt_id,
                            value,
                        } => self.deliver(execution_id, attempt_id, Inbound::Value(value), read),
                        Frame::EffectError {
                            execution_id,
                            attempt_id,
                            message,
                        } => self.deliver(execution_id, attempt_id, Inbound::Failed(message), read),
                        Frame::Start { execution_id } => {
                            if let Some(sender) = self.commands.lock().expect("commands").as_ref() {
                                let _ = sender.send(HostCommand::Start(execution_id));
                            }
                        }
                        _ => {}
                    }
                }
                Err(_) => {
                    self.fail_all();
                    return;
                }
            }
        }
    }

    fn deliver(&self, execution_id: String, attempt_id: String, result: Inbound, read: Duration) {
        let key = (execution_id, attempt_id);
        self.reads.lock().expect("reads").insert(key.clone(), read);
        let sender = self.pending.lock().expect("pending").remove(&key);
        if let Some(sender) = sender {
            let _ = sender.send(result);
        }
    }

    fn fail_all(&self) {
        self.alive.store(false, Ordering::SeqCst);
        let pending = std::mem::take(&mut *self.pending.lock().expect("pending"));
        for (_, sender) in pending {
            let _ = sender.send(Inbound::Gone);
        }
    }
}

pub(crate) enum DispatchError {
    Timeout,
    Disconnected,
    Unavailable,
    Failed(String),
}

/// Socket write is the finished request frame. Socket read starts once the
/// response's first byte is readable. Round trip includes the callback.
pub(crate) struct TransportSample {
    pub outcome: std::result::Result<String, DispatchError>,
    pub socket_write: Duration,
    pub socket_read: Duration,
    pub round_trip: Duration,
}

impl TransportSample {
    pub(crate) fn failed(outcome: DispatchError) -> Self {
        Self {
            outcome: Err(outcome),
            socket_write: Duration::ZERO,
            socket_read: Duration::ZERO,
            round_trip: Duration::ZERO,
        }
    }
}

/// Test and binary client. Authentication happens before register or dispatch.
pub struct Adapter {
    writer: Mutex<UnixStream>,
    stop: Arc<AtomicBool>,
}

pub struct EffectRequest {
    pub execution_id: String,
    pub attempt_id: String,
    pub key: String,
    pub input: String,
}

pub enum Reply {
    Value(String),
    Error(String),
    Disconnect,
    Delay(Duration, String),
    StaleThen(String),
    Duplicate(String),
}

impl Adapter {
    pub fn connect(dir: &Path) -> Result<Self> {
        let bearer = fs::read_to_string(dir.join("bearer"))
            .map_err(|err| Error::new(format!("read bearer: {err}")))?;
        let mut stream = UnixStream::connect(dir.join("pulse.sock"))
            .map_err(|err| Error::new(format!("connect: {err}")))?;
        write_frame(&mut stream, &Frame::Auth { bearer })
            .map_err(|err| Error::new(err.to_string()))?;
        match read_frame(&mut stream).map_err(|err| Error::new(err.to_string()))? {
            Frame::AuthOk => {}
            Frame::AuthError { message } => return Err(Error::new(message)),
            _ => return Err(Error::new("expected authOk")),
        }
        Ok(Self {
            writer: Mutex::new(stream),
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    /// `started` means the host registered this id. It does not mean an effect ran.
    pub fn start(&self, execution_id: &str) -> Result<()> {
        let mut stream = self.writer.lock().expect("writer");
        write_frame(
            &mut *stream,
            &Frame::Start {
                execution_id: execution_id.to_string(),
            },
        )
        .map_err(|err| Error::new(err.to_string()))?;
        match read_frame(&mut *stream).map_err(|err| Error::new(err.to_string()))? {
            Frame::Started { execution_id: id } if id == execution_id => Ok(()),
            Frame::StartError { message, .. } => Err(Error::new(message)),
            other => Err(Error::new(format!("expected started, got {other:?}"))),
        }
    }

    pub fn register(&self, artifact_hash: &str, keys: &[&str]) -> Result<()> {
        let mut stream = self.writer.lock().expect("writer");
        write_frame(
            &mut *stream,
            &Frame::Register {
                artifact_hash: artifact_hash.to_string(),
                keys: keys.iter().map(|key| (*key).to_string()).collect(),
            },
        )
        .map_err(|err| Error::new(err.to_string()))?;
        match read_frame(&mut *stream).map_err(|err| Error::new(err.to_string()))? {
            Frame::Registered => Ok(()),
            other => Err(Error::new(format!("expected registered, got {other:?}"))),
        }
    }

    pub fn serve<F>(&self, handler: F) -> Result<()>
    where
        F: Fn(EffectRequest) -> Reply + Send + Sync + 'static,
    {
        let mut reader = self
            .writer
            .lock()
            .expect("writer")
            .try_clone()
            .map_err(|err| Error::new(err.to_string()))?;
        let writer = self
            .writer
            .lock()
            .expect("writer")
            .try_clone()
            .map_err(|err| Error::new(err.to_string()))?;
        let writer = Arc::new(Mutex::new(writer));
        let stop = Arc::clone(&self.stop);
        let handler = Arc::new(handler);
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let frame = match read_frame(&mut reader) {
                    Ok(frame) => frame,
                    Err(_) => return,
                };
                let Frame::Effect {
                    execution_id,
                    attempt_id,
                    key,
                    input,
                } = frame
                else {
                    continue;
                };
                let handler = Arc::clone(&handler);
                let writer = Arc::clone(&writer);
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let reply = handler(EffectRequest {
                        execution_id: execution_id.clone(),
                        attempt_id: attempt_id.clone(),
                        key,
                        input,
                    });
                    let mut stream = writer.lock().expect("writer");
                    match reply {
                        Reply::Value(value) => {
                            let _ = write_frame(
                                &mut *stream,
                                &Frame::EffectResult {
                                    execution_id,
                                    attempt_id,
                                    value,
                                },
                            );
                        }
                        Reply::Error(message) => {
                            let _ = write_frame(
                                &mut *stream,
                                &Frame::EffectError {
                                    execution_id,
                                    attempt_id,
                                    message,
                                },
                            );
                        }
                        Reply::Disconnect => {
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                        }
                        Reply::Delay(duration, value) => {
                            drop(stream);
                            std::thread::sleep(duration);
                            if stop.load(Ordering::SeqCst) {
                                return;
                            }
                            let mut stream = writer.lock().expect("writer");
                            let _ = write_frame(
                                &mut *stream,
                                &Frame::EffectResult {
                                    execution_id,
                                    attempt_id,
                                    value,
                                },
                            );
                        }
                        Reply::StaleThen(value) => {
                            let _ = write_frame(
                                &mut *stream,
                                &Frame::EffectResult {
                                    execution_id: execution_id.clone(),
                                    attempt_id: "stale".to_string(),
                                    value: value.clone(),
                                },
                            );
                            let _ = write_frame(
                                &mut *stream,
                                &Frame::EffectResult {
                                    execution_id,
                                    attempt_id,
                                    value,
                                },
                            );
                        }
                        Reply::Duplicate(value) => {
                            let _ = write_frame(
                                &mut *stream,
                                &Frame::EffectResult {
                                    execution_id: execution_id.clone(),
                                    attempt_id: attempt_id.clone(),
                                    value: value.clone(),
                                },
                            );
                            let _ = write_frame(
                                &mut *stream,
                                &Frame::EffectResult {
                                    execution_id,
                                    attempt_id,
                                    value,
                                },
                            );
                        }
                    }
                });
            }
        });
        Ok(())
    }

    pub fn disconnect(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(stream) = self.writer.lock() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    pub fn push_result(&self, execution_id: &str, attempt_id: &str, value: &str) -> Result<()> {
        let mut stream = self.writer.lock().expect("writer");
        write_frame(
            &mut *stream,
            &Frame::EffectResult {
                execution_id: execution_id.to_string(),
                attempt_id: attempt_id.to_string(),
                value: value.to_string(),
            },
        )
        .map_err(|err| Error::new(err.to_string()))
    }
}

pub fn listen(dir: &Path) -> Result<()> {
    let listener = Listener::bind(dir)?;
    eprintln!(
        "trigora-pulse listening on {}",
        listener.socket_path().display()
    );
    loop {
        let (stream, _) = listener
            .accept()
            .map_err(|err| Error::new(err.to_string()))?;
        let bearer = listener.bearer().to_string();
        std::thread::spawn(move || {
            let _ = Connection::handshake(stream, &bearer);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        root.pop();
        root.pop();
        root.push("target");
        root.push("pulse");
        let path = root.join(format!("s{}-{n}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn uid_check_rejects_a_different_user() {
        assert!(uid_allowed(10, 10));
        assert!(!uid_allowed(11, 10));
    }

    #[test]
    fn register_before_auth_is_rejected() {
        let dir = temp_dir("auth");
        let listener = Listener::bind(&dir).unwrap();
        let dir_thread = dir.clone();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            Connection::handshake(stream, listener.bearer()).map(|_| ())
        });
        let mut client = UnixStream::connect(dir_thread.join("pulse.sock")).unwrap();
        write_frame(
            &mut client,
            &Frame::Register {
                artifact_hash: "hash".to_string(),
                keys: vec!["work".to_string()],
            },
        )
        .unwrap();
        let err = handle.join().unwrap().unwrap_err();
        assert_eq!(err.to_string(), "authentication is required");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_socket_is_replaced_and_a_symlink_is_refused() {
        let dir = temp_dir("stale");
        let listener = Listener::bind(&dir).unwrap();
        drop(listener);
        let again = Listener::bind(&dir).unwrap();
        drop(again);
        let socket = dir.join("pulse.sock");
        let _ = fs::remove_file(&socket);
        std::os::unix::fs::symlink("elsewhere", &socket).unwrap();
        let err = match Listener::bind(&dir) {
            Ok(_) => panic!("symlink should be refused"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("symlink"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn directory_is_private() {
        let dir = temp_dir("mode");
        let listener = Listener::bind(&dir).unwrap();
        let mode = fs::metadata(listener.dir()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let socket_mode = fs::metadata(listener.socket_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(socket_mode, 0o600);
        drop(listener);
        let _ = fs::remove_dir_all(dir);
    }
}
