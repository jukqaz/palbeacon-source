use pal_domain::{
    DisplayMode, ExpandedMapView, Freshness, MiniMapView, OverlaySettings, PositionSample,
    SampleClock, SettingsValidationError,
};
use pal_state::{ClockInvalidReason, Event, PositionSourceEvent, ReducerError, StateReducer};

fn connected(generation: u64) -> Event {
    Event::PositionSource(PositionSourceEvent::Connected { generation })
}

fn disconnected(generation: u64) -> Event {
    Event::PositionSource(PositionSourceEvent::Disconnected { generation })
}

fn clock_invalid(generation: u64) -> Event {
    Event::PositionSource(PositionSourceEvent::ClockInvalid {
        generation,
        reason: ClockInvalidReason::MonotonicRegression,
    })
}

fn sample(
    generation: u64,
    boot: &[u8],
    sequence: u64,
    x: f64,
    heading: Option<f32>,
    received_ms: u64,
    age_ms: u64,
) -> PositionSample {
    PositionSample::new(
        "test-world",
        b"subject",
        boot,
        generation,
        sequence,
        x,
        2.0,
        3.0,
        heading,
        SampleClock::received_with_age(age_ms, received_ms),
    )
    .unwrap()
}

fn position(value: PositionSample) -> Event {
    Event::PositionSource(PositionSourceEvent::Sample(value))
}

fn live_reducer() -> StateReducer {
    let mut reducer = StateReducer::default();
    reducer.apply(connected(1)).unwrap();
    reducer
        .apply(position(sample(
            1,
            b"boot-a",
            1,
            10.0,
            Some(45.0),
            1_000,
            0,
        )))
        .unwrap();
    reducer
}

#[test]
fn trusted_position_requires_connection_valid_clock_and_a_sample() {
    let mut reducer = StateReducer::default();
    assert!(!reducer.has_trusted_position());

    reducer.apply(connected(1)).unwrap();
    assert!(!reducer.has_trusted_position());

    reducer
        .apply(position(sample(
            1,
            b"boot-a",
            1,
            10.0,
            Some(45.0),
            1_000,
            0,
        )))
        .unwrap();
    assert!(reducer.has_trusted_position());
}

#[test]
fn clock_failure_or_disconnect_revokes_a_retained_position() {
    let mut reducer = live_reducer();
    assert!(reducer.has_trusted_position());

    reducer.apply(clock_invalid(1)).unwrap();
    assert!(!reducer.has_trusted_position());
    assert!(reducer.last_position().is_some());

    reducer
        .apply(position(sample(
            1,
            b"boot-a",
            2,
            11.0,
            Some(46.0),
            1_100,
            0,
        )))
        .unwrap();
    assert!(reducer.has_trusted_position());

    reducer.apply(disconnected(1)).unwrap();
    assert!(!reducer.has_trusted_position());
    assert!(reducer.last_position().is_some());
}

#[test]
fn unavailable_world_clears_position_without_disconnect_and_accepts_recovery() {
    let mut reducer = live_reducer();

    reducer
        .apply(Event::PositionSource(PositionSourceEvent::Unavailable {
            generation: 1,
        }))
        .unwrap();

    assert!(!reducer.has_trusted_position());
    assert!(reducer.last_position().is_none());
    assert_eq!(reducer.freshness(1_001), Freshness::Stale);

    reducer
        .apply(position(sample(
            1,
            b"boot-a",
            2,
            11.0,
            Some(46.0),
            1_100,
            0,
        )))
        .unwrap();

    assert!(reducer.has_trusted_position());
    assert_eq!(reducer.last_position().unwrap().sequence(), 2);
}

#[test]
fn duplicate_out_of_order_and_gap_samples_are_deterministic() {
    let mut reducer = live_reducer();

    reducer
        .apply(position(sample(1, b"boot-a", 1, 99.0, Some(1.0), 1_100, 0)))
        .unwrap();
    reducer
        .apply(position(sample(1, b"boot-a", 0, 98.0, Some(2.0), 1_100, 0)))
        .unwrap();
    reducer
        .apply(position(sample(1, b"boot-a", 5, 50.0, Some(5.0), 1_200, 0)))
        .unwrap();
    reducer
        .apply(position(sample(1, b"boot-a", 3, 30.0, Some(3.0), 1_300, 0)))
        .unwrap();

    assert_eq!(reducer.last_position().unwrap().sequence(), 5);
    assert_eq!(reducer.last_position().unwrap().x(), 50.0);
    let diagnostics = reducer.diagnostics();
    assert_eq!(diagnostics.accepted_samples, 2);
    assert_eq!(diagnostics.duplicate_drops, 1);
    assert_eq!(diagnostics.out_of_order_drops, 1);
    assert_eq!(diagnostics.invalid_transition_drops, 1);
    assert_eq!(diagnostics.gap_events, 1);
    assert_eq!(diagnostics.missing_sequences, 3);
}

#[test]
fn a_higher_generation_accepts_a_new_boot_and_resets_sequence() {
    let mut reducer = live_reducer();
    reducer.apply(connected(2)).unwrap();

    let awaiting = reducer.core_state(1_100);
    assert!(awaiting.connected());
    assert_eq!(awaiting.freshness(), Freshness::Stale);
    assert!(awaiting.position_sample().is_none());

    reducer
        .apply(position(sample(2, b"boot-b", 1, 20.0, None, 1_200, 0)))
        .unwrap();
    assert_eq!(reducer.last_position().unwrap().agent_boot_id(), b"boot-b");
    assert_eq!(reducer.last_position().unwrap().sequence(), 1);
    assert!(!reducer.core_state(1_200).heading_available());
}

#[test]
fn same_generation_boot_change_is_rejected() {
    let mut reducer = live_reducer();
    reducer
        .apply(position(sample(
            1,
            b"boot-b",
            2,
            99.0,
            Some(90.0),
            1_100,
            0,
        )))
        .unwrap();

    assert_eq!(reducer.last_position().unwrap().agent_boot_id(), b"boot-a");
    assert_eq!(reducer.last_position().unwrap().x(), 10.0);
    assert_eq!(reducer.diagnostics().same_generation_boot_mismatch_drops, 1);
}

#[test]
fn old_and_future_generation_events_never_roll_state_back() {
    let mut reducer = live_reducer();
    reducer.apply(connected(2)).unwrap();
    reducer
        .apply(position(sample(2, b"boot-b", 1, 20.0, None, 2_000, 0)))
        .unwrap();

    reducer
        .apply(position(sample(1, b"boot-a", 2, 11.0, None, 2_100, 0)))
        .unwrap();
    reducer.apply(disconnected(1)).unwrap();
    reducer
        .apply(position(sample(3, b"boot-c", 1, 30.0, None, 2_100, 0)))
        .unwrap();
    reducer.apply(disconnected(3)).unwrap();
    reducer.apply(clock_invalid(3)).unwrap();

    assert_eq!(reducer.last_position().unwrap().x(), 20.0);
    assert!(reducer.core_state(2_100).connected());
    assert_eq!(reducer.diagnostics().old_generation_drops, 2);
    assert_eq!(reducer.diagnostics().invalid_transition_drops, 3);
}

#[test]
fn duplicate_connected_is_noop_and_cannot_revive_disconnected_generation() {
    let mut reducer = live_reducer();
    reducer.apply(connected(1)).unwrap();
    assert_eq!(reducer.diagnostics().accepted_samples, 1);

    reducer.apply(disconnected(1)).unwrap();
    reducer.apply(connected(1)).unwrap();
    let state = reducer.core_state(1_100);
    assert!(!state.connected());
    assert_eq!(state.freshness(), Freshness::Offline);
}

#[test]
fn zero_generation_and_zero_sequence_are_dropped() {
    let mut reducer = StateReducer::default();
    reducer.apply(connected(0)).unwrap();
    reducer
        .apply(position(sample(0, b"boot", 1, 1.0, None, 0, 0)))
        .unwrap();
    reducer.apply(connected(1)).unwrap();
    reducer
        .apply(position(sample(1, b"boot", 0, 1.0, None, 0, 0)))
        .unwrap();

    assert!(reducer.last_position().is_none());
    assert_eq!(reducer.diagnostics().invalid_transition_drops, 3);
}

#[test]
fn disconnect_is_immediately_offline_and_preserves_last_position() {
    let mut reducer = live_reducer();
    reducer.apply(disconnected(1)).unwrap();

    let state = reducer.core_state(1_001);
    assert_eq!(state.freshness(), Freshness::Offline);
    assert!(!state.connected());
    assert!(!state.interpolate_position());
    assert_eq!(state.position_sample().unwrap().x(), 10.0);
}

#[test]
fn freshness_boundaries_and_stale_freeze_are_exact() {
    let reducer = live_reducer();
    for (now, expected, interpolate) in [
        (2_500, Freshness::Live, true),
        (2_501, Freshness::Delayed, true),
        (6_000, Freshness::Delayed, true),
        (6_001, Freshness::Stale, false),
    ] {
        let state = reducer.core_state(now);
        assert_eq!(state.freshness(), expected);
        assert_eq!(state.interpolate_position(), interpolate);
        assert_eq!(state.position_sample().unwrap().x(), 10.0);
    }
}

#[test]
fn clock_invalid_stales_then_valid_sample_recovers() {
    let mut reducer = live_reducer();
    reducer.apply(clock_invalid(1)).unwrap();
    let invalid = reducer.core_state(1_100);
    assert_eq!(invalid.freshness(), Freshness::Stale);
    assert!(!invalid.interpolate_position());
    assert_eq!(invalid.position_sample().unwrap().x(), 10.0);
    assert_eq!(reducer.diagnostics().clock_invalid_events, 1);

    reducer
        .apply(position(sample(1, b"boot-a", 2, 20.0, None, 1_200, 0)))
        .unwrap();
    let recovered = reducer.core_state(1_200);
    assert_eq!(recovered.freshness(), Freshness::Live);
    assert!(recovered.interpolate_position());
    assert!(!recovered.heading_available());
}

#[test]
fn monotonic_rollback_and_age_overflow_use_saturation() {
    let mut rollback = StateReducer::default();
    rollback.apply(connected(1)).unwrap();
    rollback
        .apply(position(sample(1, b"boot", 1, 1.0, None, 5_000, 1_500)))
        .unwrap();
    assert_eq!(rollback.core_state(4_000).freshness(), Freshness::Live);

    let mut overflow = StateReducer::default();
    overflow.apply(connected(1)).unwrap();
    overflow
        .apply(position(sample(1, b"boot", 1, 2.0, None, 1, u64::MAX - 1)))
        .unwrap();
    let state = overflow.core_state(u64::MAX);
    assert_eq!(state.freshness(), Freshness::Stale);
    assert!(!state.interpolate_position());
    assert_eq!(state.position_sample().unwrap().x(), 2.0);
}

#[test]
fn settings_versions_are_strict_and_invalid_candidates_are_atomic() {
    let mut reducer = StateReducer::default();
    assert_eq!(reducer.settings_version(), 1);

    let candidate = OverlaySettings {
        opacity: 0.75,
        ..OverlaySettings::default()
    };
    reducer
        .apply(Event::SettingsApplied {
            version: 2,
            settings: candidate.clone(),
        })
        .unwrap();
    assert_eq!(reducer.settings_version(), 2);
    assert_eq!(reducer.settings(), &candidate);

    let error = reducer
        .apply(Event::SettingsApplied {
            version: 2,
            settings: OverlaySettings::default(),
        })
        .unwrap_err();
    assert_eq!(
        error,
        ReducerError::SettingsVersionNotNewer {
            current: 2,
            candidate: 2,
        }
    );

    let invalid = OverlaySettings {
        opacity: f32::NAN,
        ..OverlaySettings::default()
    };
    let error = reducer
        .apply(Event::SettingsApplied {
            version: 3,
            settings: invalid,
        })
        .unwrap_err();
    assert_eq!(
        error,
        ReducerError::Settings(SettingsValidationError::OpacityOutOfRange)
    );
    assert_eq!(reducer.settings_version(), 2);
    assert_eq!(reducer.settings(), &candidate);

    assert_eq!(
        reducer
            .apply(Event::SettingsApplied {
                version: 0,
                settings: OverlaySettings::default(),
            })
            .unwrap_err(),
        ReducerError::SettingsVersionZero
    );
}

#[test]
fn display_mode_change_increments_once_and_overflow_is_atomic() {
    let mut reducer = StateReducer::default();
    reducer
        .apply(Event::SetDisplayMode(DisplayMode::MiniMap))
        .unwrap();
    assert_eq!(reducer.settings_version(), 1);

    reducer
        .apply(Event::SetDisplayMode(DisplayMode::ExpandedMap))
        .unwrap();
    assert_eq!(reducer.settings_version(), 2);
    reducer
        .apply(Event::SetDisplayMode(DisplayMode::ExpandedMap))
        .unwrap();
    assert_eq!(reducer.settings_version(), 2);

    reducer
        .apply(Event::SettingsApplied {
            version: u64::MAX,
            settings: OverlaySettings::default(),
        })
        .unwrap();
    assert_eq!(
        reducer
            .apply(Event::SetDisplayMode(DisplayMode::ExpandedMap))
            .unwrap_err(),
        ReducerError::SettingsVersionOverflow
    );
    assert_eq!(reducer.settings().display_mode, DisplayMode::MiniMap);
}

#[test]
fn expanded_open_pan_close_restores_minimap_and_reuses_zoom() {
    let mut reducer = StateReducer::default();
    let mini = MiniMapView::new(12.0, -5.0, 1.75).unwrap();
    reducer.apply(Event::SetMiniMapView(mini.clone())).unwrap();

    reducer
        .apply(Event::SetDisplayMode(DisplayMode::ExpandedMap))
        .unwrap();
    assert_eq!(reducer.expanded_map_view().center_x(), 12.0);
    assert_eq!(reducer.expanded_map_view().center_y(), -5.0);
    reducer
        .apply(Event::ExpandedPanZoom {
            center_x: 40.0,
            center_y: 80.0,
            zoom: 3.0,
        })
        .unwrap();
    assert_eq!(reducer.minimap_view(), &mini);

    reducer
        .apply(Event::SetDisplayMode(DisplayMode::MiniMap))
        .unwrap();
    assert_eq!(reducer.minimap_view(), &mini);

    reducer
        .apply(Event::SetDisplayMode(DisplayMode::ExpandedMap))
        .unwrap();
    assert_eq!(reducer.expanded_map_view().center_x(), mini.center_x());
    assert_eq!(reducer.expanded_map_view().center_y(), mini.center_y());
    assert_eq!(reducer.expanded_map_view().zoom(), 3.0);
}

#[test]
fn invalid_expanded_pan_zoom_is_atomic_and_names_field() {
    let mut reducer = StateReducer::default();
    reducer
        .apply(Event::SetDisplayMode(DisplayMode::ExpandedMap))
        .unwrap();
    let before: ExpandedMapView = reducer.expanded_map_view().clone();
    let error = reducer
        .apply(Event::ExpandedPanZoom {
            center_x: f64::NAN,
            center_y: 2.0,
            zoom: 1.0,
        })
        .unwrap_err();
    assert_eq!(error.field(), Some("expanded_map_view.center_x"));
    assert_eq!(reducer.expanded_map_view(), &before);
}

#[test]
fn core_state_exposes_complete_immutable_runtime_output() {
    let mut reducer = live_reducer();
    reducer.apply(Event::SetVisible(false)).unwrap();
    let state = reducer.core_state(1_000);

    assert_eq!(state.settings_version(), 1);
    assert!(state.connected());
    assert!(!state.visible());
    assert!(state.interpolate_position());
    assert_eq!(state.mini_map_view(), reducer.minimap_view());
    assert_eq!(state.expanded_map_view(), reducer.expanded_map_view());
}
