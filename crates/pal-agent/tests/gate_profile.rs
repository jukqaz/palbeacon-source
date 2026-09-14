#![cfg(windows)]

use std::{
    net::TcpListener,
    process,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pal_agent::{
    AttestedGateProfile, GateProfileError, GateProfilePayload, StartupGateConfig,
    bind_preverified_gate, build_agent_descriptor, canonical_profile_sha256, enforce_startup_gate,
    finalize_remote_startup_gate, finalize_startup_gate, inspect_live_server, load_gate_profile,
    preverify_startup_gate,
};
use pal_rest::SanitizedServerInfo;
use pal_rest_probe::{
    CandidateLoadReport, CardinalDirection, DistributionSummary, GateAEvidence, LoadErrorCounts,
    MovementEvidence, MovementObservation, PairOrder, PairedLoadWindow, PreflightEvidence,
    PrivacyEvidence, ProbeRunner, RotationEvidence, RotationObservation, ServerFingerprintInput,
    WindowMetrics, evaluate,
};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::Value;

const SIGNING_SEED: [u8; 32] = [0x91; 32];
const SUBJECT: [u8; 32] = [0x42; 32];
const COORDINATE_PROFILE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn signing_key() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&SIGNING_SEED).unwrap()
}

fn verification_key() -> [u8; 32] {
    signing_key().public_key().as_ref().try_into().unwrap()
}

fn signature(message: &[u8]) -> String {
    hex(signing_key().sign(message).as_ref())
}

fn summary(value: f64) -> DistributionSummary {
    DistributionSummary::from_constant(value, 30).unwrap()
}

fn window(fps: f64) -> WindowMetrics {
    WindowMetrics {
        duration_ms: 300_000,
        request_attempts: 30,
        request_successes: 30,
        selected_player_samples: 30,
        response_latency_ms: summary(20.0),
        decode_latency_ms: summary(2.0),
        decoded_body_bytes: summary(64_000.0),
        actor_count: summary(100.0),
        server_fps: summary(fps),
        frame_time_ms: summary(16.0),
        probe_cpu_percent: summary(0.4),
        probe_private_bytes_peak: 24 * 1024 * 1024,
    }
}

fn candidate(interval_ms: u64) -> CandidateLoadReport {
    CandidateLoadReport {
        interval_ms,
        pairs: (0..5)
            .map(|index| PairedLoadWindow {
                pair_index: index,
                order: if index % 2 == 0 {
                    PairOrder::BaselineThenCandidate
                } else {
                    PairOrder::CandidateThenBaseline
                },
                baseline: window(60.0),
                candidate: window(59.8),
            })
            .collect(),
        errors: LoadErrorCounts::default(),
        safety_abort: None,
    }
}

fn profile_fixture() -> (
    TcpListener,
    StartupGateConfig,
    AttestedGateProfile,
    SanitizedServerInfo,
    pal_agent::LiveServerIdentity,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let rest_base_url = format!("http://{}/v1/api", listener.local_addr().unwrap());
    let live_server = inspect_live_server(&rest_base_url, process::id()).unwrap();
    let executable_hash = live_server.executable_sha256();
    let fingerprint = ProbeRunner::preflight(&ServerFingerprintInput {
        rest_version: "v-test".to_owned(),
        steam_manifest_id: Some(123),
        executable_sha256: executable_hash,
        server_subject_id: SUBJECT,
        executable_hash_verified: true,
        endpoint_private_lan: true,
        auth_ok: true,
        info_ok: true,
        privacy_boundary_ok: true,
    })
    .unwrap()
    .server_fingerprint;
    let rotation = RotationEvidence::from_observations((0..100).map(|index| {
        let expected = CardinalDirection::ALL[index % 4];
        RotationObservation {
            expected,
            observed_degrees: Some(expected.degrees()),
        }
    }))
    .unwrap();
    let movement = MovementEvidence::from_observations((0..5).map(|_| MovementObservation {
        changed_after_ms: Some(900.0),
    }))
    .unwrap();
    let report = evaluate(&GateAEvidence {
        schema_version: 1,
        preflight: PreflightEvidence::complete(fingerprint),
        candidates: vec![candidate(2_000), candidate(1_000), candidate(500)],
        rotation,
        movement,
        privacy: PrivacyEvidence {
            artifact_scan_clean: true,
        },
    });
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let payload = GateProfilePayload {
        schema_version: 1,
        authority: "local_host_adapter_v1".to_owned(),
        issued_at_unix_ms: now_ms - 1_000,
        expires_at_unix_ms: now_ms + 60_000,
        process_id: live_server.process_id(),
        process_creation_time_100ns: live_server.creation_time_100ns(),
        rest_server_version: "v-test".to_owned(),
        steam_manifest_id: Some(123),
        executable_sha256: hex(&executable_hash),
        server_subject_id: hex(&SUBJECT),
        rotation_z_validated: true,
        report,
    };
    let canonical = serde_json::to_vec(&payload).unwrap();
    let profile = AttestedGateProfile {
        payload,
        signature_ed25519: signature(&canonical),
    };
    let config = StartupGateConfig {
        expected_profile_sha256: canonical_profile_sha256(&profile.payload).unwrap(),
        coordinate_profile_sha256: COORDINATE_PROFILE.to_owned(),
        selected_interval_ms: 500,
    };
    let info = SanitizedServerInfo {
        version: "v-test".to_owned(),
        server_subject_id: hex(&SUBJECT),
    };
    (listener, config, profile, info, live_server)
}

fn reattest(config: &mut StartupGateConfig, profile: &mut AttestedGateProfile) {
    let canonical = serde_json::to_vec(&profile.payload).unwrap();
    profile.signature_ed25519 = signature(&canonical);
    config.expected_profile_sha256 = canonical_profile_sha256(&profile.payload).unwrap();
}

fn enforce(
    config: &StartupGateConfig,
    profile: &AttestedGateProfile,
    info: &SanitizedServerInfo,
    live_server: &pal_agent::LiveServerIdentity,
) -> Result<pal_agent::ApprovedGate, GateProfileError> {
    enforce_startup_gate(
        config,
        profile,
        &verification_key(),
        info,
        live_server,
        SystemTime::now(),
    )
}

#[test]
fn canonical_attested_current_profile_allows_startup() {
    let (_listener, config, profile, info, live_server) = profile_fixture();

    let approved = enforce_startup_gate(
        &config,
        &profile,
        &verification_key(),
        &info,
        &live_server,
        SystemTime::now(),
    )
    .unwrap();

    assert_eq!(approved.selected_interval, Duration::from_millis(500));
    assert_eq!(approved.coordinate_profile_sha256, COORDINATE_PROFILE);
    assert!(approved.rotation_z_validated);

    let descriptor = build_agent_descriptor("world-a", &approved).unwrap();
    assert_eq!(descriptor.world_alias, "world-a");
    assert_eq!(descriptor.rest_server_version, "v-test");
    assert_eq!(descriptor.server_fingerprint, approved.server_fingerprint);
    assert_eq!(descriptor.gate_profile_sha256, approved.gate_profile_sha256);
    assert_eq!(descriptor.coordinate_profile_sha256, COORDINATE_PROFILE);
    assert_eq!(descriptor.selected_interval_ms, 500);
    pal_telemetry::validate_descriptor(&descriptor, COORDINATE_PROFILE).unwrap();
}

#[test]
fn startup_gate_can_be_verified_and_bound_before_authenticated_rest() {
    let (_listener, config, profile, info, live_server) = profile_fixture();

    let preverified =
        preverify_startup_gate(&config, &profile, &verification_key(), SystemTime::now()).unwrap();
    assert_eq!(preverified.process_id(), profile.payload.process_id);
    bind_preverified_gate(&preverified, &live_server).unwrap();
    let approved =
        finalize_startup_gate(&preverified, &info, &live_server, SystemTime::now()).unwrap();

    assert_eq!(approved.selected_interval, Duration::from_millis(500));
    assert_eq!(approved.coordinate_profile_sha256, COORDINATE_PROFILE);
}

#[test]
fn remote_https_gate_binds_signed_evidence_to_current_info() {
    let (_listener, config, profile, info, _live_server) = profile_fixture();
    let preverified =
        preverify_startup_gate(&config, &profile, &verification_key(), SystemTime::now()).unwrap();

    let approved = finalize_remote_startup_gate(&preverified, &info, SystemTime::now()).unwrap();
    assert_eq!(approved.rest_server_version, info.version);
    assert_eq!(hex(&approved.server_subject_id), info.server_subject_id);

    let mut wrong_world = info.clone();
    wrong_world.server_subject_id = "33".repeat(32);
    assert_eq!(
        finalize_remote_startup_gate(&preverified, &wrong_world, SystemTime::now()).unwrap_err(),
        GateProfileError::ServerSubjectMismatch
    );

    let mut wrong_version = info;
    wrong_version.version = "other".to_owned();
    assert_eq!(
        finalize_remote_startup_gate(&preverified, &wrong_version, SystemTime::now()).unwrap_err(),
        GateProfileError::RestServerVersionMismatch
    );
}

#[test]
fn profile_that_expires_during_startup_is_refused_at_final_approval() {
    let (_listener, config, profile, info, live_server) = profile_fixture();
    let before_expiry = UNIX_EPOCH + Duration::from_millis(profile.payload.expires_at_unix_ms - 1);
    let at_expiry = UNIX_EPOCH + Duration::from_millis(profile.payload.expires_at_unix_ms);
    let preverified =
        preverify_startup_gate(&config, &profile, &verification_key(), before_expiry).unwrap();

    assert_eq!(
        finalize_startup_gate(&preverified, &info, &live_server, at_expiry).unwrap_err(),
        GateProfileError::Expired
    );
}

#[test]
fn invalid_signature_is_refused_by_the_pre_rest_gate() {
    let (_listener, config, mut profile, _info, _live_server) = profile_fixture();
    profile.signature_ed25519 = "00".repeat(64);

    assert_eq!(
        preverify_startup_gate(&config, &profile, &verification_key(), SystemTime::now())
            .unwrap_err(),
        GateProfileError::Unattested
    );
}

#[test]
fn missing_or_unattested_profile_refuses_startup() {
    let (_listener, config, mut profile, info, live_server) = profile_fixture();
    assert_eq!(
        load_gate_profile(std::path::Path::new("missing-gate-profile.json")).unwrap_err(),
        GateProfileError::Missing
    );
    profile.signature_ed25519 = "00".repeat(64);

    let error = enforce_startup_gate(
        &config,
        &profile,
        &verification_key(),
        &info,
        &live_server,
        SystemTime::now(),
    )
    .unwrap_err();

    assert_eq!(error, GateProfileError::Unattested);
}

#[test]
fn wrong_public_key_and_payload_mutation_refuse_startup() {
    let (_listener, config, profile, info, live_server) = profile_fixture();
    let wrong_key = Ed25519KeyPair::from_seed_unchecked(&[0x37; 32]).unwrap();
    let wrong_public: [u8; 32] = wrong_key.public_key().as_ref().try_into().unwrap();
    assert_eq!(
        enforce_startup_gate(
            &config,
            &profile,
            &wrong_public,
            &info,
            &live_server,
            SystemTime::now()
        )
        .unwrap_err(),
        GateProfileError::Unattested
    );

    let mut mutated = profile.clone();
    mutated.payload.rest_server_version = "mutated-after-signing".to_owned();
    assert_eq!(
        enforce(&config, &mutated, &info, &live_server).unwrap_err(),
        GateProfileError::Unattested
    );
}

#[test]
fn legacy_hmac_profile_schema_is_rejected() {
    let (_listener, _config, profile, _info, _live_server) = profile_fixture();
    let mut value = serde_json::to_value(profile).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("signature_ed25519");
    object.insert(
        "attestation_hmac_sha256".to_owned(),
        Value::String("91".repeat(32)),
    );

    assert_eq!(
        pal_agent::parse_gate_profile(&serde_json::to_vec(&value).unwrap()).unwrap_err(),
        GateProfileError::InvalidProfile
    );
}

#[test]
fn noncanonical_profile_refuses_startup_with_typed_error() {
    let (_listener, mut config, mut profile, info, live_server) = profile_fixture();
    profile.payload.report.candidates.swap(0, 1);
    reattest(&mut config, &mut profile);

    assert_eq!(
        enforce(&config, &profile, &info, &live_server).unwrap_err(),
        GateProfileError::NonCanonical
    );
}

#[test]
fn expired_profile_refuses_startup_with_typed_error() {
    let (_listener, mut config, mut profile, info, live_server) = profile_fixture();
    profile.payload.issued_at_unix_ms = 1;
    profile.payload.expires_at_unix_ms = 2;
    reattest(&mut config, &mut profile);

    assert_eq!(
        enforce(&config, &profile, &info, &live_server).unwrap_err(),
        GateProfileError::Expired
    );
}

#[test]
fn wrong_process_executable_version_subject_fingerprint_interval_and_coordinate_refuse_startup() {
    let (_listener, mut config, mut profile, info, live_server) = profile_fixture();
    profile.payload.process_id = profile.payload.process_id.wrapping_add(1);
    reattest(&mut config, &mut profile);
    assert_eq!(
        enforce(&config, &profile, &info, &live_server).unwrap_err(),
        GateProfileError::ProcessIdentityMismatch
    );

    let (_listener, mut config, mut profile, info, live_server) = profile_fixture();
    profile.payload.process_creation_time_100ns += 1;
    reattest(&mut config, &mut profile);
    assert_eq!(
        enforce(&config, &profile, &info, &live_server).unwrap_err(),
        GateProfileError::ProcessCreationMismatch
    );

    let (_listener, mut config, mut profile, info, live_server) = profile_fixture();
    profile.payload.executable_sha256 = "37".repeat(32);
    reattest(&mut config, &mut profile);
    assert_eq!(
        enforce(&config, &profile, &info, &live_server).unwrap_err(),
        GateProfileError::ExecutableMismatch
    );

    let (_listener, config, profile, mut info, live_server) = profile_fixture();
    info.version = "other".to_owned();
    assert_eq!(
        enforce(&config, &profile, &info, &live_server).unwrap_err(),
        GateProfileError::RestServerVersionMismatch
    );

    let (_listener, config, profile, mut info, live_server) = profile_fixture();
    info.server_subject_id = "33".repeat(32);
    assert_eq!(
        enforce(&config, &profile, &info, &live_server).unwrap_err(),
        GateProfileError::ServerSubjectMismatch
    );

    let (_listener, mut config, mut profile, info, live_server) = profile_fixture();
    profile.payload.steam_manifest_id = Some(999);
    reattest(&mut config, &mut profile);
    assert_eq!(
        enforce(&config, &profile, &info, &live_server).unwrap_err(),
        GateProfileError::FingerprintMismatch
    );

    let (_listener, mut config, profile, info, live_server) = profile_fixture();
    config.selected_interval_ms = 1_000;
    assert_eq!(
        enforce(&config, &profile, &info, &live_server).unwrap_err(),
        GateProfileError::IntervalMismatch
    );

    let (_listener, mut config, profile, info, live_server) = profile_fixture();
    config.coordinate_profile_sha256 = "A".repeat(64);
    assert_eq!(
        enforce(&config, &profile, &info, &live_server).unwrap_err(),
        GateProfileError::CoordinateProfileMismatch
    );
}

#[test]
fn profile_json_is_strict_and_bounded() {
    let (_listener, _config, profile, _info, _live_server) = profile_fixture();
    let mut value = serde_json::to_value(profile).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("unexpected".to_owned(), Value::Bool(true));
    let bytes = serde_json::to_vec(&value).unwrap();

    assert!(pal_agent::parse_gate_profile(&bytes).is_err());
    assert!(pal_agent::parse_gate_profile(&vec![b' '; 1_048_577]).is_err());
}
