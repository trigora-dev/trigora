use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

use crate::error::CliError;

pub(super) const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(10);

pub(super) fn wait_until_ready(
    host: &mut Child,
    adapter: &mut Child,
    signal: &std::sync::mpsc::Receiver<()>,
    dir: &Path,
) -> Result<(), CliError> {
    let ready = dir.join("ready.json");
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        if ready.is_file() {
            return Ok(());
        }
        if signal.try_recv().is_ok() {
            return Err(CliError::new("Pulse shutdown exceeded 10 seconds.").with_code(1));
        }
        if let Some(status) = host
            .try_wait()
            .map_err(|error| CliError::plain(error.to_string()))?
        {
            return Err(CliError::new("Pulse host exited")
                .detail("Status", status.code().unwrap_or(1).to_string())
                .with_code(status.code().unwrap_or(1)));
        }
        if let Some(status) = adapter
            .try_wait()
            .map_err(|error| CliError::plain(error.to_string()))?
        {
            return Err(CliError::new("Pulse adapter exited")
                .detail("Status", status.code().unwrap_or(1).to_string())
                .with_code(status.code().unwrap_or(1)));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(CliError::plain("Pulse did not become ready."))
}

pub(super) fn wait_child(child: &mut Child, limit: Duration) -> Option<std::process::ExitStatus> {
    let started = Instant::now();
    while started.elapsed() < limit {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.try_wait().ok().flatten()
}

pub(super) fn signal(child: &Child, sig: i32) -> Result<(), CliError> {
    let pid = child.id() as i32;
    let result = unsafe { libc::kill(pid, sig) };
    if result == 0 {
        Ok(())
    } else {
        Err(CliError::plain("could not signal the Pulse host"))
    }
}
