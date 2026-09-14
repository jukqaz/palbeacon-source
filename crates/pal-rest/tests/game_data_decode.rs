use pal_rest::{PlayerSelector, Pseudonymizer, decode_selected_player, sanitize_player};

const GAME_DATA: &[u8] = include_bytes!("../../../tests/fixtures/rest/game-data-sensitive.json");

#[test]
fn selects_exact_user_id_and_retains_only_navigation_fields() {
    let selector = PlayerSelector::user_id("selected-user").unwrap();
    let selected = decode_selected_player(GAME_DATA, &selector).unwrap();
    let safe = sanitize_player(selected, &Pseudonymizer::new([7_u8; 32]), "world-a").unwrap();

    assert_eq!(safe.x, 12_345.5);
    assert_eq!(safe.y, -67_890.25);
    assert_eq!(safe.z, 321.75);
    assert_eq!(safe.heading_degrees, Some(271.25));
    assert_eq!(safe.server_fps, 59.75);
    assert_eq!(safe.average_server_fps, 57.5);
    assert_eq!(safe.actor_count, 4);
    assert_eq!(safe.subject_id.len(), 64);
}

#[test]
fn selects_exact_instance_id() {
    let selector = PlayerSelector::instance_id("selected-instance").unwrap();
    let selected = decode_selected_player(GAME_DATA, &selector).unwrap();
    let safe = sanitize_player(selected, &Pseudonymizer::new([9_u8; 32]), "world-a").unwrap();

    assert_eq!(safe.x, 12_345.5);
    assert_eq!(safe.heading_degrees, Some(271.25));
}

#[test]
fn safe_serialization_contains_no_sensitive_fixture_sentinel() {
    let selector = PlayerSelector::user_id("selected-user").unwrap();
    let selected = decode_selected_player(GAME_DATA, &selector).unwrap();
    let safe = sanitize_player(selected, &Pseudonymizer::new([7_u8; 32]), "world-a").unwrap();
    let encoded = serde_json::to_vec(&safe).unwrap();

    for sentinel in [
        b"203.0.113.42".as_slice(),
        b"account-secret".as_slice(),
        b"other-player-id".as_slice(),
        b"guild-private".as_slice(),
        b"selected-user".as_slice(),
        b"selected-instance".as_slice(),
        b"selected-private-nickname".as_slice(),
    ] {
        assert!(
            !encoded
                .windows(sentinel.len())
                .any(|window| window == sentinel),
            "safe serialization leaked a sensitive sentinel"
        );
    }
}

#[test]
fn pseudonym_is_deterministic_and_domain_separated() {
    let selected_a = decode_selected_player(
        GAME_DATA,
        &PlayerSelector::user_id("selected-user").unwrap(),
    )
    .unwrap();
    let selected_b = decode_selected_player(
        GAME_DATA,
        &PlayerSelector::user_id("selected-user").unwrap(),
    )
    .unwrap();
    let selected_c = decode_selected_player(
        GAME_DATA,
        &PlayerSelector::instance_id("selected-instance").unwrap(),
    )
    .unwrap();
    let pseudonymizer = Pseudonymizer::new([3_u8; 32]);

    let same_a = sanitize_player(selected_a, &pseudonymizer, "world-a").unwrap();
    let same_b = sanitize_player(selected_b, &pseudonymizer, "world-a").unwrap();
    let different_selector = sanitize_player(selected_c, &pseudonymizer, "world-a").unwrap();

    assert_eq!(same_a.subject_id, same_b.subject_id);
    assert_eq!(
        same_a.subject_id,
        "6b4bac2f5005e6e5cd152bb171e1b12491be33f520e5a7322745993e4e4745dd"
    );
    assert_ne!(same_a.subject_id, different_selector.subject_id);
}

#[test]
fn opaque_pre_sanitized_values_and_key_never_debug_raw_identity() {
    let selector = PlayerSelector::user_id("selected-user").unwrap();
    let selected = decode_selected_player(GAME_DATA, &selector).unwrap();
    let pseudonymizer = Pseudonymizer::new([7_u8; 32]);

    assert_eq!(format!("{selector:?}"), "[REDACTED]");
    assert_eq!(format!("{selected:?}"), "[REDACTED]");
    assert_eq!(format!("{pseudonymizer:?}"), "[REDACTED]");
}
