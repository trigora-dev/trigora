use std::env;
use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use crate::{EffectEndpoint, EventSink, Listeners, LocalRuntime};

pub fn run() -> ExitCode {
    match serve() {
        Ok(listeners) => {
            listeners.block();
            ExitCode::SUCCESS
        }
        Err(message) if message.is_empty() => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn serve() -> Result<Listeners, String> {
    let mut args = env::args().skip(1);
    let mut db = None;
    let mut host = "127.0.0.1".to_string();
    let mut port = 8787_u16;
    let mut effect_url = None;
    let mut effect_secret = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = args.next(),
            "--host" => host = args.next().ok_or("`--host` needs a value.")?,
            "--port" => {
                let value = args.next().ok_or("`--port` needs a value.")?;
                port = value
                    .parse()
                    .map_err(|_| format!("invalid port `{value}`"))?;
            }
            "--effect-url" => effect_url = args.next(),
            "--effect-secret" => effect_secret = args.next(),
            "--help" | "-h" => {
                eprintln!(
                    "trigora-local --db <path> [--host <host>] [--port <port>] [--effect-url <url>] [--effect-secret <secret>]"
                );
                return Err(String::new());
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    let db = db.ok_or("`--db` is required.")?;
    let effect = match (effect_url, effect_secret) {
        (Some(url), Some(secret)) => Some(EffectEndpoint { url, secret }),
        (None, None) => None,
        _ => return Err("`--effect-url` and `--effect-secret` are set together.".to_string()),
    };
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let events: EventSink = {
        let stdout = Arc::clone(&stdout);
        Arc::new(move |event| {
            let mut out = stdout.lock().expect("stdout");
            let _ = writeln!(out, "{event}");
            let _ = out.flush();
        })
    };
    let runtime = LocalRuntime::open(db, effect, events).map_err(|err| err.to_string())?;
    let restored = runtime.restored_waiting().map_err(|err| err.to_string())?;
    let listeners =
        Listeners::spawn(Arc::new(runtime), &host, port).map_err(|err| err.to_string())?;
    {
        let mut out = stdout.lock().expect("stdout");
        let _ = writeln!(
            out,
            "{}",
            serde_json::json!({
                "type": "ready",
                "port": listeners.port,
                "controlPort": listeners.control_port,
                "controlToken": listeners.control_token,
                "restored": restored,
            })
        );
        let _ = out.flush();
    }
    Ok(listeners)
}
