use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Socket handshake spoken by this host. The CLI refuses a different number.
pub const ADAPTER_PROTOCOL: u32 = 1;

const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Frame {
    #[serde(rename = "auth")]
    Auth { bearer: String },
    #[serde(rename = "authOk")]
    AuthOk { version: String, protocol: u32 },
    #[serde(rename = "authError")]
    AuthError { message: String },
    #[serde(rename = "register")]
    Register {
        #[serde(rename = "artifactHash")]
        artifact_hash: String,
        keys: Vec<String>,
        #[serde(default)]
        artifact: Option<String>,
    },
    #[serde(rename = "registered")]
    Registered,
    #[serde(rename = "registerError")]
    RegisterError { message: String },
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
    #[serde(rename = "resume")]
    Resume {
        #[serde(rename = "executionId")]
        execution_id: String,
    },
    #[serde(rename = "resumed")]
    Resumed {
        #[serde(rename = "executionId")]
        execution_id: String,
    },
    #[serde(rename = "resumeError")]
    ResumeError {
        #[serde(rename = "executionId")]
        execution_id: String,
        message: String,
    },
    #[serde(rename = "event")]
    Event {
        #[serde(rename = "executionId")]
        execution_id: String,
        name: String,
        payload: String,
    },
    #[serde(rename = "eventOk")]
    EventOk {
        #[serde(rename = "executionId")]
        execution_id: String,
    },
    #[serde(rename = "eventError")]
    EventError {
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
pub(super) fn write_frame_timed(stream: &mut impl Write, frame: &Frame) -> io::Result<Duration> {
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
