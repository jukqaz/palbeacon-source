use std::net::SocketAddr;

use pal_agent::{
    AgentConfig, ConfigError, HealthMonitor, ProtectedFileError, RestTransport, SecretBundle,
    SingleInstanceError, SingleInstanceGuard, load_protected_config, parse_config,
    parse_verification_key, read_protected_file,
};
use tempfile::TempDir;

const SENTINEL_PASSWORD: &str = "REST-PASSWORD-SENTINEL";
const SENTINEL_PLAYER: &str = "PLAYER-ID-SENTINEL";
const SENTINEL_IP: &str = "192.168.77.88";

fn config_text(extra: &str) -> String {
    format!(
        r#"
world_alias = "world-a"
rest_base_url = "http://127.0.0.1:8212/v1/api"
listen_address = "127.0.0.1:9443"
profile_path = "C:/secure/gate-a.json"
gate_verification_key_path = "C:/secure/gate-verification.pub"
secrets_path = "C:/secure/rest-secrets.json"
server_certificate_path = "C:/secure/server.crt"
server_private_key_path = "C:/secure/server.key"
client_ca_certificate_path = "C:/secure/client-ca.crt"
expected_gate_profile_sha256 = "{digest}"
coordinate_profile_sha256 = "{coordinate}"
selected_interval_ms = 500

[[client_allowlist]]
certificate_fingerprint_sha256 = "{client_digest}"
identity_uri = "spiffe://palcompanion/client/test"
subject_id = "{subject}"
{extra}
"#,
        digest = "a".repeat(64),
        coordinate = "b".repeat(64),
        client_digest = "c".repeat(64),
        subject = "42".repeat(32),
    )
}

#[test]
fn configuration_requires_at_least_one_bound_mtls_client() {
    let text = config_text("");
    let truncated = text.split("[[client_allowlist]]").next().unwrap();

    assert_eq!(
        parse_config(truncated.as_bytes()).unwrap_err(),
        ConfigError::Invalid
    );
}

#[test]
fn secret_bundle_is_strict_and_redacted() {
    let pseudonymization_key = (32_u8..64)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let valid = format!(
        r#"{{
            "rest_username":"admin",
            "rest_password":"{SENTINEL_PASSWORD}",
            "selector_kind":"user_id",
            "selector_value":"{SENTINEL_PLAYER}",
            "pseudonymization_key_hex":"{key}",
            "expected_player_subject_id":"{subject}"
        }}"#,
        key = pseudonymization_key,
        subject = "42".repeat(32),
    );
    let secret = SecretBundle::parse(valid.as_bytes()).unwrap();
    let debug = format!("{secret:?}");
    assert!(!debug.contains(SENTINEL_PASSWORD));
    assert!(!debug.contains(SENTINEL_PLAYER));

    let invalid = valid.replace(
        "\n        }",
        ",\n\"unknown\":\"REST-PASSWORD-SENTINEL\"\n}",
    );
    let error = SecretBundle::parse(invalid.as_bytes()).unwrap_err();
    let formatted = format!("{error:?} {error}");
    assert_eq!(error, ConfigError::InvalidSecrets);
    assert!(!formatted.contains(SENTINEL_PASSWORD));

    let weak = valid.replace(
        &(32_u8..64)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        &"dd".repeat(32),
    );
    assert_eq!(
        SecretBundle::parse(weak.as_bytes()).unwrap_err(),
        ConfigError::InvalidSecrets
    );
}

#[test]
fn gate_verification_key_is_canonical_and_nonzero() {
    assert_eq!(
        parse_verification_key("00".repeat(32).as_bytes()).unwrap_err(),
        ConfigError::InvalidSecrets
    );
    let varied = (0_u8..32)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(parse_verification_key(varied.as_bytes()).unwrap(), {
        let mut expected = [0_u8; 32];
        for (index, byte) in expected.iter_mut().enumerate() {
            *byte = index as u8;
        }
        expected
    });
}

#[test]
fn strict_config_rejects_unknown_fields_without_echoing_input() {
    let text = config_text(&format!(
        "unexpected = \"{SENTINEL_PASSWORD}-{SENTINEL_PLAYER}-{SENTINEL_IP}\""
    ));

    let error = parse_config(text.as_bytes()).unwrap_err();
    let formatted = format!("{error:?} {error}");

    assert_eq!(error, ConfigError::Invalid);
    assert!(!formatted.contains(SENTINEL_PASSWORD));
    assert!(!formatted.contains(SENTINEL_PLAYER));
    assert!(!formatted.contains(SENTINEL_IP));
}

#[test]
fn remote_rest_requires_an_explicit_exact_https_transport_mode() {
    let remote = config_text("").replace(
        "rest_base_url = \"http://127.0.0.1:8212/v1/api\"",
        "rest_base_url = \"https://streamline.example.invalid/v1/api\"\nrest_transport = \"explicit_remote_https\"",
    );
    let config = parse_config(remote.as_bytes()).unwrap();
    assert_eq!(config.rest_transport(), RestTransport::ExplicitRemoteHttps);

    let implicit_remote = remote.replace("\nrest_transport = \"explicit_remote_https\"", "");
    assert_eq!(
        parse_config(implicit_remote.as_bytes()).unwrap_err(),
        ConfigError::Invalid
    );

    let cleartext_remote = remote.replace("https://", "http://");
    assert_eq!(
        parse_config(cleartext_remote.as_bytes()).unwrap_err(),
        ConfigError::Invalid
    );

    let wrong_path = remote.replace("/v1/api\"", "/not-api\"");
    assert_eq!(
        parse_config(wrong_path.as_bytes()).unwrap_err(),
        ConfigError::Invalid
    );
}

#[test]
fn config_debug_and_health_serialization_contain_no_private_values() {
    let config: AgentConfig = parse_config(config_text("").as_bytes()).unwrap();
    let health = HealthMonitor::default();
    let serialized = serde_json::to_string(&health.snapshot()).unwrap();
    let debug = format!("{config:?}");

    for sentinel in [SENTINEL_PASSWORD, SENTINEL_PLAYER, SENTINEL_IP, "127.0.0.1"] {
        assert!(!serialized.contains(sentinel));
        assert!(!debug.contains(sentinel));
    }
}

#[test]
fn single_instance_lock_is_scoped_to_world_and_subject_and_fails_closed() {
    let first = SingleInstanceGuard::acquire("world-a", &[0x42; 32]).unwrap();

    let duplicate = SingleInstanceGuard::acquire("world-a", &[0x42; 32]);
    assert!(matches!(
        duplicate,
        Err(SingleInstanceError::AlreadyRunning)
    ));
    let other = SingleInstanceGuard::acquire("world-b", &[0x42; 32]).unwrap();

    drop((first, other));
}

#[cfg(windows)]
#[test]
fn single_instance_mutex_is_enforced_across_real_processes() {
    use std::{env, process::Command};

    let world = format!("cross-process-{}", std::process::id());
    let guard = SingleInstanceGuard::acquire(&world, &[0x42; 32]).unwrap();
    let output = Command::new(env::current_exe().unwrap())
        .args(["--exact", "single_instance_child_helper", "--nocapture"])
        .env("PAL_AGENT_MUTEX_CHILD_WORLD", &world)
        .env("PAL_AGENT_MUTEX_CHILD_EXPECT", "already")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    drop(guard);

    let output = Command::new(env::current_exe().unwrap())
        .args(["--exact", "single_instance_child_helper", "--nocapture"])
        .env("PAL_AGENT_MUTEX_CHILD_WORLD", &world)
        .env("PAL_AGENT_MUTEX_CHILD_EXPECT", "acquired")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
fn single_instance_child_helper() {
    let Ok(world) = std::env::var("PAL_AGENT_MUTEX_CHILD_WORLD") else {
        return;
    };
    let expected = std::env::var("PAL_AGENT_MUTEX_CHILD_EXPECT").unwrap();
    let acquired = SingleInstanceGuard::acquire(&world, &[0x42; 32]);
    match expected.as_str() {
        "already" => assert!(matches!(acquired, Err(SingleInstanceError::AlreadyRunning))),
        "acquired" => assert!(acquired.is_ok()),
        _ => panic!("unexpected helper expectation"),
    }
}

#[test]
fn telemetry_contract_has_no_remote_control_rpc() {
    let proto = include_str!("../../../proto/palcompanion/v2/telemetry.proto");
    let service = proto
        .split("service PositionTelemetryService")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    let rpc_names = service
        .lines()
        .filter_map(|line| line.trim().strip_prefix("rpc "))
        .map(|line| line.split('(').next().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(rpc_names, ["GetDescriptor", "Subscribe", "ClockProbe"]);
    for forbidden in ["Stop", "Restart", "Kick", "Ban", "Save", "Command"] {
        assert!(!service.contains(forbidden));
    }
}

#[test]
fn production_agent_surface_has_no_gate_signing_or_private_key_capability() {
    let production = [
        include_str!("../src/lib.rs"),
        include_str!("../src/main.rs"),
        include_str!("../src/config.rs"),
        include_str!("../src/gate_profile.rs"),
    ]
    .join("\n");

    for forbidden in [
        "SigningKey",
        "Ed25519KeyPair",
        "--sign",
        "private_signing_key",
        "attestation_hmac_sha256",
    ] {
        assert!(
            !production.contains(forbidden),
            "production signing capability contained {forbidden}"
        );
    }
}

#[test]
fn production_rest_credentials_require_an_explicit_production_transport() {
    let production = include_str!("../src/main.rs");

    assert!(production.contains("PalRestClient::new_verified("));
    assert!(production.contains("PalRestClient::new_explicit_remote_https("));
    assert!(!production.contains("PalRestClient::new_unverified_for_probe("));
}

#[test]
fn telemetry_listener_is_prebound_before_the_poller_can_start() {
    let production = include_str!("../src/main.rs");
    let bind = production
        .find("TcpListener::bind")
        .expect("production startup must explicitly pre-bind telemetry");
    let poller_start = production
        .find("poller.run(shutdown_rx).await")
        .expect("production startup must run the poller");

    assert!(bind < poller_start);
    assert!(production.contains("serve_with_incoming_shutdown"));
    assert!(!production.contains(".serve_with_shutdown(config.listen_address()"));
}

#[test]
fn command_line_accepts_only_a_config_path() {
    let parsed = pal_agent::parse_command([
        "pal-agent",
        "--config",
        "C:/ProgramData/PalCompanion/agent.toml",
    ])
    .unwrap();
    assert!(parsed.config_path.ends_with("agent.toml"));

    for forbidden in [
        vec!["pal-agent", "--password", SENTINEL_PASSWORD],
        vec!["pal-agent", "--player-id", SENTINEL_PLAYER],
        vec!["pal-agent", "--listen", SENTINEL_IP],
    ] {
        assert!(pal_agent::parse_command(forbidden).is_err());
    }
}

#[test]
fn listen_address_must_be_literal_loopback_or_private_ip() {
    let mut public = config_text("");
    public = public.replace("127.0.0.1:9443", "8.8.8.8:9443");
    assert!(parse_config(public.as_bytes()).is_err());

    let config = parse_config(config_text("").as_bytes()).unwrap();
    assert_eq!(
        config.listen_address(),
        "127.0.0.1:9443".parse::<SocketAddr>().unwrap()
    );
}

#[cfg(windows)]
#[test]
fn protected_file_reader_rejects_acl_access_for_everyone() {
    use std::{fs, process::Command};

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("secret.json");
    fs::write(&path, SENTINEL_PASSWORD).unwrap();
    let status = Command::new("icacls")
        .arg(&path)
        .args(["/grant", "*S-1-1-0:(R)"])
        .status()
        .unwrap();
    assert!(status.success());

    let error = read_protected_file(&path, 1024).unwrap_err();
    let formatted = format!("{error:?} {error}");
    assert_eq!(error, ProtectedFileError::InsecurePermissions);
    assert!(!formatted.contains(SENTINEL_PASSWORD));
}

#[cfg(windows)]
#[test]
fn protected_file_reader_accepts_an_owner_only_acl() {
    use std::{fs, process::Command};

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("secret.json");
    fs::write(&path, b"synthetic-secret").unwrap();
    let identity = Command::new("whoami").output().unwrap();
    assert!(identity.status.success());
    let identity = String::from_utf8(identity.stdout).unwrap();
    let owner_status = Command::new("icacls")
        .arg(&path)
        .args(["/setowner", identity.trim()])
        .status()
        .unwrap();
    assert!(owner_status.success());
    let grant = format!("{}:(F)", identity.trim());
    let status = Command::new("icacls")
        .arg(&path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(grant)
        .status()
        .unwrap();
    assert!(status.success());

    assert_eq!(
        read_protected_file(&path, 1024).unwrap(),
        b"synthetic-secret"
    );
}

#[cfg(windows)]
#[test]
fn protected_config_is_mandatory() {
    use std::{fs, process::Command};

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("agent.toml");
    fs::write(&path, config_text("")).unwrap();
    let status = Command::new("icacls")
        .arg(&path)
        .args(["/grant", "*S-1-1-0:(R)"])
        .status()
        .unwrap();
    assert!(status.success());

    assert_eq!(
        load_protected_config(&path).unwrap_err(),
        ProtectedFileError::InsecurePermissions
    );
}

#[cfg(windows)]
#[test]
fn protected_file_reader_rejects_builtin_users_allow_ace() {
    use std::{fs, process::Command};

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("secret.json");
    fs::write(&path, b"synthetic-secret").unwrap();
    let status = Command::new("icacls")
        .arg(&path)
        .args(["/grant", "*S-1-5-32-545:(R)"])
        .status()
        .unwrap();
    assert!(status.success());

    assert_eq!(
        read_protected_file(&path, 1024).unwrap_err(),
        ProtectedFileError::InsecurePermissions
    );
}

#[cfg(windows)]
#[test]
fn protected_file_reader_rejects_foreign_owner() {
    let path = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap()).join("win.ini");
    assert!(path.is_file());

    assert_eq!(
        read_protected_file(&path, 1024).unwrap_err(),
        ProtectedFileError::InsecurePermissions
    );
}

#[cfg(windows)]
#[test]
fn protected_file_reader_rejects_final_symlink_and_parent_junction() {
    use std::{fs, os::windows::fs::symlink_file, process::Command};

    let temp = TempDir::new().unwrap();
    let target_dir = temp.path().join("target");
    fs::create_dir(&target_dir).unwrap();
    let target = target_dir.join("secret.json");
    fs::write(&target, b"synthetic-secret").unwrap();

    let final_link = temp.path().join("final-link.json");
    if symlink_file(&target, &final_link).is_ok() {
        assert_eq!(
            read_protected_file(&final_link, 1024).unwrap_err(),
            ProtectedFileError::Invalid
        );
    }

    let junction = temp.path().join("junction");
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&target_dir)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        read_protected_file(&junction.join("secret.json"), 1024).unwrap_err(),
        ProtectedFileError::Invalid
    );
}
