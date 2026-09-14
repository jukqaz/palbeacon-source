use pal_rest::{Pseudonymizer, RestError, decode_info, decode_metrics, sanitize_server_info};

const METRICS: &[u8] = include_bytes!("../../../tests/fixtures/rest/metrics.json");
const INFO: &[u8] = include_bytes!("../../../tests/fixtures/rest/info.json");

#[test]
fn decodes_exact_official_metrics_schema() {
    let metrics = decode_metrics(METRICS).unwrap();

    assert_eq!(metrics.server_fps, 57);
    assert_eq!(metrics.current_player_count, 3);
    assert_eq!(metrics.server_frame_time_ms, 16.7671);
    assert_eq!(metrics.max_player_count, 32);
    assert_eq!(metrics.uptime_seconds, 3600);
    assert_eq!(metrics.base_camp_count, 4);
    assert_eq!(metrics.game_days, 127);
}

#[test]
fn info_exposes_version_and_only_keyed_server_identity() {
    let raw = decode_info(INFO).unwrap();
    let safe = sanitize_server_info(raw, &Pseudonymizer::new([11_u8; 32]), "world-a").unwrap();
    let encoded = serde_json::to_vec(&safe).unwrap();

    assert_eq!(safe.version, "v1.0.1");
    assert_eq!(safe.server_subject_id.len(), 64);
    for sentinel in [
        b"private-server-name".as_slice(),
        b"private-server-description".as_slice(),
        b"A7E97BAA767DB9029EF013BB71E993A0".as_slice(),
    ] {
        assert!(
            !encoded
                .windows(sentinel.len())
                .any(|window| window == sentinel)
        );
    }
}

#[test]
fn info_and_metrics_reject_unknown_or_missing_fields() {
    let metrics_unknown = br#"{
      "serverfps":57,"currentplayernum":3,"serverframetime":16.7,
      "maxplayernum":32,"uptime":1,"basecampnum":4,"days":1,"extra":0
    }"#;
    assert!(matches!(
        decode_metrics(metrics_unknown),
        Err(RestError::Decode { .. })
    ));

    let info_missing = br#"{"version":"v1.0.1","servername":"x","description":"y"}"#;
    assert!(matches!(
        decode_info(info_missing),
        Err(RestError::Decode { .. })
    ));
}
