use std::path::{Path, PathBuf};

use crate::error::CliError;

pub struct Tools {
    pub local_bin: PathBuf,
    pub node_helper: Option<PathBuf>,
    pub rust_compiler: Option<PathBuf>,
}

pub fn tools() -> Result<Tools, CliError> {
    let exe = std::env::current_exe().unwrap_or_default();
    Ok(Tools {
        local_bin: local_runtime_from(&exe)?,
        node_helper: optional("TRIGORA_NODE_HELPER"),
        rust_compiler: rust_compiler_from(&exe),
    })
}

pub fn local_runtime_from(exe: &Path) -> Result<PathBuf, CliError> {
    const MISSING: &str = "The local runtime binary is missing. Reinstall trigora.";
    if std::env::var_os("TRIGORA_LOCAL_BIN").is_some() {
        return std::env::var("TRIGORA_LOCAL_BIN")
            .ok()
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .ok_or_else(|| CliError::plain(MISSING));
    }
    let name = local_file_name();
    let Some(dir) = exe.parent() else {
        return Err(CliError::plain(MISSING));
    };
    let sibling = dir.join(&name);
    if sibling.is_file() {
        return Ok(sibling);
    }
    if let Some(platform) = dir.file_name() {
        if let Some(vendor) = dir.parent().and_then(|parent| parent.parent()) {
            let packaged = vendor.join("trigora-local").join(platform).join(&name);
            if packaged.is_file() {
                return Ok(packaged);
            }
        }
    }
    Err(CliError::plain(MISSING))
}

fn local_file_name() -> &'static str {
    if cfg!(windows) {
        "trigora-local.exe"
    } else {
        "trigora-local"
    }
}

pub fn rust_compiler_from(exe: &Path) -> Option<PathBuf> {
    if let Some(path) = optional("TRIGORA_RUST_COMPILER_BIN") {
        return Some(path);
    }
    let name = compiler_file_name();
    let dir = exe.parent()?;
    let sibling = dir.join(&name);
    if sibling.is_file() {
        return Some(sibling);
    }
    let platform = dir.file_name()?;
    let vendor = dir.parent()?.parent()?;
    let packaged = vendor.join("tcc-rust-compile").join(platform).join(&name);
    if packaged.is_file() {
        return Some(packaged);
    }
    None
}

fn compiler_file_name() -> &'static str {
    if cfg!(windows) {
        "tcc-rust-compile.exe"
    } else {
        "tcc-rust-compile"
    }
}

fn optional(var: &str) -> Option<PathBuf> {
    std::env::var(var)
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 24];
    getrandom::getrandom(&mut bytes).expect("random");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn workspace_hash(programs: &[crate::model::Program]) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut ordered = programs.to_vec();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));
    for program in ordered {
        hash.update(program.id.as_bytes());
        hash.update([0]);
        hash.update(program.artifact_hash.as_bytes());
        hash.update([0]);
    }
    format!("{:x}", hash.finalize())
}

pub fn temp_file(name: &str, contents: &str) -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("trigora-{}-{name}", random_token()));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, contents)?;
    Ok(path)
}

pub fn command_output(
    program: &Path,
    args: &[&str],
    stdin: &str,
) -> std::io::Result<std::process::Output> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut input) = child.stdin.take() {
        use std::io::Write;
        input.write_all(stdin.as_bytes())?;
    }
    child.wait_with_output()
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn the_rust_compiler_is_found_beside_the_executable() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var("TRIGORA_RUST_COMPILER_BIN").ok();
        std::env::remove_var("TRIGORA_RUST_COMPILER_BIN");
        let dir = std::env::temp_dir().join(format!("trigora-beside-{}", random_token()));
        let elsewhere = std::env::temp_dir().join(format!("trigora-path-{}", random_token()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let exe = dir.join("trigora");
        let compiler = dir.join(compiler_file_name());
        std::fs::write(&exe, b"exe").unwrap();
        std::fs::write(&compiler, b"compiler").unwrap();
        std::fs::write(elsewhere.join(compiler_file_name()), b"not-on-path").unwrap();
        assert_eq!(rust_compiler_from(&exe).unwrap(), compiler);

        let isolated = std::env::temp_dir().join(format!("trigora-isolated-{}", random_token()));
        std::fs::create_dir_all(&isolated).unwrap();
        let lone = isolated.join("trigora");
        std::fs::write(&lone, b"exe").unwrap();
        assert!(rust_compiler_from(&lone).is_none());

        let vendor = std::env::temp_dir().join(format!("trigora-vendor-{}", random_token()));
        let platform = vendor.join("vendor").join("trigora").join("test-platform");
        std::fs::create_dir_all(&platform).unwrap();
        let packaged_exe = platform.join("trigora");
        std::fs::write(&packaged_exe, b"exe").unwrap();
        let packaged = vendor
            .join("vendor")
            .join("tcc-rust-compile")
            .join("test-platform")
            .join(compiler_file_name());
        std::fs::create_dir_all(packaged.parent().unwrap()).unwrap();
        std::fs::write(&packaged, b"compiler").unwrap();
        assert_eq!(rust_compiler_from(&packaged_exe).unwrap(), packaged);

        std::env::set_var("TRIGORA_RUST_COMPILER_BIN", &compiler);
        assert_eq!(rust_compiler_from(&lone).unwrap(), compiler);
        match previous {
            Some(value) => std::env::set_var("TRIGORA_RUST_COMPILER_BIN", value),
            None => std::env::remove_var("TRIGORA_RUST_COMPILER_BIN"),
        }
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(elsewhere);
        let _ = std::fs::remove_dir_all(isolated);
        let _ = std::fs::remove_dir_all(vendor);
    }

    #[test]
    fn the_local_runtime_is_found_beside_the_executable() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var("TRIGORA_LOCAL_BIN").ok();
        std::env::remove_var("TRIGORA_LOCAL_BIN");
        let dir = std::env::temp_dir().join(format!("trigora-local-beside-{}", random_token()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("trigora");
        let runtime = dir.join(local_file_name());
        std::fs::write(&exe, b"exe").unwrap();
        std::fs::write(&runtime, b"runtime").unwrap();
        assert_eq!(local_runtime_from(&exe).unwrap(), runtime);

        let missing = dir.join("missing-runtime");
        std::env::set_var("TRIGORA_LOCAL_BIN", &missing);
        assert!(local_runtime_from(&exe).is_err());

        let vendor = std::env::temp_dir().join(format!("trigora-local-vendor-{}", random_token()));
        let platform = vendor.join("vendor").join("trigora").join("test-platform");
        std::fs::create_dir_all(&platform).unwrap();
        let packaged_exe = platform.join("trigora");
        std::fs::write(&packaged_exe, b"exe").unwrap();
        let packaged = vendor
            .join("vendor")
            .join("trigora-local")
            .join("test-platform")
            .join(local_file_name());
        std::fs::create_dir_all(packaged.parent().unwrap()).unwrap();
        std::fs::write(&packaged, b"runtime").unwrap();
        std::env::remove_var("TRIGORA_LOCAL_BIN");
        assert_eq!(local_runtime_from(&packaged_exe).unwrap(), packaged);

        match previous {
            Some(value) => std::env::set_var("TRIGORA_LOCAL_BIN", value),
            None => std::env::remove_var("TRIGORA_LOCAL_BIN"),
        }
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(vendor);
    }
}
