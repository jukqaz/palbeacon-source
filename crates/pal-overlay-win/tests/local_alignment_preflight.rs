#![cfg(feature = "development-local-alignment-diagnostic")]

use std::{cell::Cell, ffi::OsString, path::Path, time::Duration};

use pal_overlay_win::local_alignment_diagnostic::{
    LocalAlignmentDiagnosticCliError, LocalAlignmentDiagnosticCommand,
    LocalAlignmentDiagnosticError, LocalAlignmentPreflightError, LocalAlignmentPreflightPolicy,
    parse_local_alignment_observation, preflight_local_alignment_then_with_policy,
    preflight_local_alignment_with,
};
use sha2::{Digest, Sha256};

const MODE: &str = "--development-local-alignment-diagnostic-json";
const CSHARP_CANONICAL_OBSERVATION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/pal-map-pack/tests/PalMapPack.Tests/fixtures/",
    "local-alignment-observation-canonical.json"
));
const CSHARP_CANONICAL_NUMERIC_MATRIX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/pal-map-pack/tests/PalMapPack.Tests/fixtures/",
    "local-alignment-observation-canonical-matrix.jsonl"
));

#[test]
fn exact_csharp_exporter_fixture_is_accepted_without_normalization() {
    assert!(!CSHARP_CANONICAL_OBSERVATION.ends_with(b"\n"));
    let document: serde_json::Value = serde_json::from_slice(CSHARP_CANONICAL_OBSERVATION).unwrap();
    let properties = document.as_object().unwrap();
    assert_eq!(properties.len(), 9);
    for property in [
        "schema",
        "claim",
        "game_build_id",
        "map_sha256",
        "map_width_px",
        "map_height_px",
        "observed_marker_x_px",
        "observed_marker_y_px",
        "nonce",
    ] {
        assert!(properties.contains_key(property), "missing {property}");
    }

    let sha = sha256_hex(CSHARP_CANONICAL_OBSERVATION);
    assert!(parse_local_alignment_observation(CSHARP_CANONICAL_OBSERVATION, &sha).is_ok());
}

#[test]
fn csharp_exporter_numeric_matrix_is_accepted_byte_for_byte() {
    assert!(!CSHARP_CANONICAL_NUMERIC_MATRIX.ends_with(b"\n"));
    let observations = CSHARP_CANONICAL_NUMERIC_MATRIX
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .collect::<Vec<_>>();
    assert_eq!(observations.len(), 5);

    for observation in observations {
        assert!(
            parse_local_alignment_observation(observation, &sha256_hex(observation)).is_ok(),
            "{}",
            String::from_utf8_lossy(observation)
        );
    }
}

#[test]
fn canonical_observation_rejects_each_invalid_field_with_a_typed_error() {
    for (field, replacement, expected) in [
        (
            "schema",
            serde_json::json!("other"),
            LocalAlignmentDiagnosticError::ObservationSchema,
        ),
        (
            "claim",
            serde_json::json!("development_smoke_only_not_gate_b"),
            LocalAlignmentDiagnosticError::ObservationClaim,
        ),
        (
            "game_build_id",
            serde_json::json!(24_181_526),
            LocalAlignmentDiagnosticError::ObservationBuild,
        ),
        (
            "map_sha256",
            serde_json::json!("00".repeat(32)),
            LocalAlignmentDiagnosticError::ObservationMapSha,
        ),
        (
            "map_width_px",
            serde_json::json!(2_047),
            LocalAlignmentDiagnosticError::ObservationMapDimensions,
        ),
        (
            "map_height_px",
            serde_json::json!(2_047),
            LocalAlignmentDiagnosticError::ObservationMapDimensions,
        ),
        (
            "observed_marker_x_px",
            serde_json::json!(2_048),
            LocalAlignmentDiagnosticError::ObservationMarker,
        ),
        (
            "observed_marker_y_px",
            serde_json::json!(-1),
            LocalAlignmentDiagnosticError::ObservationMarker,
        ),
        (
            "nonce",
            serde_json::json!("000102030405060708090A0B0C0D0E0F"),
            LocalAlignmentDiagnosticError::ObservationNonce,
        ),
    ] {
        let bytes = mutate_csharp_fixture_field(field, replacement);
        assert_eq!(
            parse_local_alignment_observation(&bytes, &sha256_hex(&bytes)),
            Err(expected),
            "field {field}"
        );
    }

    for nonce in ["".to_owned(), "0".repeat(31), "0".repeat(33)] {
        let bytes = mutate_csharp_fixture_field("nonce", serde_json::json!(nonce));
        assert_eq!(
            parse_local_alignment_observation(&bytes, &sha256_hex(&bytes)),
            Err(LocalAlignmentDiagnosticError::ObservationNonce)
        );
    }
}

#[test]
fn observation_wire_form_rejects_every_noncanonical_representation() {
    let canonical = std::str::from_utf8(CSHARP_CANONICAL_OBSERVATION).unwrap();
    let schema_property = "\"schema\":\"pal_companion.local_alignment_observation.v1\",";
    let claim_property = "\"claim\":\"independent_native_marker_observation_not_gate_b\",";
    let reordered = canonical.replacen(
        &format!("{{{schema_property}{claim_property}"),
        &format!("{{{claim_property}{schema_property}"),
        1,
    );
    let duplicate = canonical.replacen("}", ",\"nonce\":\"000102030405060708090a0b0c0d0e0f\"}", 1);
    let unknown = canonical.replacen("}", ",\"unexpected\":true}", 1);
    let alternate_numeric = canonical.replacen(
        "\"observed_marker_x_px\":100.25",
        "\"observed_marker_x_px\":100.250",
        1,
    );

    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend_from_slice(CSHARP_CANONICAL_OBSERVATION);
    let mut trailing_newline = CSHARP_CANONICAL_OBSERVATION.to_vec();
    trailing_newline.push(b'\n');

    for bytes in [
        bom,
        trailing_newline,
        canonical.replacen('{', "{\r\n", 1).into_bytes(),
        reordered.into_bytes(),
        alternate_numeric.into_bytes(),
        duplicate.into_bytes(),
        unknown.into_bytes(),
    ] {
        assert_eq!(
            parse_local_alignment_observation(&bytes, &sha256_hex(&bytes)),
            Err(LocalAlignmentDiagnosticError::InvalidObservationDocument)
        );
    }
}

#[test]
fn observation_wire_form_rejects_old_keys_and_claim() {
    let old_claim = replace_bytes(
        CSHARP_CANONICAL_OBSERVATION,
        b"independent_native_marker_observation_not_gate_b",
        b"development_smoke_only_not_gate_b",
    );
    let old_map_key = replace_bytes(
        CSHARP_CANONICAL_OBSERVATION,
        b"map_sha256",
        b"map_bmp_sha256",
    );
    let old_nonce_key = replace_bytes(CSHARP_CANONICAL_OBSERVATION, b"nonce", b"capture_nonce");

    for bytes in [old_claim, old_map_key, old_nonce_key] {
        assert!(parse_local_alignment_observation(&bytes, &sha256_hex(&bytes)).is_err());
    }
}

fn mutate_csharp_fixture_field(field: &str, replacement: serde_json::Value) -> Vec<u8> {
    let mut document: serde_json::Value =
        serde_json::from_slice(CSHARP_CANONICAL_OBSERVATION).unwrap();
    document
        .as_object_mut()
        .unwrap()
        .insert(field.to_owned(), replacement);
    serde_json::to_vec(&document).unwrap()
}

fn replace_bytes(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    let offset = haystack
        .windows(needle.len())
        .position(|window| window == needle)
        .unwrap();
    let mut output = Vec::with_capacity(haystack.len() - needle.len() + replacement.len());
    output.extend_from_slice(&haystack[..offset]);
    output.extend_from_slice(replacement);
    output.extend_from_slice(&haystack[offset + needle.len()..]);
    output
}

#[test]
fn command_is_strict_required_bounded_and_order_independent() {
    let bmp = bmp_2x2();
    let map_sha = sha256_hex(&bmp);
    let observation = observation_bytes(&map_sha, 2, 2);
    let observation_sha = sha256_hex(&observation);
    let command = LocalAlignmentDiagnosticCommand::parse([
        "--timeout-seconds",
        "7",
        "--observation-sha256",
        &observation_sha,
        "--real-map-bmp",
        "map.bmp",
        MODE,
        "--observation-file",
        "observation.json",
    ])
    .unwrap();

    assert_eq!(command.timeout(), Duration::from_secs(7));
    assert_eq!(command.map_path(), Path::new("map.bmp"));
    assert_eq!(command.observation_path(), Path::new("observation.json"));
    assert_eq!(command.observation_sha256(), observation_sha);
}

#[test]
fn command_rejects_missing_duplicate_unknown_and_out_of_range_arguments() {
    let sha = "1".repeat(64);
    let valid = valid_arguments(&sha);
    assert_eq!(
        LocalAlignmentDiagnosticCommand::parse(valid[1..].iter().cloned()),
        Err(LocalAlignmentDiagnosticCliError::MissingModeOptIn)
    );

    for (flag, has_value) in [
        (MODE, false),
        ("--real-map-bmp", true),
        ("--observation-file", true),
        ("--observation-sha256", true),
        ("--timeout-seconds", true),
    ] {
        let mut arguments = valid.clone();
        remove_argument(&mut arguments, flag, has_value);
        assert!(LocalAlignmentDiagnosticCommand::parse(arguments).is_err());
    }

    for arguments in [
        vec![OsString::from(MODE), OsString::from("--real-map-bmp")],
        vec![OsString::from(MODE), OsString::from("--observation-file")],
        vec![OsString::from(MODE), OsString::from("--observation-sha256")],
        vec![OsString::from(MODE), OsString::from("--timeout-seconds")],
        vec![OsString::from(MODE), OsString::from(MODE)],
        vec![OsString::from(MODE), OsString::from("--bogus")],
    ] {
        assert!(LocalAlignmentDiagnosticCommand::parse(arguments).is_err());
    }

    for invalid in ["0", "31", "abc"] {
        let mut arguments = valid.clone();
        *arguments.last_mut().unwrap() = OsString::from(invalid);
        assert!(LocalAlignmentDiagnosticCommand::parse(arguments).is_err());
    }

    for invalid_sha in ["a".to_owned(), "G".repeat(64), "A".repeat(64)] {
        let mut arguments = valid.clone();
        arguments[6] = OsString::from(invalid_sha);
        assert!(LocalAlignmentDiagnosticCommand::parse(arguments).is_err());
    }

    for (flag, value) in [
        (MODE, None),
        ("--real-map-bmp", Some("other.bmp")),
        ("--observation-file", Some("other.json")),
        ("--observation-sha256", Some(sha.as_str())),
        ("--timeout-seconds", Some("8")),
    ] {
        let mut duplicate = valid.clone();
        duplicate.push(OsString::from(flag));
        if let Some(value) = value {
            duplicate.push(OsString::from(value));
        }
        assert_eq!(
            LocalAlignmentDiagnosticCommand::parse(duplicate),
            Err(LocalAlignmentDiagnosticCliError::DuplicateArgument)
        );
    }

    for incompatible in [
        "--synthetic-map",
        "--position-replay",
        "--development-local-readonly-position",
        "--development-local-live-performance-diagnostic-json",
    ] {
        let mut arguments = valid.clone();
        arguments.push(OsString::from(incompatible));
        assert_eq!(
            LocalAlignmentDiagnosticCommand::parse(arguments),
            Err(LocalAlignmentDiagnosticCliError::UnknownArgument)
        );
    }
}

#[cfg(windows)]
#[test]
fn command_rejects_non_unicode_hash_and_timeout_values() {
    use std::os::windows::ffi::OsStringExt;

    let sha = "1".repeat(64);
    let invalid = OsString::from_wide(&[0xd800]);

    let mut invalid_hash = valid_arguments(&sha);
    invalid_hash[6] = invalid.clone();
    assert_eq!(
        LocalAlignmentDiagnosticCommand::parse(invalid_hash),
        Err(LocalAlignmentDiagnosticCliError::InvalidObservationSha)
    );

    let mut invalid_timeout = valid_arguments(&sha);
    invalid_timeout[8] = invalid;
    assert_eq!(
        LocalAlignmentDiagnosticCommand::parse(invalid_timeout),
        Err(LocalAlignmentDiagnosticCliError::InvalidTimeout)
    );
}

#[test]
fn observation_failure_happens_before_map_read_or_source_factory() {
    let bmp = bmp_2x2();
    let policy = policy_for(&bmp, 2, 2);
    let observation = observation_bytes(policy.map_sha256(), 2, 2);
    let command = command("0".repeat(64));
    let map_reads = Cell::new(0);
    let source_calls = Cell::new(0);

    let result = preflight_local_alignment_with(
        &command,
        &policy,
        |_, maximum| {
            assert_eq!(maximum, 64 * 1024);
            Ok::<_, ()>(observation.clone())
        },
        |_, _| {
            map_reads.set(map_reads.get() + 1);
            Ok::<_, ()>(bmp.clone())
        },
        |_| {
            source_calls.set(source_calls.get() + 1);
        },
    );

    assert_eq!(
        result,
        Err(LocalAlignmentPreflightError::ObservationInvalid)
    );
    assert_eq!(map_reads.get(), 0);
    assert_eq!(source_calls.get(), 0);
}

#[test]
fn malformed_observation_happens_before_map_read_or_source_factory() {
    let bmp = bmp_2x2();
    let policy = policy_for(&bmp, 2, 2);
    let observation = b"{}".to_vec();
    let cmd = command(sha256_hex(&observation));
    let map_reads = Cell::new(0);
    let source_calls = Cell::new(0);

    let result = preflight_local_alignment_with(
        &cmd,
        &policy,
        |_, _| Ok::<_, ()>(observation.clone()),
        |_, _| {
            map_reads.set(map_reads.get() + 1);
            Ok::<_, ()>(bmp.clone())
        },
        |_| source_calls.set(source_calls.get() + 1),
    );

    assert_eq!(
        result,
        Err(LocalAlignmentPreflightError::ObservationInvalid)
    );
    assert_eq!(map_reads.get(), 0);
    assert_eq!(source_calls.get(), 0);
}

#[test]
fn map_hash_and_dimensions_fail_before_source_factory() {
    let bmp = bmp_2x2();
    let policy = policy_for(&bmp, 2, 2);
    let observation = observation_bytes(policy.map_sha256(), 2, 2);
    let cmd = command(sha256_hex(&observation));

    let source_calls = Cell::new(0);
    let result = preflight_local_alignment_with(
        &cmd,
        &policy,
        |_, _| Ok::<_, ()>(observation.clone()),
        |_, maximum| {
            assert_eq!(maximum, 64 * 1024 * 1024);
            Ok::<_, ()>(b"different-map".to_vec())
        },
        |_| source_calls.set(source_calls.get() + 1),
    );
    assert_eq!(result, Err(LocalAlignmentPreflightError::MapIntegrity));
    assert_eq!(source_calls.get(), 0);

    let one_by_one = bmp_1x1();
    let wrong_dimensions_policy =
        LocalAlignmentPreflightPolicy::new_for_test(24_181_527, sha256_hex(&one_by_one), 2, 2)
            .unwrap();
    let wrong_dimensions_observation =
        observation_bytes(wrong_dimensions_policy.map_sha256(), 2, 2);
    let wrong_dimensions_command = command(sha256_hex(&wrong_dimensions_observation));
    let source_calls = Cell::new(0);
    let result = preflight_local_alignment_with(
        &wrong_dimensions_command,
        &wrong_dimensions_policy,
        |_, _| Ok::<_, ()>(wrong_dimensions_observation),
        |_, _| Ok::<_, ()>(one_by_one),
        |_| source_calls.set(source_calls.get() + 1),
    );
    assert_eq!(result, Err(LocalAlignmentPreflightError::MapDimensions));
    assert_eq!(source_calls.get(), 0);
}

#[test]
fn matching_hash_invalid_bmp_fails_as_map_format_before_source_factory() {
    let invalid_map = b"not-a-bmp".to_vec();
    let policy = policy_for(&invalid_map, 2, 2);
    let observation = observation_bytes(policy.map_sha256(), 2, 2);
    let cmd = command(sha256_hex(&observation));
    let source_calls = Cell::new(0);

    let result = preflight_local_alignment_with(
        &cmd,
        &policy,
        |_, _| Ok::<_, ()>(observation),
        |_, _| Ok::<_, ()>(invalid_map),
        |_| source_calls.set(source_calls.get() + 1),
    );

    assert_eq!(result, Err(LocalAlignmentPreflightError::MapFormat));
    assert_eq!(source_calls.get(), 0);
}

#[test]
fn complete_preflight_invokes_source_factory_once_after_bounded_reads() {
    let bmp = bmp_2x2();
    let policy = policy_for(&bmp, 2, 2);
    let observation = observation_bytes(policy.map_sha256(), 2, 2);
    let cmd = command(sha256_hex(&observation));
    let order = std::cell::RefCell::new(Vec::new());
    let source_calls = Cell::new(0);

    let value = preflight_local_alignment_with(
        &cmd,
        &policy,
        |path, maximum| {
            order.borrow_mut().push("observation");
            assert_eq!(path, Path::new("observation.json"));
            assert_eq!(maximum, 64 * 1024);
            Ok::<_, ()>(observation.clone())
        },
        |path, maximum| {
            order.borrow_mut().push("map");
            assert_eq!(path, Path::new("map.bmp"));
            assert_eq!(maximum, 64 * 1024 * 1024);
            Ok::<_, ()>(bmp.clone())
        },
        |preflight| {
            order.borrow_mut().push("source");
            source_calls.set(source_calls.get() + 1);
            assert_eq!(preflight.map().width(), 2);
            assert_eq!(preflight.map().height(), 2);
            42_u32
        },
    )
    .unwrap();

    assert_eq!(value, 42);
    assert_eq!(source_calls.get(), 1);
    assert_eq!(&*order.borrow(), &["observation", "map", "source"]);
}

#[cfg(windows)]
#[test]
fn protected_wrapper_accepts_only_owner_only_observation_and_regular_bounded_map() {
    use std::{
        fs::{self, File},
        os::windows::fs::symlink_file,
    };

    let temp = tempfile::TempDir::new().unwrap();
    let bmp = bmp_2x2();
    let policy = policy_for(&bmp, 2, 2);
    let observation = observation_bytes(policy.map_sha256(), 2, 2);
    let observation_sha = sha256_hex(&observation);
    let observation_path = temp.path().join("observation.json");
    let map_path = temp.path().join("map.bmp");
    fs::write(&observation_path, &observation).unwrap();
    make_owner_only(&observation_path);
    fs::write(&map_path, &bmp).unwrap();
    let command = command_for_paths(&map_path, &observation_path, &observation_sha);
    let source_calls = Cell::new(0);

    let dimensions = preflight_local_alignment_then_with_policy(&command, &policy, |preflight| {
        source_calls.set(source_calls.get() + 1);
        (preflight.map().width(), preflight.map().height())
    })
    .unwrap();
    assert_eq!(dimensions, (2, 2));
    assert_eq!(source_calls.get(), 1);

    let insecure_observation = temp.path().join("insecure-observation.json");
    fs::write(&insecure_observation, &observation).unwrap();
    grant_everyone_read(&insecure_observation);
    assert_preflight_file_error(
        &policy,
        &map_path,
        &insecure_observation,
        &observation_sha,
        LocalAlignmentPreflightError::ObservationFile,
    );

    let observation_directory = temp.path().join("observation-directory");
    fs::create_dir(&observation_directory).unwrap();
    assert_preflight_file_error(
        &policy,
        &map_path,
        &observation_directory,
        &observation_sha,
        LocalAlignmentPreflightError::ObservationFile,
    );

    let oversized_observation = temp.path().join("oversized-observation.json");
    File::create(&oversized_observation)
        .unwrap()
        .set_len(64 * 1_024 + 1)
        .unwrap();
    assert_preflight_file_error(
        &policy,
        &map_path,
        &oversized_observation,
        &observation_sha,
        LocalAlignmentPreflightError::ObservationFile,
    );

    let observation_link = temp.path().join("observation-link.json");
    if symlink_file(&observation_path, &observation_link).is_ok() {
        assert_preflight_file_error(
            &policy,
            &map_path,
            &observation_link,
            &observation_sha,
            LocalAlignmentPreflightError::ObservationFile,
        );
    }

    let map_directory = temp.path().join("map-directory");
    fs::create_dir(&map_directory).unwrap();
    assert_preflight_file_error(
        &policy,
        &map_directory,
        &observation_path,
        &observation_sha,
        LocalAlignmentPreflightError::MapFile,
    );

    let oversized_map = temp.path().join("oversized-map.bmp");
    File::create(&oversized_map)
        .unwrap()
        .set_len(64 * 1_024 * 1_024 + 1)
        .unwrap();
    assert_preflight_file_error(
        &policy,
        &oversized_map,
        &observation_path,
        &observation_sha,
        LocalAlignmentPreflightError::MapFile,
    );

    let map_link = temp.path().join("map-link.bmp");
    if symlink_file(&map_path, &map_link).is_ok() {
        assert_preflight_file_error(
            &policy,
            &map_link,
            &observation_path,
            &observation_sha,
            LocalAlignmentPreflightError::MapFile,
        );
    }
}

fn command(observation_sha256: String) -> LocalAlignmentDiagnosticCommand {
    command_for_paths(
        Path::new("map.bmp"),
        Path::new("observation.json"),
        &observation_sha256,
    )
}

fn command_for_paths(
    map_path: &Path,
    observation_path: &Path,
    observation_sha256: &str,
) -> LocalAlignmentDiagnosticCommand {
    LocalAlignmentDiagnosticCommand::parse([
        OsString::from(MODE),
        OsString::from("--real-map-bmp"),
        map_path.as_os_str().to_owned(),
        OsString::from("--observation-file"),
        observation_path.as_os_str().to_owned(),
        OsString::from("--observation-sha256"),
        OsString::from(observation_sha256),
        OsString::from("--timeout-seconds"),
        OsString::from("7"),
    ])
    .unwrap()
}

fn valid_arguments(sha: &str) -> Vec<OsString> {
    vec![
        OsString::from(MODE),
        OsString::from("--real-map-bmp"),
        OsString::from("map.bmp"),
        OsString::from("--observation-file"),
        OsString::from("observation.json"),
        OsString::from("--observation-sha256"),
        OsString::from(sha),
        OsString::from("--timeout-seconds"),
        OsString::from("7"),
    ]
}

fn remove_argument(arguments: &mut Vec<OsString>, flag: &str, has_value: bool) {
    let index = arguments
        .iter()
        .position(|argument| argument == flag)
        .unwrap();
    arguments.remove(index);
    if has_value {
        arguments.remove(index);
    }
}

#[cfg(windows)]
fn assert_preflight_file_error(
    policy: &LocalAlignmentPreflightPolicy,
    map_path: &Path,
    observation_path: &Path,
    observation_sha256: &str,
    expected: LocalAlignmentPreflightError,
) {
    let source_calls = Cell::new(0);
    let command = command_for_paths(map_path, observation_path, observation_sha256);
    let result = preflight_local_alignment_then_with_policy(&command, policy, |_| {
        source_calls.set(source_calls.get() + 1);
    });
    assert_eq!(result, Err(expected));
    assert_eq!(source_calls.get(), 0);
}

#[cfg(windows)]
fn make_owner_only(path: &Path) {
    use std::process::Command;

    let identity = Command::new("whoami").output().unwrap();
    assert!(identity.status.success());
    let identity = String::from_utf8(identity.stdout).unwrap();
    let owner_status = Command::new("icacls")
        .arg(path)
        .args(["/setowner", identity.trim()])
        .status()
        .unwrap();
    assert!(owner_status.success());
    let grant = format!("{}:(F)", identity.trim());
    let status = Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(grant)
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(windows)]
fn grant_everyone_read(path: &Path) {
    use std::process::Command;

    let status = Command::new("icacls")
        .arg(path)
        .args(["/grant", "*S-1-1-0:(R)"])
        .status()
        .unwrap();
    assert!(status.success());
}

fn policy_for(bytes: &[u8], width: u32, height: u32) -> LocalAlignmentPreflightPolicy {
    LocalAlignmentPreflightPolicy::new_for_test(24_181_527, sha256_hex(bytes), width, height)
        .unwrap()
}

fn observation_bytes(map_sha256: &str, width: u32, height: u32) -> Vec<u8> {
    format!(
        concat!(
            "{{",
            "\"schema\":\"pal_companion.local_alignment_observation.v1\",",
            "\"claim\":\"independent_native_marker_observation_not_gate_b\",",
            "\"game_build_id\":24181527,",
            "\"map_sha256\":\"{map_sha256}\",",
            "\"map_width_px\":{width},",
            "\"map_height_px\":{height},",
            "\"observed_marker_x_px\":0.5,",
            "\"observed_marker_y_px\":0.5,",
            "\"nonce\":\"000102030405060708090a0b0c0d0e0f\"",
            "}}"
        ),
        map_sha256 = map_sha256,
        width = width,
        height = height,
    )
    .into_bytes()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn bmp_2x2() -> Vec<u8> {
    bmp(2, 2, &[0x000000, 0x112233, 0x445566, 0x778899])
}

fn bmp_1x1() -> Vec<u8> {
    bmp(1, 1, &[0x112233])
}

fn bmp(width: u32, height: u32, pixels: &[u32]) -> Vec<u8> {
    let row_stride = (usize::try_from(width).unwrap() * 3 + 3) & !3;
    let pixel_bytes = row_stride * usize::try_from(height).unwrap();
    let mut bytes = vec![0_u8; 54 + pixel_bytes];
    bytes[0..2].copy_from_slice(b"BM");
    let file_size = u32::try_from(bytes.len()).unwrap();
    bytes[2..6].copy_from_slice(&file_size.to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&width.to_le_bytes());
    bytes[22..26].copy_from_slice(&height.to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&u32::try_from(pixel_bytes).unwrap().to_le_bytes());
    for y in 0..usize::try_from(height).unwrap() {
        let source_y = usize::try_from(height).unwrap() - y - 1;
        let row = 54 + y * row_stride;
        for x in 0..usize::try_from(width).unwrap() {
            let color = pixels[source_y * usize::try_from(width).unwrap() + x];
            let offset = row + x * 3;
            bytes[offset] = (color & 0xff) as u8;
            bytes[offset + 1] = ((color >> 8) & 0xff) as u8;
            bytes[offset + 2] = ((color >> 16) & 0xff) as u8;
        }
    }
    bytes
}
