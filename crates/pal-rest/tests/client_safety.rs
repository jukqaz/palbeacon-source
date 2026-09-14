use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use pal_rest::{
    BasicAuthSecret, ClientConfig, EndpointPolicy, PalRestClient, PlayerSelector, Pseudonymizer,
    RestError, SafePlayerObservation, TimedResponse,
};

#[test]
fn basic_auth_secret_is_redacted_in_debug_and_display() {
    let secret = BasicAuthSecret::new("admin", "super-secret-password").unwrap();

    assert_eq!(format!("{secret:?}"), "[REDACTED]");
    assert_eq!(secret.to_string(), "[REDACTED]");
}

#[test]
fn basic_auth_rejects_ambiguous_or_control_character_credentials() {
    for (username, password) in [
        ("admin:other", "password"),
        ("admin", "password\u{0}suffix"),
        ("admin\u{7f}", "password"),
    ] {
        assert!(matches!(
            BasicAuthSecret::new(username, password),
            Err(RestError::InvalidClientConfig)
        ));
    }
    assert!(matches!(
        BasicAuthSecret::new("a".repeat(4_097), "password"),
        Err(RestError::InvalidClientConfig)
    ));
}

#[test]
fn loopback_policy_accepts_literal_loopback_and_rejects_public_or_dns_hosts() {
    let policy = EndpointPolicy::loopback_only();

    assert!(ClientConfig::new("http://127.0.0.1:8212/v1/api", policy.clone()).is_ok());
    assert!(matches!(
        ClientConfig::new("http://8.8.8.8:8212/v1/api", policy.clone()),
        Err(RestError::EndpointNotAllowed)
    ));
    assert!(matches!(
        ClientConfig::new("http://example.com:8212/v1/api", policy),
        Err(RestError::EndpointNotAllowed)
    ));
}

#[test]
fn ipv6_loopback_http_is_parsed_without_losing_url_brackets() {
    let policy = EndpointPolicy::loopback_only();

    assert!(ClientConfig::new("http://[::1]:8212/v1/api", policy.clone()).is_ok());
    assert!(matches!(
        ClientConfig::new("http://[2001:db8::7]:8212/v1/api", policy),
        Err(RestError::EndpointNotAllowed)
    ));
}

#[test]
fn exact_rfc1918_address_requires_explicit_allowlist() {
    let private = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 25));
    let denied = EndpointPolicy::loopback_only();
    assert!(matches!(
        ClientConfig::new("http://192.168.1.25:8212/v1/api", denied),
        Err(RestError::EndpointNotAllowed)
    ));

    let allowed = EndpointPolicy::loopback_only()
        .allow_private_ip(private)
        .unwrap();
    assert!(ClientConfig::new("http://192.168.1.25:8212/v1/api", allowed).is_ok());
}

#[test]
fn public_address_cannot_be_added_to_private_allowlist() {
    let result =
        EndpointPolicy::loopback_only().allow_private_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));

    assert!(matches!(result, Err(RestError::EndpointNotAllowed)));
}

#[test]
fn explicit_insecure_http_accepts_only_a_socket_bound_configuration() {
    for endpoint in [
        "127.0.0.1:8212",
        "192.168.1.25:8212",
        "203.0.113.17:8212",
        "[2001:db8::7]:8212",
    ] {
        let endpoint = endpoint.parse().unwrap();
        let config = ClientConfig::explicit_insecure_http(endpoint).unwrap();
        assert!(
            PalRestClient::new_explicit_insecure_http(
                config,
                BasicAuthSecret::new("admin", "password").unwrap(),
            )
            .is_ok()
        );
    }

    assert!(matches!(
        ClientConfig::explicit_insecure_http("203.0.113.17:0".parse().unwrap()),
        Err(RestError::InvalidEndpoint)
    ));

    for forbidden in [
        "0.0.0.0:8212",
        "[::]:8212",
        "224.0.0.1:8212",
        "[ff02::1]:8212",
        "255.255.255.255:8212",
        "[::ffff:0.0.0.0]:8212",
        "[::ffff:224.0.0.1]:8212",
        "[::ffff:255.255.255.255]:8212",
    ] {
        assert!(
            ClientConfig::explicit_insecure_http(forbidden.parse().unwrap()).is_err(),
            "{forbidden} must not be an authenticated target"
        );
    }
}

#[test]
fn broad_http_policy_cannot_be_upgraded_to_the_explicit_insecure_client() {
    let private = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 25));
    let policy = EndpointPolicy::loopback_only()
        .allow_private_ip(private)
        .unwrap();
    let config = ClientConfig::new("http://192.168.1.25:8212/v1/api", policy).unwrap();

    assert!(matches!(
        PalRestClient::new_explicit_insecure_http(
            config,
            BasicAuthSecret::new("admin", "password").unwrap(),
        ),
        Err(RestError::EndpointNotAllowed)
    ));
}

#[test]
fn remote_https_requires_the_exact_explicit_base_url() {
    let endpoint = "https://pal-rest.example.test:8443/v1/api";
    let denied = EndpointPolicy::loopback_only();
    assert!(matches!(
        ClientConfig::new(endpoint, denied),
        Err(RestError::EndpointNotAllowed)
    ));

    let allowed = EndpointPolicy::loopback_only()
        .allow_exact_remote_https(endpoint)
        .unwrap();
    let config = ClientConfig::new(endpoint, allowed.clone()).unwrap();
    assert!(
        PalRestClient::new_explicit_remote_https(
            config,
            BasicAuthSecret::new("admin", "password").unwrap(),
        )
        .is_ok()
    );

    for other in [
        "https://other.example.test:8443/v1/api",
        "https://pal-rest.example.test:9443/v1/api",
        "http://pal-rest.example.test:8443/v1/api",
    ] {
        assert!(
            ClientConfig::new(other, allowed.clone()).is_err(),
            "{other} must not inherit another endpoint's authorization"
        );
    }
}

#[test]
fn remote_ipv6_https_normalizes_with_exactly_one_bracket_pair() {
    let endpoint = "https://[2001:db8::7]:8443/v1/api";
    let allowed = EndpointPolicy::loopback_only()
        .allow_exact_remote_https(endpoint)
        .unwrap();
    let config = ClientConfig::new(endpoint, allowed.clone()).unwrap();
    assert!(
        PalRestClient::new_explicit_remote_https(
            config,
            BasicAuthSecret::new("admin", "password").unwrap(),
        )
        .is_ok()
    );
    assert!(matches!(
        ClientConfig::new("https://[2001:db8::8]:8443/v1/api", allowed),
        Err(RestError::EndpointNotAllowed)
    ));
}

#[test]
fn explicit_remote_policy_rejects_ambiguous_or_non_tls_origins() {
    for endpoint in [
        "http://pal-rest.example.test:8212/v1/api",
        "https://user:password@pal-rest.example.test/v1/api",
        "https://pal-rest.example.test/v1/api?secret=value",
        "https://pal-rest.example.test/v1/api#fragment",
        "https://pal-rest.example.test/not-the-api",
    ] {
        assert!(
            EndpointPolicy::loopback_only()
                .allow_exact_remote_https(endpoint)
                .is_err(),
            "{endpoint} must be rejected"
        );
    }
}

#[test]
fn endpoint_rejects_credentials_queries_fragments_and_wrong_path() {
    let policy = EndpointPolicy::loopback_only();
    for endpoint in [
        "https://127.0.0.1:8212/v1/api",
        "http://user:password@127.0.0.1:8212/v1/api",
        "http://127.0.0.1:8212/v1/api?secret=value",
        "http://127.0.0.1:8212/v1/api#fragment",
        "http://127.0.0.1:8212/not-the-api",
    ] {
        assert!(
            ClientConfig::new(endpoint, policy.clone()).is_err(),
            "{endpoint} must be rejected"
        );
    }
}

#[test]
fn client_config_has_bounded_defaults_and_validates_overrides() {
    let config = ClientConfig::loopback("http://127.0.0.1:8212/v1/api").unwrap();
    assert_eq!(config.max_decoded_body_bytes(), 32 * 1024 * 1024);
    assert!(config.connect_timeout() <= Duration::from_secs(5));
    assert!(config.request_timeout() <= Duration::from_secs(30));

    assert!(matches!(
        config.clone().with_max_decoded_body_bytes(0),
        Err(RestError::InvalidClientConfig)
    ));
}

#[test]
fn endpoint_policy_and_client_config_debug_do_not_expose_network_addresses() {
    let private = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 25));
    let policy = EndpointPolicy::loopback_only()
        .allow_private_ip(private)
        .unwrap();
    let config = ClientConfig::new("http://192.168.1.25:8212/v1/api", policy.clone()).unwrap();

    for debug in [format!("{policy:?}"), format!("{config:?}")] {
        assert!(!debug.contains("192.168.1.25"));
        assert!(!debug.contains("8212"));
    }

    let endpoint = "https://pal-rest.example.test:8443/v1/api";
    let remote_policy = EndpointPolicy::loopback_only()
        .allow_exact_remote_https(endpoint)
        .unwrap();
    let remote_config = ClientConfig::new(endpoint, remote_policy.clone()).unwrap();
    for debug in [format!("{remote_policy:?}"), format!("{remote_config:?}")] {
        assert!(!debug.contains("pal-rest.example.test"));
        assert!(!debug.contains("8443"));
    }

    let insecure = ClientConfig::explicit_insecure_http("203.0.113.17:8212".parse().unwrap())
        .expect("explicit literal endpoint");
    let debug = format!("{insecure:?}");
    assert!(!debug.contains("203.0.113.17"));
    assert!(!debug.contains("8212"));
}

#[test]
fn client_and_owned_inputs_can_move_to_a_dedicated_async_worker() {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    fn assert_send_static<T: Send + 'static>() {}

    assert_send_sync_static::<PalRestClient>();
    assert_send_sync_static::<PlayerSelector>();
    assert_send_sync_static::<Pseudonymizer>();
    assert_send_static::<SafePlayerObservation>();
    assert_send_static::<TimedResponse<SafePlayerObservation>>();
}
