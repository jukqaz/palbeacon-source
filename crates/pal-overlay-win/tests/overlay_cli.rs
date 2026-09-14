use std::process::Command;

#[test]
fn product_binary_has_a_non_gui_help_probe() {
    let output = Command::new(env!("CARGO_BIN_EXE_pal-overlay"))
        .arg("--help")
        .output()
        .expect("run overlay help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 help");
    assert!(stdout.contains("Pal Companion overlay"));
    assert!(stdout.contains("Palworld-Win64-Shipping.exe"));
}

#[cfg(not(feature = "development-live-performance-diagnostic"))]
#[test]
fn default_help_excludes_live_performance_diagnostic() {
    let output = Command::new(env!("CARGO_BIN_EXE_pal-overlay"))
        .arg("--help")
        .output()
        .expect("run default preview help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 help");
    assert!(!stdout.contains("live-performance-diagnostic"));
}

#[cfg(feature = "development-live-performance-diagnostic")]
#[test]
fn feature_help_includes_live_performance_diagnostic() {
    let output = Command::new(env!("CARGO_BIN_EXE_pal-overlay"))
        .arg("--help")
        .output()
        .expect("run diagnostic preview help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 help");
    assert!(stdout.contains("--development-local-live-performance-diagnostic-json"));
}

#[cfg(feature = "test-harness")]
#[test]
fn synthetic_preview_exits_after_the_requested_duration() {
    let output = Command::new(env!("CARGO_BIN_EXE_pal-overlay"))
        .args(["--synthetic-map", "--duration-seconds", "1"])
        .output()
        .expect("run bounded synthetic preview");

    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 status");
    assert!(stderr.contains("Synthetic grid preview prepared"));
    assert!(stderr.contains("Preview duration elapsed"));
}
