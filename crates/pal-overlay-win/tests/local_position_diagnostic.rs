#![cfg(all(windows, feature = "development-local-readonly-position"))]

use std::time::Duration;

use pal_domain::{PositionSample, SampleClock};
use pal_overlay_win::local_position_runtime::{
    DevelopmentLocalPositionDiagnosticCliError, DevelopmentLocalPositionDiagnosticCommand,
    DevelopmentLocalPositionDiagnosticError, DevelopmentLocalPositionDiagnosticRecord,
};

const DIAGNOSTIC_FLAG: &str = "--development-local-position-diagnostic-json";

#[test]
fn diagnostic_command_is_explicit_bounded_and_order_independent() {
    let command = DevelopmentLocalPositionDiagnosticCommand::parse([
        "--timeout-seconds",
        "7",
        DIAGNOSTIC_FLAG,
    ])
    .expect("explicit bounded diagnostic");
    assert_eq!(command.timeout(), Duration::from_secs(7));

    let default = DevelopmentLocalPositionDiagnosticCommand::parse([DIAGNOSTIC_FLAG])
        .expect("diagnostic default");
    assert_eq!(default.timeout(), Duration::from_secs(15));
}

#[test]
fn diagnostic_command_rejects_unknown_duplicate_and_overlay_arguments() {
    for arguments in [
        vec![DIAGNOSTIC_FLAG, "--bogus"],
        vec![DIAGNOSTIC_FLAG, DIAGNOSTIC_FLAG],
        vec![DIAGNOSTIC_FLAG, "--real-map-bmp", "map.bmp"],
        vec![
            DIAGNOSTIC_FLAG,
            "--timeout-seconds",
            "3",
            "--timeout-seconds",
            "4",
        ],
    ] {
        assert!(DevelopmentLocalPositionDiagnosticCommand::parse(arguments).is_err());
    }

    assert_eq!(
        DevelopmentLocalPositionDiagnosticCommand::parse(["--timeout-seconds", "3"]),
        Err(DevelopmentLocalPositionDiagnosticCliError::MissingModeOptIn)
    );
    assert_eq!(
        DevelopmentLocalPositionDiagnosticCommand::parse([
            DIAGNOSTIC_FLAG,
            "--timeout-seconds",
            "0",
        ]),
        Err(DevelopmentLocalPositionDiagnosticCliError::TimeoutOutOfRange)
    );
    assert_eq!(
        DevelopmentLocalPositionDiagnosticCommand::parse([
            DIAGNOSTIC_FLAG,
            "--timeout-seconds",
            "31",
        ]),
        Err(DevelopmentLocalPositionDiagnosticCliError::TimeoutOutOfRange)
    );
}

#[test]
fn fresh_record_is_machine_readable_exact_build_provenance_without_identity() {
    let sample = PositionSample::new(
        "secret-world",
        b"private-player-id",
        b"private-agent-boot",
        8,
        19,
        -327_711.295_4,
        216_974.095_1,
        1_234.5,
        Some(136.8173),
        SampleClock::received_with_age(25, 1_000),
    )
    .unwrap();

    let record = DevelopmentLocalPositionDiagnosticRecord::from_fresh_sample(&sample, 1_075)
        .expect("fresh exact-build record");
    let line = record.to_json_line();
    let json: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");

    assert_eq!(json["schema"], "pal_companion.gate_b_position.v1");
    assert_eq!(json["source"], "development_local_readonly_exact_build");
    assert_eq!(json["build"]["steam_build_id"], 24_575_825);
    assert_eq!(json["build"]["executable_file_size"], 161_397_248);
    assert_eq!(json["build"]["loaded_module_size"], 167_432_192);
    assert_eq!(
        json["build"]["executable_sha256"],
        "fe3c15064524bae1947852467c4f92bc22469acc033a3d3c8031eab4324e41e8"
    );
    assert_eq!(json["build"]["gate_b_approved"], false);
    assert_eq!(json["sample"]["sequence"], 19);
    assert_eq!(json["sample"]["age_upper_bound_ms"], 100);
    assert_eq!(json["sample"]["x"], -327_711.295_4);
    assert_eq!(json["sample"]["y"], 216_974.095_1);
    assert_eq!(json["sample"]["z"], 1_234.5);
    assert_eq!(json["sample"]["yaw_degrees"], 136.8173);
    assert!(!line.contains("secret-world"));
    assert!(!line.contains("private-player-id"));
    assert!(!line.contains("private-agent-boot"));
}

#[test]
fn diagnostic_record_fails_closed_for_stale_or_headingless_samples() {
    let stale = PositionSample::new(
        "world",
        [0x11; 32],
        [0x22; 32],
        1,
        1,
        1.0,
        2.0,
        3.0,
        Some(4.0),
        SampleClock::received_with_age(1_501, 10),
    )
    .unwrap();
    assert_eq!(
        DevelopmentLocalPositionDiagnosticRecord::from_fresh_sample(&stale, 10),
        Err(DevelopmentLocalPositionDiagnosticError::StaleSample)
    );

    let no_heading = PositionSample::new(
        "world",
        [0x11; 32],
        [0x22; 32],
        1,
        1,
        1.0,
        2.0,
        3.0,
        None,
        SampleClock::received_with_age(0, 10),
    )
    .unwrap();
    assert_eq!(
        DevelopmentLocalPositionDiagnosticRecord::from_fresh_sample(&no_heading, 10),
        Err(DevelopmentLocalPositionDiagnosticError::MissingYaw)
    );
}

#[test]
fn headless_diagnostic_path_does_not_create_an_overlay_window() {
    const SOURCE: &str = include_str!("../src/bin/pal-overlay.rs");
    let start = SOURCE
        .find("fn run_development_local_position_diagnostic(")
        .expect("headless diagnostic entry point");
    let end = SOURCE[start..]
        .find("fn run_development_local_position_preview(")
        .map(|offset| start + offset)
        .expect("diagnostic ends before overlay preview");
    let diagnostic = &SOURCE[start..end];

    assert!(!diagnostic.contains("OverlayWindowHost::create"));
    assert!(!diagnostic.contains("MapRaster::load_bmp_file"));
    assert!(!diagnostic.contains("set_requested_visible"));
}
