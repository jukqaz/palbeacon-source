#![cfg(feature = "test-harness")]

use std::path::PathBuf;

use pal_domain::{Freshness, OverlaySettings, PositionSample, RotationMode, SampleClock};
use pal_overlay_win::actual_map_preview::{
    WorldToImageTransform, authoritative_main_map_world_to_image,
};
use pal_overlay_win::actual_map_runtime::{
    ActualMapPreviewRuntime, PendingActualMapView, PreviewTick, PreviewVisibilityAction,
    PreviewVisibilityGate,
};
use pal_state::{
    ClockInvalidReason, PositionSource, PositionSourceError, PositionSourceEvent, ReplayConfig,
    ReplayPositionSource,
};

const MAP_SIDE: u32 = 2_048;

#[test]
fn replay_samples_move_the_real_map_center_and_unchanged_ticks_do_not_redraw() {
    let source = replay_source();
    let mut runtime = ActualMapPreviewRuntime::new(
        source,
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");

    let first = present(runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("first tick"));
    let idle = runtime.tick(50, MAP_SIDE, MAP_SIDE).expect("idle tick");
    let second = present(runtime.tick(100, MAP_SIDE, MAP_SIDE).expect("second tick"));

    assert_eq!(idle, PreviewTick::Unchanged);
    assert_eq!(first.sequence(), 1);
    assert_eq!(second.sequence(), 2);
    assert_ne!(first.view().center_x(), second.view().center_x());
    assert_ne!(first.view().center_y(), second.view().center_y());
    assert_eq!(second.freshness(), Freshness::Live);
}

#[test]
fn stationary_higher_sequence_samples_do_not_present_and_refresh_freshness() {
    let mut runtime = ActualMapPreviewRuntime::new(
        StationaryRefreshSource::new(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");

    let first = present(runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("first tick"));
    assert_eq!(first.sequence(), 1);
    assert_eq!(
        runtime.performance_counters().stationary_suppressed_frames,
        0
    );
    assert_eq!(
        runtime
            .tick(100, MAP_SIDE, MAP_SIDE)
            .expect("fresh same-pose tick"),
        PreviewTick::Unchanged
    );
    assert_eq!(
        runtime.performance_counters().stationary_suppressed_frames,
        1
    );
    assert_eq!(
        runtime
            .tick(1_600, MAP_SIDE, MAP_SIDE)
            .expect("refreshed live tick"),
        PreviewTick::Unchanged
    );
    assert_eq!(
        runtime.performance_counters().stationary_suppressed_frames,
        1
    );
    assert_eq!(
        runtime
            .tick(1_601, MAP_SIDE, MAP_SIDE)
            .expect("delayed tick"),
        PreviewTick::MetadataChanged(Freshness::Delayed)
    );
    assert_eq!(
        runtime.performance_counters().stationary_suppressed_frames,
        1
    );
}

#[test]
fn runtime_projects_samples_with_the_explicitly_injected_transform() {
    let injected = WorldToImageTransform::from_world_bounds(-400_000.0, 0.0, 0.0, 400_000.0)
        .expect("custom transform");
    let expected = injected.project(-343_155.0, 244_585.0);
    let mut runtime = ActualMapPreviewRuntime::new(
        HeldPositionSource::new(),
        OverlaySettings::default(),
        injected,
    )
    .expect("runtime");

    let frame = present(runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("first tick"));

    assert!((frame.view().center_x() - expected.x() * f64::from(MAP_SIDE)).abs() < 0.001);
    assert!((frame.view().center_y() - expected.y() * f64::from(MAP_SIDE)).abs() < 0.001);
}

#[test]
fn north_up_rotates_the_arrow_while_heading_up_rotates_the_map() {
    let north_settings = OverlaySettings {
        rotation_mode: RotationMode::NorthUp,
        ..OverlaySettings::default()
    };
    let mut north = ActualMapPreviewRuntime::new(
        replay_source(),
        north_settings,
        authoritative_main_map_world_to_image(),
    )
    .expect("north runtime");
    let _ = north.tick(0, MAP_SIDE, MAP_SIDE).expect("first north tick");
    let north_frame = present(
        north
            .tick(1_000, MAP_SIDE, MAP_SIDE)
            .expect("second north tick"),
    );

    let heading_settings = OverlaySettings {
        rotation_mode: RotationMode::HeadingUp,
        ..OverlaySettings::default()
    };
    let mut heading = ActualMapPreviewRuntime::new(
        replay_source(),
        heading_settings,
        authoritative_main_map_world_to_image(),
    )
    .expect("heading runtime");
    let _ = heading
        .tick(0, MAP_SIDE, MAP_SIDE)
        .expect("first heading tick");
    let heading_frame = present(
        heading
            .tick(1_000, MAP_SIDE, MAP_SIDE)
            .expect("second heading tick"),
    );

    assert_eq!(north_frame.view().map_rotation_degrees(), 0.0);
    assert_eq!(north_frame.view().player_rotation_degrees(), 45.0);
    assert_eq!(heading_frame.view().map_rotation_degrees(), -45.0);
    assert_eq!(heading_frame.view().player_rotation_degrees(), 0.0);
    assert_eq!(
        north.last_live_world_pose(),
        Some((-293_155.0, 284_585.0, Some(45.0)))
    );
    assert_eq!(
        heading.last_live_world_pose(),
        Some((-293_155.0, 284_585.0, Some(45.0)))
    );
}

#[test]
fn changing_the_map_raster_dimensions_reprojects_without_a_new_position() {
    let mut runtime = ActualMapPreviewRuntime::new(
        replay_source(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");
    let small = present(runtime.tick(0, 2_048, 2_048).expect("small map tick"));
    let large = present(runtime.tick(50, 4_096, 4_096).expect("large map tick"));

    assert!((large.view().center_x() - small.view().center_x() * 2.0).abs() < 0.001);
    assert!((large.view().center_y() - small.view().center_y() * 2.0).abs() < 0.001);
}

#[test]
fn freshness_boundaries_rerasterize_once_to_remove_a_stale_player_marker() {
    let mut runtime = ActualMapPreviewRuntime::new(
        HeldPositionSource::new(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");

    assert_eq!(
        present(runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("live tick")).freshness(),
        Freshness::Live
    );
    assert_eq!(
        runtime
            .tick(1_500, MAP_SIDE, MAP_SIDE)
            .expect("last live tick"),
        PreviewTick::Unchanged
    );
    assert_eq!(
        runtime
            .tick(1_501, MAP_SIDE, MAP_SIDE)
            .expect("delayed tick"),
        PreviewTick::MetadataChanged(Freshness::Delayed)
    );
    let stale = present(runtime.tick(5_001, MAP_SIDE, MAP_SIDE).expect("stale tick"));
    assert_eq!(stale.freshness(), Freshness::Stale);
    assert_eq!(stale.view().player_map_pose(), None);
    assert_eq!(
        runtime
            .tick(6_000, MAP_SIDE, MAP_SIDE)
            .expect("unchanged stale tick"),
        PreviewTick::Unchanged
    );
}

#[test]
fn stale_hides_only_the_player_marker_while_offline_hides_the_overlay() {
    let mut runtime = ActualMapPreviewRuntime::new(
        HeldPositionSource::new(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");
    let mut gate = PreviewVisibilityGate::default();

    let live = runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("live tick");
    assert_eq!(live.visibility_action(), PreviewVisibilityAction::Show);
    assert_eq!(gate.update(live), Some(true));
    assert!(gate.requested_visible());

    let offline = PreviewTick::MetadataChanged(Freshness::Offline);
    let mut offline_gate = PreviewVisibilityGate::default();
    assert_eq!(offline_gate.update(live), Some(true));
    assert_eq!(offline.visibility_action(), PreviewVisibilityAction::Hide);
    assert_eq!(offline_gate.update(offline), Some(false));
    assert!(!offline_gate.requested_visible());

    let stale = runtime.tick(5_001, MAP_SIDE, MAP_SIDE).expect("stale tick");
    let stale_frame = present(stale);
    assert_eq!(stale_frame.freshness(), Freshness::Stale);
    assert_eq!(stale_frame.view().player_map_pose(), None);
    assert_eq!(stale.visibility_action(), PreviewVisibilityAction::Show);
    assert_eq!(gate.update(stale), None);
    assert!(gate.requested_visible());

    let mut recovered_runtime = ActualMapPreviewRuntime::new(
        HeldPositionSource::received_at(6_000),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("recovered runtime");
    let recovered = recovered_runtime
        .tick(6_000, MAP_SIDE, MAP_SIDE)
        .expect("recovered tick");
    assert_eq!(recovered.visibility_action(), PreviewVisibilityAction::Show);
    assert_eq!(gate.update(recovered), None);
    assert!(gate.requested_visible());

    let unavailable = PreviewTick::Unavailable(Freshness::Offline);
    assert_eq!(
        unavailable.visibility_action(),
        PreviewVisibilityAction::Hide
    );
    assert_eq!(gate.update(unavailable), Some(false));
    assert!(!gate.requested_visible());
}

#[test]
fn a_nonzero_shared_monotonic_epoch_preserves_freshness_boundaries() {
    let epoch_offset_ms = 30_000;
    let mut runtime = ActualMapPreviewRuntime::new(
        HeldPositionSource::received_at(epoch_offset_ms),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");

    assert_eq!(
        present(
            runtime
                .tick(epoch_offset_ms, MAP_SIDE, MAP_SIDE)
                .expect("live tick")
        )
        .freshness(),
        Freshness::Live
    );
    assert_eq!(
        runtime
            .tick(epoch_offset_ms + 1_501, MAP_SIDE, MAP_SIDE)
            .expect("delayed tick"),
        PreviewTick::MetadataChanged(Freshness::Delayed)
    );
}

#[test]
fn hidden_overlay_keeps_only_the_latest_view_until_it_is_visible() {
    let mut runtime = ActualMapPreviewRuntime::new(
        replay_source(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");
    let first = present(runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("first tick"));
    let second = present(runtime.tick(100, MAP_SIDE, MAP_SIDE).expect("second tick"));
    let mut pending = PendingActualMapView::default();

    pending.submit(first.view());
    assert_eq!(pending.take_if_visible(false), None);
    pending.submit(second.view());
    assert_eq!(pending.take_if_visible(false), None);
    assert_eq!(pending.take_if_visible(true), Some(second.view()));
    assert_eq!(pending.take_if_visible(true), None);
}

#[test]
fn connected_without_a_sample_is_unavailable() {
    let mut runtime = ActualMapPreviewRuntime::new(
        AvailabilitySource::connected_only(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");

    assert_eq!(
        runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("connected tick"),
        PreviewTick::Unavailable(Freshness::Stale)
    );
    assert_eq!(
        runtime.tick(1, MAP_SIDE, MAP_SIDE).expect("unchanged tick"),
        PreviewTick::Unchanged
    );
}

#[test]
fn finite_position_outside_the_selected_map_region_fails_closed() {
    let mut runtime = ActualMapPreviewRuntime::new(
        HeldPositionSource::at(349_401.0, 0.0),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");

    let tick = runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("bounded tick");

    assert_eq!(tick, PreviewTick::Unavailable(Freshness::Live));
    assert_eq!(tick.visibility_action(), PreviewVisibilityAction::Hide);
    assert_eq!(
        runtime.last_live_world_pose(),
        Some((349_401.0, 0.0, Some(45.0)))
    );
}

#[test]
fn disconnect_hides_a_retained_last_position_and_recovery_presents_again() {
    let mut runtime = ActualMapPreviewRuntime::new(
        AvailabilitySource::disconnect_then_recover(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");

    let first = present(runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("first sample"));
    assert_eq!(first.sequence(), 1);
    assert_eq!(
        runtime.tick(100, MAP_SIDE, MAP_SIDE).expect("disconnect"),
        PreviewTick::Unavailable(Freshness::Offline)
    );
    assert_eq!(runtime.last_live_world_pose(), None);
    let recovered = present(runtime.tick(200, MAP_SIDE, MAP_SIDE).expect("recovery"));
    assert_eq!(recovered.sequence(), 1);
    assert_eq!(first.view().center_x(), recovered.view().center_x());
    assert_ne!(first.view().center_y(), recovered.view().center_y());
}

#[test]
fn invalid_clock_hides_retained_coordinates_until_a_new_sample_recovers() {
    let mut runtime = ActualMapPreviewRuntime::new(
        AvailabilitySource::clock_invalid_then_recover(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");

    let _ = present(runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("first sample"));
    assert_eq!(
        runtime
            .tick(100, MAP_SIDE, MAP_SIDE)
            .expect("clock invalid"),
        PreviewTick::Unavailable(Freshness::Stale)
    );
    assert_eq!(
        present(runtime.tick(200, MAP_SIDE, MAP_SIDE).expect("recovery")).sequence(),
        2
    );
}

#[test]
fn unavailable_state_clears_a_pending_hidden_view() {
    let mut runtime = ActualMapPreviewRuntime::new(
        replay_source(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");
    let frame = present(runtime.tick(0, MAP_SIDE, MAP_SIDE).expect("first sample"));
    let mut pending = PendingActualMapView::default();
    pending.submit(frame.view());

    pending.clear();

    assert_eq!(pending.take_if_visible(true), None);
}

fn present(tick: PreviewTick) -> pal_overlay_win::actual_map_runtime::ActualMapFrame {
    match tick {
        PreviewTick::Present(frame) => frame,
        PreviewTick::Unchanged | PreviewTick::MetadataChanged(_) | PreviewTick::Unavailable(_) => {
            panic!("expected a present frame")
        }
    }
}

fn replay_source() -> ReplayPositionSource {
    ReplayPositionSource::from_path(
        fixture_path(),
        ReplayConfig {
            world_alias: "preview-world".to_owned(),
            subject_id: b"preview-subject".to_vec(),
            starting_generation: 1,
            base_boot_id: b"preview-boot".to_vec(),
            loop_playback: true,
            starting_monotonic_ms: 0,
        },
    )
    .expect("valid replay")
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("replay")
        .join("palworld-map-route.json")
}

struct HeldPositionSource {
    phase: u8,
    sample: PositionSample,
}

struct StationaryRefreshSource {
    phase: u8,
    sample: PositionSample,
}

impl StationaryRefreshSource {
    fn new() -> Self {
        Self {
            phase: 0,
            sample: HeldPositionSource::new().sample,
        }
    }
}

impl PositionSource for StationaryRefreshSource {
    fn poll(
        &mut self,
        now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
        let event = match self.phase {
            0 => Some(PositionSourceEvent::Connected { generation: 1 }),
            1 => Some(PositionSourceEvent::Sample(self.sample.clone())),
            2 if now_monotonic_ms >= 100 => Some(PositionSourceEvent::Sample(sample_like(
                &self.sample,
                2,
                0.0,
                self.sample.heading_degrees(),
                now_monotonic_ms,
            ))),
            _ => None,
        };
        if event.is_some() {
            self.phase = self.phase.saturating_add(1);
        }
        Ok(event)
    }
}

fn sample_like(
    sample: &PositionSample,
    sequence: u64,
    x_offset: f64,
    heading_degrees: Option<f32>,
    received_at_monotonic_ms: u64,
) -> PositionSample {
    PositionSample::new(
        sample.world_alias(),
        sample.subject_id(),
        sample.agent_boot_id(),
        sample.source_connection_generation(),
        sequence,
        sample.x() + x_offset,
        sample.y(),
        sample.z(),
        heading_degrees,
        SampleClock::received_with_age(0, received_at_monotonic_ms),
    )
    .expect("valid sample")
}

impl HeldPositionSource {
    fn new() -> Self {
        Self::received_at(0)
    }

    fn received_at(received_at_monotonic_ms: u64) -> Self {
        Self::at_received(-343_155.0, 244_585.0, received_at_monotonic_ms)
    }

    fn at(x: f64, y: f64) -> Self {
        Self::at_received(x, y, 0)
    }

    fn at_received(x: f64, y: f64, received_at_monotonic_ms: u64) -> Self {
        Self {
            phase: 0,
            sample: PositionSample::new(
                "preview-world",
                b"preview-subject",
                b"preview-boot",
                1,
                1,
                x,
                y,
                0.0,
                Some(45.0),
                SampleClock::received_with_age(0, received_at_monotonic_ms),
            )
            .expect("valid sample"),
        }
    }
}

impl PositionSource for HeldPositionSource {
    fn poll(
        &mut self,
        _now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
        let event = match self.phase {
            0 => Some(PositionSourceEvent::Connected { generation: 1 }),
            1 => Some(PositionSourceEvent::Sample(self.sample.clone())),
            _ => None,
        };
        self.phase = self.phase.saturating_add(1);
        Ok(event)
    }
}

#[derive(Clone, Copy)]
enum AvailabilityScenario {
    ConnectedOnly,
    DisconnectThenRecover,
    ClockInvalidThenRecover,
}

struct AvailabilitySource {
    scenario: AvailabilityScenario,
    phase: u8,
}

impl AvailabilitySource {
    const fn connected_only() -> Self {
        Self {
            scenario: AvailabilityScenario::ConnectedOnly,
            phase: 0,
        }
    }

    const fn disconnect_then_recover() -> Self {
        Self {
            scenario: AvailabilityScenario::DisconnectThenRecover,
            phase: 0,
        }
    }

    const fn clock_invalid_then_recover() -> Self {
        Self {
            scenario: AvailabilityScenario::ClockInvalidThenRecover,
            phase: 0,
        }
    }
}

impl PositionSource for AvailabilitySource {
    fn poll(
        &mut self,
        now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
        let event = match (self.scenario, self.phase) {
            (_, 0) => Some(PositionSourceEvent::Connected { generation: 1 }),
            (AvailabilityScenario::ConnectedOnly, _) => None,
            (_, 1) => Some(PositionSourceEvent::Sample(availability_sample(
                1,
                1,
                -343_155.0,
                now_monotonic_ms,
            ))),
            (AvailabilityScenario::DisconnectThenRecover, 2) if now_monotonic_ms >= 100 => {
                Some(PositionSourceEvent::Disconnected { generation: 1 })
            }
            (AvailabilityScenario::DisconnectThenRecover, 3) if now_monotonic_ms >= 200 => {
                Some(PositionSourceEvent::Connected { generation: 2 })
            }
            (AvailabilityScenario::DisconnectThenRecover, 4) if now_monotonic_ms >= 200 => {
                Some(PositionSourceEvent::Sample(availability_sample(
                    2,
                    1,
                    -300_000.0,
                    now_monotonic_ms,
                )))
            }
            (AvailabilityScenario::ClockInvalidThenRecover, 2) if now_monotonic_ms >= 100 => {
                Some(PositionSourceEvent::ClockInvalid {
                    generation: 1,
                    reason: ClockInvalidReason::MonotonicRegression,
                })
            }
            (AvailabilityScenario::ClockInvalidThenRecover, 3) if now_monotonic_ms >= 200 => {
                Some(PositionSourceEvent::Sample(availability_sample(
                    1,
                    2,
                    -300_000.0,
                    now_monotonic_ms,
                )))
            }
            _ => None,
        };
        if event.is_some() {
            self.phase = self.phase.saturating_add(1);
        }
        Ok(event)
    }
}

fn availability_sample(
    generation: u64,
    sequence: u64,
    x: f64,
    received_at_ms: u64,
) -> PositionSample {
    PositionSample::new(
        "preview-world",
        b"preview-subject",
        if generation == 1 {
            b"preview-boot-a".as_slice()
        } else {
            b"preview-boot-b".as_slice()
        },
        generation,
        sequence,
        x,
        244_585.0,
        0.0,
        Some(45.0),
        SampleClock::received_with_age(0, received_at_ms),
    )
    .expect("valid availability sample")
}
