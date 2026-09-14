use serde_json::Value;

const FIXTURE: &str = include_str!("../../../cloudflare/fixtures/projection.demo.json");

#[test]
fn projection_fixture_is_redacted_and_container_stable() {
    let document: Value = serde_json::from_str(FIXTURE).unwrap();
    let profile = document["profile"].as_object().unwrap();
    for forbidden in ["character_name", "gold", "last_save_at"] {
        assert!(
            !profile.contains_key(forbidden),
            "{forbidden} must stay local"
        );
    }

    let pal = document["pals"][0].as_object().unwrap();
    for forbidden in ["nickname", "location_label"] {
        assert!(!pal.contains_key(forbidden), "{forbidden} must stay local");
    }
    assert!(pal.contains_key("container_ordinal"));
    assert!(pal.contains_key("slot_index"));

    let slot = document["inventory"][0].as_object().unwrap();
    assert!(slot.contains_key("container_ordinal"));

    let serialized = serde_json::to_string(&document).unwrap();
    for forbidden in ["jukqaz", "@example.com", "C:\\", "PalServer", "password"] {
        assert!(
            !serialized.contains(forbidden),
            "fixture leaked forbidden token {forbidden}"
        );
    }
}
