#![cfg(feature = "development-local-alignment-diagnostic")]

use std::process::Command;

// Inspect the product entry point directly so these structural guards cover the
// same binary used by production and development diagnostics.
const PREVIEW_SOURCE: &str = include_str!("../src/bin/pal-overlay.rs");
const MODE: &str = "--development-local-alignment-diagnostic-json";

#[test]
fn alignment_mode_dispatches_before_every_preview_or_other_local_mode() {
    let main_body = function_body(PREVIEW_SOURCE, "fn main()");
    let alignment = main_body
        .find("local_alignment_diagnostic_mode_requested")
        .expect("alignment dispatch");

    for later in [
        "local_position_diagnostic_mode_requested",
        "live_performance_diagnostic_mode_requested",
        "local_position_mode_requested",
        "run_standard_preview",
    ] {
        assert!(
            alignment < main_body.find(later).expect("later dispatch"),
            "alignment dispatch must precede {later}"
        );
    }
}

#[test]
fn headless_alignment_path_preflights_before_source_and_has_no_overlay_surface_calls() {
    let entry = function_body(
        PREVIEW_SOURCE,
        "fn run_development_local_alignment_diagnostic(",
    );
    let after_preflight = function_body(
        PREVIEW_SOURCE,
        "fn run_development_local_alignment_after_preflight(",
    );
    let parse = entry
        .find("LocalAlignmentDiagnosticCommand::parse")
        .expect("strict command parse");
    let preflight = entry
        .find("preflight_local_alignment_then")
        .expect("protected preflight");
    let source = entry
        .find("run_development_local_alignment_after_preflight")
        .expect("source collection after preflight");
    assert!(parse < preflight && preflight < source);
    let deadline = entry
        .find(".checked_add(command.timeout())")
        .expect("whole-diagnostic timeout deadline");
    assert!(
        deadline < preflight,
        "the single diagnostic timeout budget must start before protected input preflight"
    );

    for body in [entry, after_preflight] {
        for forbidden in [
            "OverlayWindowHost::create",
            "attach_preview(",
            "set_requested_visible",
            "pump_messages",
            "ActualMapSurface",
            "PreviewVisibilityGate",
            "println!",
        ] {
            assert!(
                !body.contains(forbidden),
                "headless alignment path must not contain {forbidden}"
            );
        }
    }
    assert!(entry.contains("write_all(result.to_json_line().as_bytes())"));
    assert!(
        after_preflight.contains("local_alignment_completion_is_before_deadline(Instant::now()"),
        "completion must re-check the absolute deadline after the tenth sample"
    );
    assert!(
        after_preflight
            .matches("local_alignment_completion_is_before_deadline(Instant::now()")
            .count()
            >= 5,
        "blocking discovery phases must re-check the deadline before starting later access"
    );
}

#[test]
fn invalid_alignment_cli_exits_nonzero_without_stdout() {
    let output = Command::new(env!("CARGO_BIN_EXE_pal-overlay"))
        .arg(MODE)
        .output()
        .expect("run preview binary");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn incompatible_preview_flag_is_rejected_before_any_file_or_process_access() {
    let output = Command::new(env!("CARGO_BIN_EXE_pal-overlay"))
        .args([
            MODE,
            "--real-map-bmp",
            "does-not-exist.bmp",
            "--observation-file",
            "does-not-exist.json",
            "--observation-sha256",
            "1111111111111111111111111111111111111111111111111111111111111111",
            "--timeout-seconds",
            "1",
            "--synthetic-map",
        ])
        .output()
        .expect("run preview binary");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

fn function_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source.find(signature).expect("function signature");
    let body_start = source[start..].find('{').unwrap() + start;
    let mut depth = 0_u32;
    for (offset, byte) in source.as_bytes()[body_start..].iter().copied().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[body_start..=body_start + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated function body")
}
