use std::process::Command;

#[test]
fn product_overlay_binary_preserves_the_existing_non_gui_help_probe() {
    let output = Command::new(env!("CARGO_BIN_EXE_pal-overlay"))
        .arg("--help")
        .output()
        .expect("pal-overlay help probe must start");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Pal Companion overlay"));
    assert!(stdout.contains("Palworld-Win64-Shipping.exe"));
}
