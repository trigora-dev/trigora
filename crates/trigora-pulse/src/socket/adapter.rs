use std::fs;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::protocol::{read_frame, write_frame, Frame};
use crate::error::{Error, Result};

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
            Frame::AuthOk { version, protocol }
                if version == env!("CARGO_PKG_VERSION") && protocol == crate::ADAPTER_PROTOCOL => {}
            Frame::AuthOk { version, protocol } => {
                return Err(Error::new(format!(
                    "Incompatible Pulse host {version} protocol {protocol}"
                )));
            }
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
                artifact: None,
            },
        )
        .map_err(|err| Error::new(err.to_string()))?;
        Self::expect_registered(&mut stream)
    }

    pub fn register_artifact(
        &self,
        artifact_hash: &str,
        keys: &[&str],
        artifact: &str,
    ) -> Result<()> {
        let mut stream = self.writer.lock().expect("writer");
        write_frame(
            &mut *stream,
            &Frame::Register {
                artifact_hash: artifact_hash.to_string(),
                keys: keys.iter().map(|key| (*key).to_string()).collect(),
                artifact: Some(artifact.to_string()),
            },
        )
        .map_err(|err| Error::new(err.to_string()))?;
        Self::expect_registered(&mut stream)
    }

    fn expect_registered(stream: &mut UnixStream) -> Result<()> {
        match read_frame(stream).map_err(|err| Error::new(err.to_string()))? {
            Frame::Registered => Ok(()),
            Frame::RegisterError { message } => Err(Error::new(message)),
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
