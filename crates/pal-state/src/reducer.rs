//! Sole mutable overlay state owner.

use std::sync::Arc;

use pal_domain::{
    CoreState, CoreStateParts, DisplayMode, ExpandedMapView, Freshness, MiniMapView,
    OverlaySettings, PositionSample, SettingsValidationError, ViewValidationError, WindowSnapshot,
    classify_freshness,
};
use thiserror::Error;

use crate::PositionSourceEvent;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReducerDiagnostics {
    pub accepted_samples: u64,
    pub duplicate_drops: u64,
    pub out_of_order_drops: u64,
    pub gap_events: u64,
    pub missing_sequences: u64,
    pub old_generation_drops: u64,
    pub same_generation_boot_mismatch_drops: u64,
    pub invalid_transition_drops: u64,
    pub clock_invalid_events: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    PositionSource(PositionSourceEvent),
    SettingsApplied {
        version: u64,
        settings: OverlaySettings,
    },
    WindowChanged(Option<WindowSnapshot>),
    SetVisible(bool),
    SetDisplayMode(DisplayMode),
    SetMiniMapView(MiniMapView),
    ExpandedPanZoom {
        center_x: f64,
        center_y: f64,
        zoom: f32,
    },
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ReducerError {
    #[error("settings version must be nonzero")]
    SettingsVersionZero,
    #[error("settings version {candidate} is not newer than {current}")]
    SettingsVersionNotNewer { current: u64, candidate: u64 },
    #[error("settings version is exhausted")]
    SettingsVersionOverflow,
    #[error("settings validation failed: {0}")]
    Settings(#[source] SettingsValidationError),
    #[error("view validation failed: {0}")]
    View(#[source] ViewValidationError),
}

impl ReducerError {
    pub const fn field(self) -> Option<&'static str> {
        match self {
            Self::SettingsVersionZero | Self::SettingsVersionNotNewer { .. } => {
                Some("settings_version")
            }
            Self::SettingsVersionOverflow => Some("settings_version"),
            Self::Settings(error) => Some(error.field()),
            Self::View(error) => Some(error.field()),
        }
    }
}

pub struct StateReducer {
    settings: OverlaySettings,
    settings_version: u64,
    window: Option<WindowSnapshot>,
    visible: bool,
    generation: u64,
    connected: bool,
    active_boot_id: Option<Vec<u8>>,
    last_sequence: Option<u64>,
    last_position: Option<PositionSample>,
    clock_valid: bool,
    heading_available: bool,
    mini_map_view: MiniMapView,
    expanded_map_view: ExpandedMapView,
    diagnostics: ReducerDiagnostics,
}

impl Default for StateReducer {
    fn default() -> Self {
        let settings = OverlaySettings::default();
        let mini_map_view = MiniMapView::new(0.0, 0.0, settings.zoom)
            .expect("default settings produce a valid minimap view");
        let expanded_map_view = ExpandedMapView::new(0.0, 0.0, settings.zoom)
            .expect("default settings produce a valid expanded-map view");
        Self {
            visible: settings.enabled,
            settings,
            settings_version: 1,
            window: None,
            generation: 0,
            connected: false,
            active_boot_id: None,
            last_sequence: None,
            last_position: None,
            clock_valid: false,
            heading_available: false,
            mini_map_view,
            expanded_map_view,
            diagnostics: ReducerDiagnostics::default(),
        }
    }
}

impl StateReducer {
    pub fn apply(&mut self, event: Event) -> Result<(), ReducerError> {
        match event {
            Event::PositionSource(event) => {
                self.apply_position_source(event);
                Ok(())
            }
            Event::SettingsApplied { version, settings } => self.apply_settings(version, settings),
            Event::WindowChanged(window) => {
                self.window = window;
                Ok(())
            }
            Event::SetVisible(visible) => {
                self.visible = visible;
                Ok(())
            }
            Event::SetDisplayMode(mode) => self.set_display_mode(mode),
            Event::SetMiniMapView(view) => {
                self.mini_map_view = view;
                Ok(())
            }
            Event::ExpandedPanZoom {
                center_x,
                center_y,
                zoom,
            } => {
                let candidate =
                    ExpandedMapView::new(center_x, center_y, zoom).map_err(ReducerError::View)?;
                self.expanded_map_view = candidate;
                Ok(())
            }
        }
    }

    pub fn core_state(&self, now_monotonic_ms: u64) -> Arc<CoreState> {
        let freshness = self.freshness(now_monotonic_ms);
        let interpolate_position = self.connected
            && self.clock_valid
            && self.last_position.is_some()
            && matches!(freshness, Freshness::Live | Freshness::Delayed);
        let parts = CoreStateParts {
            settings: self.settings.clone(),
            settings_version: self.settings_version,
            window_snapshot: self.window.clone(),
            position_sample: self.last_position.clone(),
            freshness,
            heading_available: self.heading_available,
            connected: self.connected,
            visible: self.visible,
            interpolate_position,
            mini_map_view: self.mini_map_view.clone(),
            expanded_map_view: self.expanded_map_view.clone(),
        };
        Arc::new(CoreState::from_parts(parts).expect("reducer state always satisfies domain rules"))
    }

    pub const fn diagnostics(&self) -> &ReducerDiagnostics {
        &self.diagnostics
    }

    pub const fn settings(&self) -> &OverlaySettings {
        &self.settings
    }

    pub const fn settings_version(&self) -> u64 {
        self.settings_version
    }

    pub const fn last_position(&self) -> Option<&PositionSample> {
        self.last_position.as_ref()
    }

    pub const fn has_trusted_position(&self) -> bool {
        self.connected && self.clock_valid && self.last_position.is_some()
    }

    pub const fn heading_available(&self) -> bool {
        self.heading_available
    }

    pub const fn minimap_view(&self) -> &MiniMapView {
        &self.mini_map_view
    }

    pub const fn expanded_map_view(&self) -> &ExpandedMapView {
        &self.expanded_map_view
    }

    pub fn freshness(&self, now_monotonic_ms: u64) -> Freshness {
        if !self.connected {
            return Freshness::Offline;
        }
        if !self.clock_valid {
            return Freshness::Stale;
        }
        let Some(position) = self.last_position.as_ref() else {
            return Freshness::Stale;
        };
        classify_freshness(position.clock().age_upper_bound_ms(now_monotonic_ms), true)
    }

    fn apply_settings(
        &mut self,
        version: u64,
        settings: OverlaySettings,
    ) -> Result<(), ReducerError> {
        settings.validate().map_err(ReducerError::Settings)?;
        if version == 0 {
            return Err(ReducerError::SettingsVersionZero);
        }
        if version <= self.settings_version {
            return Err(ReducerError::SettingsVersionNotNewer {
                current: self.settings_version,
                candidate: version,
            });
        }

        let mini_map_view = MiniMapView::new(
            self.mini_map_view.center_x(),
            self.mini_map_view.center_y(),
            settings.zoom,
        )
        .map_err(ReducerError::View)?;
        let expanded_map_view = self.display_transition_view(
            self.settings.display_mode,
            settings.display_mode,
            &mini_map_view,
        )?;

        self.visible = settings.enabled;
        self.settings = settings;
        self.settings_version = version;
        self.mini_map_view = mini_map_view;
        self.expanded_map_view = expanded_map_view;
        Ok(())
    }

    fn set_display_mode(&mut self, mode: DisplayMode) -> Result<(), ReducerError> {
        let old_mode = self.settings.display_mode;
        if old_mode == mode {
            return Ok(());
        }
        let version = self
            .settings_version
            .checked_add(1)
            .ok_or(ReducerError::SettingsVersionOverflow)?;
        let expanded_map_view =
            self.display_transition_view(old_mode, mode, &self.mini_map_view)?;
        let mut settings = self.settings.clone();
        settings.display_mode = mode;
        settings.validate().map_err(ReducerError::Settings)?;

        self.settings = settings;
        self.settings_version = version;
        self.expanded_map_view = expanded_map_view;
        Ok(())
    }

    fn display_transition_view(
        &self,
        old_mode: DisplayMode,
        new_mode: DisplayMode,
        mini_map_view: &MiniMapView,
    ) -> Result<ExpandedMapView, ReducerError> {
        if old_mode == new_mode {
            return Ok(self.expanded_map_view.clone());
        }
        ExpandedMapView::new(
            mini_map_view.center_x(),
            mini_map_view.center_y(),
            self.expanded_map_view.zoom(),
        )
        .map_err(ReducerError::View)
    }

    fn apply_position_source(&mut self, event: PositionSourceEvent) {
        match event {
            PositionSourceEvent::Connected { generation } => self.apply_connected(generation),
            PositionSourceEvent::Sample(sample) => self.apply_sample(sample),
            PositionSourceEvent::Unavailable { generation } => self.apply_unavailable(generation),
            PositionSourceEvent::Disconnected { generation } => self.apply_disconnected(generation),
            PositionSourceEvent::ClockInvalid { generation, .. } => {
                self.apply_clock_invalid(generation)
            }
        }
    }

    fn apply_unavailable(&mut self, generation: u64) {
        if generation == 0 {
            increment(&mut self.diagnostics.invalid_transition_drops);
        } else if generation < self.generation {
            increment(&mut self.diagnostics.old_generation_drops);
        } else if generation != self.generation || !self.connected {
            increment(&mut self.diagnostics.invalid_transition_drops);
        } else {
            self.last_position = None;
            self.clock_valid = false;
            self.heading_available = false;
        }
    }

    fn apply_connected(&mut self, generation: u64) {
        if generation == 0 {
            increment(&mut self.diagnostics.invalid_transition_drops);
        } else if generation < self.generation {
            increment(&mut self.diagnostics.old_generation_drops);
        } else if generation > self.generation {
            self.generation = generation;
            self.connected = true;
            self.active_boot_id = None;
            self.last_sequence = None;
            self.last_position = None;
            self.clock_valid = false;
            self.heading_available = false;
        }
    }

    fn apply_sample(&mut self, sample: PositionSample) {
        let generation = sample.source_connection_generation();
        if generation == 0 || sample.sequence() == 0 {
            increment(&mut self.diagnostics.invalid_transition_drops);
            return;
        }
        if generation < self.generation {
            increment(&mut self.diagnostics.old_generation_drops);
            return;
        }
        if generation > self.generation || !self.connected {
            increment(&mut self.diagnostics.invalid_transition_drops);
            return;
        }

        if let Some(boot_id) = self.active_boot_id.as_deref()
            && boot_id != sample.agent_boot_id()
        {
            increment(&mut self.diagnostics.same_generation_boot_mismatch_drops);
            return;
        }

        if let Some(previous) = self.last_sequence {
            if sample.sequence() == previous {
                increment(&mut self.diagnostics.duplicate_drops);
                return;
            }
            if sample.sequence() < previous {
                increment(&mut self.diagnostics.out_of_order_drops);
                return;
            }
            if sample.sequence() > previous.saturating_add(1) {
                increment(&mut self.diagnostics.gap_events);
                self.diagnostics.missing_sequences = self
                    .diagnostics
                    .missing_sequences
                    .saturating_add(sample.sequence().saturating_sub(previous).saturating_sub(1));
            }
        }

        if self.active_boot_id.is_none() {
            self.active_boot_id = Some(sample.agent_boot_id().to_vec());
        }
        self.last_sequence = Some(sample.sequence());
        self.clock_valid = true;
        self.heading_available = sample.heading_degrees().is_some();
        self.last_position = Some(sample);
        increment(&mut self.diagnostics.accepted_samples);
    }

    fn apply_disconnected(&mut self, generation: u64) {
        if generation == 0 {
            increment(&mut self.diagnostics.invalid_transition_drops);
        } else if generation < self.generation {
            increment(&mut self.diagnostics.old_generation_drops);
        } else if generation > self.generation {
            increment(&mut self.diagnostics.invalid_transition_drops);
        } else if self.connected {
            self.connected = false;
            self.clock_valid = false;
            self.heading_available = false;
        }
    }

    fn apply_clock_invalid(&mut self, generation: u64) {
        if generation == 0 {
            increment(&mut self.diagnostics.invalid_transition_drops);
        } else if generation < self.generation {
            increment(&mut self.diagnostics.old_generation_drops);
        } else if generation > self.generation || !self.connected {
            increment(&mut self.diagnostics.invalid_transition_drops);
        } else {
            self.clock_valid = false;
            self.heading_available = false;
            increment(&mut self.diagnostics.clock_invalid_events);
        }
    }
}

fn increment(counter: &mut u64) {
    *counter = counter.saturating_add(1);
}
