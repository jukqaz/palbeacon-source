use pal_rest::{PlayerSelector, RestError, decode_selected_player};
use serde_json::{Value, json};

const GAME_DATA: &[u8] = include_bytes!("../../../tests/fixtures/rest/game-data-sensitive.json");

fn fixture_value() -> Value {
    serde_json::from_slice(GAME_DATA).unwrap()
}

#[test]
fn missing_selected_player_fails_closed() {
    let error = decode_selected_player(GAME_DATA, &PlayerSelector::user_id("not-present").unwrap())
        .unwrap_err();

    assert!(matches!(error, RestError::SelectedPlayerMissing));
}

#[test]
fn duplicate_selected_player_fails_closed() {
    let mut value = fixture_value();
    let actors = value["ActorData"].as_array_mut().unwrap();
    actors.push(actors[0].clone());
    let bytes = serde_json::to_vec(&value).unwrap();

    let error = decode_selected_player(&bytes, &PlayerSelector::user_id("selected-user").unwrap())
        .unwrap_err();

    assert!(matches!(error, RestError::SelectedPlayerAmbiguous));
}

#[test]
fn inactive_or_activity_unknown_selected_player_fails_closed() {
    for active in [Some(json!("false")), None] {
        let mut value = fixture_value();
        match active {
            Some(active) => value["ActorData"][0]["IsActive"] = active,
            None => {
                value["ActorData"][0]
                    .as_object_mut()
                    .expect("selected actor")
                    .remove("IsActive");
            }
        }
        let bytes = serde_json::to_vec(&value).unwrap();

        let error =
            decode_selected_player(&bytes, &PlayerSelector::user_id("selected-user").unwrap())
                .unwrap_err();

        assert!(matches!(error, RestError::SelectedPlayerInactive));
    }
}

#[test]
fn nickname_cannot_be_constructed_as_a_selector() {
    let debug = format!(
        "{:?}",
        PlayerSelector::user_id("selected-private-nickname").unwrap()
    );

    assert_eq!(debug, "[REDACTED]");
    assert!(!debug.contains("selected-private-nickname"));
}

#[test]
fn malformed_or_non_player_matches_are_rejected() {
    let wrong_type = json!({
        "Time": "2026-07-17 21:15:30",
        "FPS": 60.0,
        "AverageFPS": 60.0,
        "ActorData": [{
            "Type": "Character",
            "InstanceID": "selected-instance",
            "UnitType": "OtomoPal",
            "userid": "selected-user",
            "LocationX": 1.0,
            "LocationY": 2.0,
            "LocationZ": 3.0
        }]
    });

    let error = decode_selected_player(
        &serde_json::to_vec(&wrong_type).unwrap(),
        &PlayerSelector::user_id("selected-user").unwrap(),
    )
    .unwrap_err();
    assert!(matches!(error, RestError::SelectedPlayerMissing));
}

#[test]
fn unknown_actor_fields_fail_strict_schema_decode() {
    let mut value = fixture_value();
    value["ActorData"][0]["UnexpectedSecretField"] = json!("must-not-be-ignored");

    let error = decode_selected_player(
        &serde_json::to_vec(&value).unwrap(),
        &PlayerSelector::user_id("selected-user").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(error, RestError::Decode { .. }));
}

#[test]
fn present_optional_actor_fields_must_have_their_official_type() {
    for field in ["ip", "RotationZ", "IsActive"] {
        let mut value = fixture_value();
        value["ActorData"][0][field] = Value::Null;

        let error = decode_selected_player(
            &serde_json::to_vec(&value).unwrap(),
            &PlayerSelector::user_id("selected-user").unwrap(),
        )
        .unwrap_err();

        assert!(
            matches!(error, RestError::Decode { .. }),
            "{field} must reject JSON null when present"
        );
    }
}

#[test]
fn empty_selector_and_world_alias_fail_closed() {
    assert!(matches!(
        PlayerSelector::user_id(""),
        Err(RestError::InvalidSelector)
    ));

    let selected = decode_selected_player(
        GAME_DATA,
        &PlayerSelector::user_id("selected-user").unwrap(),
    )
    .unwrap();
    let error = pal_rest::sanitize_player(selected, &pal_rest::Pseudonymizer::new([1; 32]), "")
        .unwrap_err();
    assert!(matches!(error, RestError::InvalidWorldAlias));
}
