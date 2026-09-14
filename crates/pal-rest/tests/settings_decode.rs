use pal_rest::{EndpointKind, RestError, decode_settings};

const SETTINGS: &[u8] = br#"{
    "Difficulty":"Normal",
    "ExpRate":1.0,
    "PalCaptureRate":1.5,
    "PalSpawnNumRate":2.0,
    "CollectionDropRate":3.0,
    "CollectionObjectHpRate":1.0,
    "CollectionObjectRespawnSpeedRate":0.5,
    "EnemyDropItemRate":2.5,
    "BaseCampWorkerMaxNum":20,
    "PalEggDefaultHatchingTime":0.0,
    "WorkSpeedRate":1.25,
    "bEnableFastTravel":true,
    "ServerName":"discarded",
    "PublicIP":"discarded"
}"#;

#[test]
fn extracts_only_farming_and_progression_settings() {
    let settings = decode_settings(SETTINGS).unwrap();

    assert_eq!(settings.experience_rate, 1.0);
    assert_eq!(settings.capture_rate, 1.5);
    assert_eq!(settings.pal_spawn_rate, 2.0);
    assert_eq!(settings.collection_drop_rate, 3.0);
    assert_eq!(settings.collection_object_hp_rate, 1.0);
    assert_eq!(settings.collection_respawn_rate, 0.5);
    assert_eq!(settings.enemy_drop_rate, 2.5);
    assert_eq!(settings.base_worker_limit, 20);
    assert_eq!(settings.egg_hatching_hours, 0.0);
    assert_eq!(settings.work_speed_rate, 1.25);
    assert!(settings.fast_travel_enabled);
}

#[test]
fn malformed_missing_or_negative_selected_settings_fail_closed() {
    for body in [
        SETTINGS
            .windows(b"\"WorkSpeedRate\":1.25,".len())
            .position(|window| window == b"\"WorkSpeedRate\":1.25,")
            .map(|start| {
                let mut value = SETTINGS.to_vec();
                value.drain(start..start + b"\"WorkSpeedRate\":1.25,".len());
                value
            })
            .unwrap(),
        SETTINGS.replace_ascii(b"\"EnemyDropItemRate\":2.5", b"\"EnemyDropItemRate\":-1."),
        SETTINGS.replace_ascii(b"\"PalCaptureRate\":1.5", b"\"PalCaptureRate\":\"x\""),
    ] {
        assert!(matches!(
            decode_settings(&body),
            Err(RestError::Decode {
                endpoint: EndpointKind::Settings
            })
        ));
    }
}

trait ReplaceAscii {
    fn replace_ascii(&self, from: &[u8], to: &[u8]) -> Vec<u8>;
}

impl ReplaceAscii for [u8] {
    fn replace_ascii(&self, from: &[u8], to: &[u8]) -> Vec<u8> {
        let start = self
            .windows(from.len())
            .position(|window| window == from)
            .expect("fixture field");
        let mut output = self.to_vec();
        output.splice(start..start + from.len(), to.iter().copied());
        output
    }
}
