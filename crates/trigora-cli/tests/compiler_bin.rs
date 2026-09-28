#[test]
fn the_compiler_binary_reports_its_version() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tcc-rust-compile"))
        .arg("--version")
        .output()
        .expect("tcc-rust-compile");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "26.10.0");
}
