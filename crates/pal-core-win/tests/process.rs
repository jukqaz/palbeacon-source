use std::process::Command;

#[test]
fn product_core_binary_has_a_non_blocking_health_probe() {
    let local_app_data = tempfile::tempdir().expect("isolated LOCALAPPDATA");
    let output = Command::new(env!("CARGO_BIN_EXE_pal-core"))
        .arg("--health-check")
        .env("LOCALAPPDATA", local_app_data.path())
        .output()
        .expect("pal-core health probe must start");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "pal-core: ready\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn product_core_binary_rejects_unknown_arguments_without_starting_ipc() {
    let output = Command::new(env!("CARGO_BIN_EXE_pal-core"))
        .arg("--unknown")
        .output()
        .expect("pal-core invalid-argument probe must start");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "pal-core startup failed: unrecognized pal-core arguments\n"
    );
}
