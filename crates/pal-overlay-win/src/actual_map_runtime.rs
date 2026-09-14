use std::error::Error;
use std::fmt;

use pal_domain::{Freshness, OverlaySettings};
use pal_render::compute_viewport_pose;
use pal_state::{Event, PositionSource, PositionSourceError, ReducerError, StateReducer};

use crate::actual_map_preview::{MapView, MapViewError, WorldToImageTransform};

const MAX_EVENTS_PER_TICK: usize = 64;
const REFERENCE_MAP_SIDE: f64 = 2_048.0;
const BASE_SOURCE_PIXELS_PER_SCREEN_PIXEL: f64 = 1.25;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActualMapFrame {
    view: MapView,
    freshness: Freshness,
    generation: u64,
    sequence: u64,
}

impl ActualMapFrame {
    pub const fn view(self) -> MapView {
        self.view
    }

    pub const fn freshness(self) -> Freshness {
        self.freshness
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn sequence(self) -> u64 {
        self.sequence
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PreviewTick {
    Unchanged,
    MetadataChanged(Freshness),
    Unavailable(Freshness),
    Present(ActualMapFrame),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewVisibilityAction {
    Preserve,
    Show,
    Hide,
}

impl PreviewTick {
    pub const fn freshness(self) -> Option<Freshness> {
        match self {
            Self::Unchanged => None,
            Self::MetadataChanged(freshness) | Self::Unavailable(freshness) => Some(freshness),
            Self::Present(frame) => Some(frame.freshness()),
        }
    }

    pub const fn visibility_action(self) -> PreviewVisibilityAction {
        match self {
            Self::Unchanged => PreviewVisibilityAction::Preserve,
            // A present stale frame is intentionally the last safe map view with its player
            // marker removed. Keep that useful static map visible without claiming an old
            // position is current.
            Self::Present(_) => PreviewVisibilityAction::Show,
            Self::MetadataChanged(freshness) => visibility_action_for_freshness(freshness),
            Self::Unavailable(_) => PreviewVisibilityAction::Hide,
        }
    }
}

#[derive(Default)]
pub struct PreviewVisibilityGate {
    requested_visible: bool,
}

impl PreviewVisibilityGate {
    pub const fn requested_visible(&self) -> bool {
        self.requested_visible
    }

    pub fn update(&mut self, tick: PreviewTick) -> Option<bool> {
        let requested_visible = match tick.visibility_action() {
            PreviewVisibilityAction::Preserve => return None,
            PreviewVisibilityAction::Show => true,
            PreviewVisibilityAction::Hide => false,
        };
        if self.requested_visible == requested_visible {
            return None;
        }
        self.requested_visible = requested_visible;
        Some(requested_visible)
    }
}

#[derive(Default)]
pub struct PendingActualMapView {
    pending: Option<MapView>,
}

impl PendingActualMapView {
    pub fn submit(&mut self, view: MapView) {
        self.pending = Some(view);
    }

    pub fn take_if_visible(&mut self, visible: bool) -> Option<MapView> {
        if visible { self.pending.take() } else { None }
    }

    pub fn clear(&mut self) {
        self.pending = None;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActualMapRuntimeError {
    PositionSource(PositionSourceError),
    Reducer(ReducerError),
    View(MapViewError),
}

impl fmt::Display for ActualMapRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PositionSource(error) => write!(formatter, "position source failed: {error}"),
            Self::Reducer(error) => write!(formatter, "state reducer failed: {error}"),
            Self::View(error) => write!(formatter, "actual-map view failed: {error}"),
        }
    }
}

impl Error for ActualMapRuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PositionSource(error) => Some(error),
            Self::Reducer(error) => Some(error),
            Self::View(error) => Some(error),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameKey {
    generation: u64,
    settings_version: u64,
    map_width: u32,
    map_height: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActualMapRuntimePerformanceCounters {
    pub stationary_suppressed_frames: u64,
}

pub struct ActualMapPreviewRuntime<S> {
    source: S,
    reducer: StateReducer,
    world_to_image: WorldToImageTransform,
    last_frame_key: Option<FrameKey>,
    last_frame_view: Option<MapView>,
    last_freshness: Option<Freshness>,
    last_available: Option<bool>,
    next_freshness_check_ms: Option<u64>,
    performance_counters: ActualMapRuntimePerformanceCounters,
}

impl<S: PositionSource> ActualMapPreviewRuntime<S> {
    pub fn new(
        source: S,
        settings: OverlaySettings,
        world_to_image: WorldToImageTransform,
    ) -> Result<Self, ActualMapRuntimeError> {
        let mut reducer = StateReducer::default();
        reducer
            .apply(Event::SettingsApplied {
                version: 2,
                settings,
            })
            .map_err(ActualMapRuntimeError::Reducer)?;
        Ok(Self {
            source,
            reducer,
            world_to_image,
            last_frame_key: None,
            last_frame_view: None,
            last_freshness: None,
            last_available: None,
            next_freshness_check_ms: None,
            performance_counters: ActualMapRuntimePerformanceCounters::default(),
        })
    }

    pub const fn performance_counters(&self) -> ActualMapRuntimePerformanceCounters {
        self.performance_counters
    }

    pub fn last_world_position(&self) -> Option<(f64, f64)> {
        self.reducer
            .last_position()
            .map(|sample| (sample.x(), sample.y()))
    }

    /// Returns the last trusted, currently available world pose without binding it to a map.
    ///
    /// Expanded-map search can temporarily display a different region from the tracking
    /// runtime. Returning world coordinates here lets that surface apply its own exact affine
    /// transform instead of reusing map pixels from the previously selected region.
    pub fn last_live_world_pose(&self) -> Option<(f64, f64, Option<f32>)> {
        if !self.reducer.has_trusted_position()
            || !matches!(
                self.last_freshness,
                Some(Freshness::Live | Freshness::Delayed)
            )
        {
            return None;
        }
        self.reducer.last_position().map(|sample| {
            let heading = self
                .reducer
                .heading_available()
                .then(|| sample.heading_degrees())
                .flatten();
            (sample.x(), sample.y(), heading)
        })
    }

    /// Returns the producer's latest player-centered view for an explicit recenter request.
    /// Retained search or manual-pan surfaces must not infer this from their own camera center.
    pub fn last_live_tracking_view(&self) -> Option<MapView> {
        if !self.reducer.has_trusted_position()
            || !matches!(
                self.last_freshness,
                Some(Freshness::Live | Freshness::Delayed)
            )
        {
            return None;
        }
        self.last_frame_view
    }

    pub fn select_world_to_image(&mut self, world_to_image: WorldToImageTransform) {
        self.world_to_image = world_to_image;
        self.last_frame_key = None;
        self.last_frame_view = None;
        self.last_available = None;
        self.next_freshness_check_ms = Some(0);
    }

    pub fn apply_settings(
        &mut self,
        version: u64,
        settings: OverlaySettings,
    ) -> Result<(), ActualMapRuntimeError> {
        self.reducer
            .apply(Event::SettingsApplied { version, settings })
            .map_err(ActualMapRuntimeError::Reducer)?;
        self.next_freshness_check_ms = Some(0);
        Ok(())
    }

    pub fn tick(
        &mut self,
        now_monotonic_ms: u64,
        map_width: u32,
        map_height: u32,
    ) -> Result<PreviewTick, ActualMapRuntimeError> {
        let mut source_changed = false;
        for _ in 0..MAX_EVENTS_PER_TICK {
            let Some(event) = self
                .source
                .poll(now_monotonic_ms)
                .map_err(ActualMapRuntimeError::PositionSource)?
            else {
                break;
            };
            source_changed = true;
            self.reducer
                .apply(Event::PositionSource(event))
                .map_err(ActualMapRuntimeError::Reducer)?;
        }
        let freshness = self.reducer.freshness(now_monotonic_ms);
        let trusted = self.reducer.has_trusted_position();
        let inside_selected_region = self.reducer.last_position().is_some_and(|sample| {
            self.world_to_image
                .project_within_bounds(sample.x(), sample.y())
                .is_some()
        });
        let available = trusted
            && inside_selected_region
            && (self.last_available == Some(true) || freshness != Freshness::Stale);
        if !available {
            self.last_frame_key = None;
            self.last_frame_view = None;
            self.next_freshness_check_ms = None;
            let changed =
                self.last_available != Some(false) || self.last_freshness != Some(freshness);
            self.last_available = Some(false);
            self.last_freshness = Some(freshness);
            return Ok(if changed {
                PreviewTick::Unavailable(freshness)
            } else {
                PreviewTick::Unchanged
            });
        }
        self.last_available = Some(true);
        if !source_changed
            && self
                .last_frame_key
                .is_some_and(|key| key.map_width == map_width && key.map_height == map_height)
            && self
                .next_freshness_check_ms
                .is_none_or(|due| now_monotonic_ms < due)
        {
            return Ok(PreviewTick::Unchanged);
        }

        let Some(sample) = self.reducer.last_position() else {
            unreachable!("trusted availability requires a retained position sample");
        };
        self.next_freshness_check_ms = next_freshness_check_ms(
            now_monotonic_ms,
            freshness,
            sample.clock().age_upper_bound_ms(now_monotonic_ms),
        );
        let key = FrameKey {
            generation: sample.source_connection_generation(),
            settings_version: self.reducer.settings_version(),
            map_width,
            map_height,
        };
        if !source_changed && self.last_frame_key == Some(key) {
            if self.last_freshness != Some(freshness) {
                self.last_freshness = Some(freshness);
                if freshness == Freshness::Stale
                    && let Some(view) = self.last_frame_view.map(MapView::without_player_marker)
                {
                    self.last_frame_view = Some(view);
                    return Ok(PreviewTick::Present(ActualMapFrame {
                        view,
                        freshness,
                        generation: sample.source_connection_generation(),
                        sequence: sample.sequence(),
                    }));
                }
                return Ok(PreviewTick::MetadataChanged(freshness));
            }
            return Ok(PreviewTick::Unchanged);
        }

        let normalized = self
            .world_to_image
            .project_within_bounds(sample.x(), sample.y())
            .expect("available samples are inside the selected map region");
        let heading = self
            .reducer
            .heading_available()
            .then(|| sample.heading_degrees())
            .flatten();
        let settings = self.reducer.settings();
        let pose = compute_viewport_pose(settings.rotation_mode, heading);
        let scale = f64::from(map_width) / REFERENCE_MAP_SIDE * BASE_SOURCE_PIXELS_PER_SCREEN_PIXEL
            / f64::from(settings.zoom);
        let view = MapView::with_rotations(
            normalized.x() * f64::from(map_width),
            normalized.y() * f64::from(map_height),
            scale,
            pose.map_rotation_degrees(),
            pose.player_rotation_degrees(),
        )
        .map_err(ActualMapRuntimeError::View)?;
        let view = if freshness == Freshness::Stale {
            view.without_player_marker()
        } else {
            view
        };
        if self.last_frame_key == Some(key) && self.last_frame_view == Some(view) {
            if source_changed {
                self.performance_counters.stationary_suppressed_frames = self
                    .performance_counters
                    .stationary_suppressed_frames
                    .saturating_add(1);
            }
            if self.last_freshness != Some(freshness) {
                self.last_freshness = Some(freshness);
                return Ok(PreviewTick::MetadataChanged(freshness));
            }
            return Ok(PreviewTick::Unchanged);
        }
        let frame = ActualMapFrame {
            view,
            freshness,
            generation: sample.source_connection_generation(),
            sequence: sample.sequence(),
        };
        self.last_frame_key = Some(key);
        self.last_frame_view = Some(view);
        self.last_freshness = Some(freshness);
        Ok(PreviewTick::Present(frame))
    }
}

fn next_freshness_check_ms(
    now_monotonic_ms: u64,
    freshness: Freshness,
    current_age_ms: u64,
) -> Option<u64> {
    let remaining = match freshness {
        Freshness::Live => 1_501_u64.saturating_sub(current_age_ms),
        Freshness::Delayed => 5_001_u64.saturating_sub(current_age_ms),
        Freshness::Stale | Freshness::Offline => return None,
    };
    Some(now_monotonic_ms.saturating_add(remaining))
}

const fn visibility_action_for_freshness(freshness: Freshness) -> PreviewVisibilityAction {
    match freshness {
        Freshness::Live | Freshness::Delayed => PreviewVisibilityAction::Show,
        Freshness::Stale | Freshness::Offline => PreviewVisibilityAction::Hide,
    }
}
