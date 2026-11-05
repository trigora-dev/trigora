use std::io::Write;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::push_programs;
use crate::adapter::Registry;
use crate::compile::discover;
use crate::config::ProjectConfig;
use crate::model::Program;

pub(super) fn session_loop(
    rx: Receiver<bool>,
    config: &ProjectConfig,
    registry: &Registry,
    programs: &Arc<Mutex<Vec<Program>>>,
    watcher: &mut RecommendedWatcher,
    control_port: u16,
    control_token: &str,
) {
    let mut pending = false;
    loop {
        let wait = if pending {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(60 * 60)
        };
        match rx.recv_timeout(wait) {
            Ok(true) => break,
            Ok(false) => pending = true,
            Err(mpsc::RecvTimeoutError::Timeout) if pending => {
                pending = false;
                reload(
                    config,
                    registry,
                    programs,
                    watcher,
                    control_port,
                    control_token,
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn reload(
    config: &ProjectConfig,
    registry: &Registry,
    programs: &Arc<Mutex<Vec<Program>>>,
    watcher: &mut RecommendedWatcher,
    control_port: u16,
    control_token: &str,
) {
    match discover(&config.root, &config.programs, registry) {
        Ok(next) => {
            if let Err(error) = push_programs(control_port, control_token, &next) {
                eprintln!();
                eprintln!("✖ Reload failed");
                eprintln!("{error}");
                return;
            }
            let names = next
                .iter()
                .map(|program| program.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            *programs.lock().expect("programs") = next;
            watch_files(
                watcher,
                config,
                &programs.lock().expect("programs"),
                registry,
            );
            println!();
            println!("Programs changed. Reloaded {names}.");
            let _ = std::io::stdout().flush();
        }
        Err(error) => {
            eprintln!();
            eprintln!("✖ Reload failed");
            eprintln!("{error}");
        }
    }
}

pub(super) fn watch_files(
    watcher: &mut RecommendedWatcher,
    config: &ProjectConfig,
    programs: &[Program],
    registry: &Registry,
) {
    let mut paths = vec![config.path.clone()];
    for program in programs {
        paths.push(config.root.join(&program.file));
    }
    paths.extend(registry.watch_paths(&config.root, programs));
    for path in paths {
        let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
    }
}
