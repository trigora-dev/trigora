mod adapter;
mod protocol;
mod server;

pub use adapter::{Adapter, EffectRequest, Reply};
pub use protocol::{Frame, ADAPTER_PROTOCOL};
pub use server::{listen, Listener};

pub(crate) use server::{Connection, DispatchError, HostCommand, TransportSample};

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    use super::protocol::write_frame;
    use super::server::{uid_allowed, Connection};
    use super::{Frame, Listener};

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
            Connection::handshake(stream, listener.bearer(), |_, _, _| Ok(())).map(|_| ())
        });
        let mut client = UnixStream::connect(dir_thread.join("pulse.sock")).unwrap();
        write_frame(
            &mut client,
            &Frame::Register {
                artifact_hash: "hash".to_string(),
                keys: vec!["work".to_string()],
                artifact: None,
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
