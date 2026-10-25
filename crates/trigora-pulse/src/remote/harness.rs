use std::thread;
use std::time::Duration;

use serde_json::json;

use super::{RemoteClient, HARD_CONTINUATION_MAX};
use crate::error::{Error, Result};

pub(crate) struct TestAuthority {
    child: std::process::Child,
    client: RemoteClient,
}

impl Drop for TestAuthority {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl RemoteClient {
    pub(crate) fn test_hook(&self, path: &str, execution: &str) -> Result<()> {
        let body = serde_json::to_vec(&json!({ "execution": execution }))
            .map_err(|err| Error::new(err.to_string()))?;
        self.endpoint
            .call("POST", path, "A", Some(&body), "application/json")?;
        Ok(())
    }

    pub(crate) fn test_renewals(&self, execution: &str) -> Result<u64> {
        let response = self.endpoint.call(
            "GET",
            &format!("/v1/test/renewals/{execution}"),
            "A",
            None,
            "text/plain",
        )?;
        String::from_utf8_lossy(&response.body)
            .trim()
            .parse()
            .map_err(|err| Error::new(format!("renewal count: {err}")))
    }
}

impl TestAuthority {
    pub(crate) fn client(&self) -> RemoteClient {
        self.client.fresh()
    }
}

pub(crate) fn serve() -> TestAuthority {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};

    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../trigora-platform/apps/pulse-authority/src/serve.ts");
    let mut command = Command::new("node");
    command
        .arg("--experimental-strip-types")
        .arg(&script)
        .env("PULSE_TEST_HOOKS", "1");
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("start pulse authority: {err}"));
    let stderr = child.stderr.take().expect("stderr");
    let errors = Arc::new(Mutex::new(String::new()));
    let logged = Arc::clone(&errors);
    thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stderr);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            logged.lock().expect("stderr").push_str(&line);
            line.clear();
        }
    });
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!(
                "pulse authority did not listen: {}",
                errors.lock().expect("stderr")
            );
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                let _ = child.wait();
                panic!("pulse authority exited: {}", errors.lock().expect("stderr"));
            }
            Ok(_) => {
                if let Some(port) = line.trim().strip_prefix("listening ") {
                    let url = format!("http://127.0.0.1:{port}");
                    let client =
                        RemoteClient::sqlite(url, "ws", "workspace-token", HARD_CONTINUATION_MAX);
                    return TestAuthority { child, client };
                }
            }
            Err(err) => panic!("read authority port: {err}"),
        }
    }
}
