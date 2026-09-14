#![cfg(windows)]

use std::{
    fs::File,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pal_agent::{
    AttestedGateProfile, GateProfilePayload, StartupGateConfig, canonical_profile_sha256,
    inspect_live_server,
};
use pal_protocol::v2::{
    GetDescriptorRequest, SubscribeRequest,
    position_telemetry_service_client::PositionTelemetryServiceClient,
};
use pal_rest::{BasicAuthSecret, ClientConfig, PalRestClient, PlayerSelector, Pseudonymizer};
use pal_rest_probe::{
    CandidateLoadReport, CardinalDirection, DistributionSummary, GateAEvidence, LoadErrorCounts,
    MovementEvidence, MovementObservation, PairOrder, PairedLoadWindow, PreflightEvidence,
    PrivacyEvidence, ProbeRunner, RotationEvidence, RotationObservation, ServerFingerprintInput,
    WindowMetrics, evaluate,
};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, SanType, string::Ia5String,
};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use tempfile::TempDir;
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint, Identity};

const INFO: &[u8] = include_bytes!("../../../tests/fixtures/rest/info.json");
const GAME_DATA: &[u8] = include_bytes!("../../../tests/fixtures/rest/game-data-sensitive.json");
const COORDINATE_PROFILE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CLIENT_URI: &str = "spiffe://palcompanion/core/production-rig";
const SIGNING_SEED: [u8; 32] = [0x91; 32];

struct RestServer {
    base_url: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RestServer {
    fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        respond(&mut stream);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            base_url: format!("http://{address}/v1/api"),
            stop,
            thread: Some(thread),
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for RestServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn respond(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = Vec::new();
    let mut buffer = [0_u8; 2_048];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let Ok(count) = stream.read(&mut buffer) else {
            return;
        };
        if count == 0 {
            return;
        }
        request.extend_from_slice(&buffer[..count]);
        if request.len() > 16 * 1024 {
            return;
        }
    }
    let body = if request.starts_with(b"GET /v1/api/info ") {
        INFO
    } else if request.starts_with(b"GET /v1/api/game-data ") {
        GAME_DATA
    } else {
        b"{}"
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

struct Cert {
    cert_pem: String,
    cert_der: Vec<u8>,
    key_pem: String,
}

struct Pki {
    ca_pem: String,
    server: Cert,
    client: Cert,
}

fn issue(issuer: &CertifiedIssuer<'_, KeyPair>, dns: Option<&str>, uri: Option<&str>) -> Cert {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "production-path-test-only");
    params.extended_key_usages = if dns.is_some() {
        vec![ExtendedKeyUsagePurpose::ServerAuth]
    } else {
        vec![ExtendedKeyUsagePurpose::ClientAuth]
    };
    if let Some(dns) = dns {
        params
            .subject_alt_names
            .push(SanType::DnsName(Ia5String::try_from(dns).unwrap()));
    }
    if let Some(uri) = uri {
        params
            .subject_alt_names
            .push(SanType::URI(Ia5String::try_from(uri).unwrap()));
    }
    let cert = params.signed_by(&key, issuer).unwrap();
    Cert {
        cert_pem: cert.pem(),
        cert_der: cert.der().to_vec(),
        key_pem: key.serialize_pem(),
    }
}

fn pki() -> Pki {
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
    Pki {
        ca_pem: ca.pem(),
        server: issue(&ca, Some("localhost"), None),
        client: issue(&ca, None, Some(CLIENT_URI)),
    }
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

fn report(
    rest_version: &str,
    executable_sha256: [u8; 32],
    server_subject_id: [u8; 32],
) -> pal_rest_probe::GateAReport {
    let fingerprint = ProbeRunner::preflight(&ServerFingerprintInput {
        rest_version: rest_version.to_owned(),
        steam_manifest_id: Some(123),
        executable_sha256,
        server_subject_id,
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
    evaluate(&GateAEvidence {
        schema_version: 1,
        preflight: PreflightEvidence::complete(fingerprint),
        candidates: vec![candidate(2_000), candidate(1_000), candidate(500)],
        rotation,
        movement,
        privacy: PrivacyEvidence {
            artifact_scan_clean: true,
        },
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_protected(path: &std::path::Path, bytes: impl AsRef<[u8]>) {
    std::fs::write(path, bytes).unwrap();
    let identity = Command::new("whoami").output().unwrap();
    assert!(identity.status.success());
    let identity = String::from_utf8(identity.stdout).unwrap();
    let owner = Command::new("icacls")
        .arg(path)
        .args(["/setowner", identity.trim()])
        .output()
        .unwrap();
    assert!(
        owner.status.success(),
        "{}{}",
        String::from_utf8_lossy(&owner.stdout),
        String::from_utf8_lossy(&owner.stderr)
    );
    let grant = format!("{}:(F)", identity.trim());
    let output = Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(grant)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path_text(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

struct ChildGuard {
    child: Child,
    error_log_path: PathBuf,
}

impl ChildGuard {
    fn failure_output(&self) -> String {
        std::fs::read_to_string(&self.error_log_path)
            .unwrap_or_else(|error| format!("unable to read child error log: {error}"))
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn connect(
    address: SocketAddr,
    pki: &Pki,
    child: &mut ChildGuard,
) -> PositionTelemetryServiceClient<tonic::transport::Channel> {
    for _ in 0..100 {
        if let Some(status) = child.child.try_wait().unwrap() {
            panic!(
                "production pal-agent exited before mTLS connect: {status}\n{}",
                child.failure_output()
            );
        }
        let tls = ClientTlsConfig::new()
            .domain_name("localhost")
            .ca_certificate(Certificate::from_pem(&pki.ca_pem))
            .identity(Identity::from_pem(
                &pki.client.cert_pem,
                &pki.client.key_pem,
            ));
        let endpoint = Endpoint::from_shared(format!("https://{address}"))
            .unwrap()
            .tls_config(tls)
            .unwrap();
        if let Ok(channel) = endpoint.connect().await {
            return PositionTelemetryServiceClient::new(channel);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("production pal-agent never accepted mTLS");
}

#[tokio::test]
async fn actual_startup_composition_streams_selected_rest_position_over_prebound_mtls() {
    let temp = TempDir::new().unwrap();
    let mut rest = RestServer::start();
    let world_alias = format!("production-rig-{}", std::process::id());
    let pseudonymization_key = std::array::from_fn::<_, 32, _>(|index| index as u8 + 32);
    let pseudonymizer = Pseudonymizer::new(pseudonymization_key);
    let rest_client = PalRestClient::new_unverified_for_probe(
        ClientConfig::loopback(&rest.base_url).unwrap(),
        BasicAuthSecret::new("admin", "test-password").unwrap(),
    )
    .unwrap();
    let info = rest_client
        .info(&pseudonymizer, &world_alias)
        .await
        .unwrap()
        .value;
    let selected = rest_client
        .game_data(
            &PlayerSelector::user_id("selected-user").unwrap(),
            &pseudonymizer,
            &world_alias,
        )
        .await
        .unwrap()
        .value;
    let server_subject: [u8; 32] = decode_sha256(&info.server_subject_id);
    let player_subject: [u8; 32] = decode_sha256(&selected.subject_id);
    let live = inspect_live_server(&rest.base_url, std::process::id()).unwrap();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let payload = GateProfilePayload {
        schema_version: 1,
        authority: "local_host_adapter_v1".to_owned(),
        issued_at_unix_ms: now_ms - 1_000,
        expires_at_unix_ms: now_ms + 60_000,
        process_id: live.process_id(),
        process_creation_time_100ns: live.creation_time_100ns(),
        rest_server_version: info.version.clone(),
        steam_manifest_id: Some(123),
        executable_sha256: hex(&live.executable_sha256()),
        server_subject_id: info.server_subject_id.clone(),
        rotation_z_validated: true,
        report: report(&info.version, live.executable_sha256(), server_subject),
    };
    let signing = Ed25519KeyPair::from_seed_unchecked(&SIGNING_SEED).unwrap();
    let canonical = serde_json::to_vec(&payload).unwrap();
    let profile = AttestedGateProfile {
        payload,
        signature_ed25519: hex(signing.sign(&canonical).as_ref()),
    };
    let gate_config = StartupGateConfig {
        expected_profile_sha256: canonical_profile_sha256(&profile.payload).unwrap(),
        coordinate_profile_sha256: COORDINATE_PROFILE.to_owned(),
        selected_interval_ms: 500,
    };
    let pki = pki();

    let profile_path = temp.path().join("gate-profile.json");
    let verification_key_path = temp.path().join("gate-verification.pub");
    let secrets_path = temp.path().join("rest-secrets.json");
    let server_certificate_path = temp.path().join("server.crt");
    let server_private_key_path = temp.path().join("server.key");
    let client_ca_path = temp.path().join("client-ca.crt");
    write_protected(&profile_path, serde_json::to_vec(&profile).unwrap());
    write_protected(&verification_key_path, hex(signing.public_key().as_ref()));
    write_protected(
        &secrets_path,
        serde_json::to_vec(&serde_json::json!({
            "rest_username": "admin",
            "rest_password": "test-password",
            "selector_kind": "user_id",
            "selector_value": "selected-user",
            "pseudonymization_key_hex": hex(&pseudonymization_key),
            "expected_player_subject_id": hex(&player_subject),
        }))
        .unwrap(),
    );
    write_protected(&server_certificate_path, &pki.server.cert_pem);
    write_protected(&server_private_key_path, &pki.server.key_pem);
    write_protected(&client_ca_path, &pki.ca_pem);

    let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let telemetry_address = reservation.local_addr().unwrap();
    drop(reservation);
    let config_path = temp.path().join("agent.toml");
    let config = format!(
        r#"
world_alias = "{world_alias}"
rest_base_url = "{rest_base_url}"
listen_address = "{telemetry_address}"
profile_path = "{profile_path}"
gate_verification_key_path = "{verification_key_path}"
secrets_path = "{secrets_path}"
server_certificate_path = "{server_certificate_path}"
server_private_key_path = "{server_private_key_path}"
client_ca_certificate_path = "{client_ca_path}"
expected_gate_profile_sha256 = "{gate_hash}"
coordinate_profile_sha256 = "{coordinate_profile}"
selected_interval_ms = 500

[[client_allowlist]]
certificate_fingerprint_sha256 = "{client_fingerprint}"
identity_uri = "{client_uri}"
subject_id = "{player_subject}"
"#,
        rest_base_url = rest.base_url,
        profile_path = path_text(&profile_path),
        verification_key_path = path_text(&verification_key_path),
        secrets_path = path_text(&secrets_path),
        server_certificate_path = path_text(&server_certificate_path),
        server_private_key_path = path_text(&server_private_key_path),
        client_ca_path = path_text(&client_ca_path),
        gate_hash = gate_config.expected_profile_sha256,
        coordinate_profile = gate_config.coordinate_profile_sha256,
        client_fingerprint = pal_telemetry::certificate_sha256(&pki.client.cert_der),
        client_uri = CLIENT_URI,
        player_subject = hex(&player_subject),
    );
    write_protected(&config_path, config);

    let error_log_path = temp.path().join("pal-agent.stderr.log");
    let error_log = File::create(&error_log_path).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_pal-agent"))
        .args(["--config"])
        .arg(&config_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(error_log))
        .spawn()
        .unwrap();
    let mut child = ChildGuard {
        child,
        error_log_path,
    };
    let mut client = connect(telemetry_address, &pki, &mut child).await;
    let descriptor = client
        .get_descriptor(GetDescriptorRequest {
            protocol_version: pal_domain::PROTOCOL_VERSION,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(descriptor.world_alias, world_alias);
    assert_eq!(
        descriptor.gate_profile_sha256,
        gate_config.expected_profile_sha256
    );

    let mut stream = client
        .subscribe(SubscribeRequest {
            protocol_version: pal_domain::PROTOCOL_VERSION,
            world_alias: world_alias.clone(),
            subject_id: player_subject.to_vec(),
            resume: None,
        })
        .await
        .unwrap()
        .into_inner();
    let envelope = tokio::time::timeout(Duration::from_secs(3), stream.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(envelope.world_alias, world_alias);
    assert_eq!(envelope.subject_id, player_subject);
    assert_eq!(envelope.position_x, 12_345.5);
    assert_eq!(envelope.position_y, -67_890.25);
    assert_eq!(envelope.position_z, 321.75);
    assert!(envelope.sequence > 0);

    // The poller starts before the client subscribes, so a slower runner can
    // legitimately deliver sequence 2 (or later) as the first latest value.
    // Verify the actual stream contract instead: subsequent values advance.
    let next_envelope = tokio::time::timeout(Duration::from_secs(3), stream.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(next_envelope.boot_id, envelope.boot_id);
    assert!(next_envelope.sequence > envelope.sequence);

    rest.stop();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "pal-agent did not terminate after its bound REST listener disappeared"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(
        !status.success(),
        "trust loss must terminate the production Agent as a refusal"
    );
}

fn decode_sha256(value: &str) -> [u8; 32] {
    assert_eq!(value.len(), 64);
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (nibble(pair[0]) << 4) | nibble(pair[1]);
    }
    output
}

fn nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("non-canonical hex"),
    }
}
