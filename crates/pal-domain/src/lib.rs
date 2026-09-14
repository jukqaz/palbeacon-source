mod position;
mod position_source_policy;
mod settings;
mod view;
mod window;

use std::error::Error;
use std::fmt;

pub use position::{
    Freshness, PositionSample, PositionValidationError, SampleClock, classify_freshness,
    shortest_angle_lerp,
};
pub use position_source_policy::{
    PositionSourceAvailability, PositionSourceDecision, PositionSourceKind, PositionSourcePolicy,
};
pub use settings::{
    DisplayMode, EGG_LAYER_IDS, FpsProfile, HIDDEN_UNVERIFIED_LAYER_IDS, HotkeyAction,
    HotkeyBindings, HotkeyChord, HotkeyModifiers, HotkeyValidationReason, InputMode, OverlayAction,
    OverlaySettings, PoiFilters, RESOURCE_LAYER_IDS, RotationMode, SALVAGE_LAYER_IDS,
    SettingsValidationError, TOWER_LAYER_IDS, layer_group_selected, reduce_overlay_action,
};
pub use view::{ExpandedMapView, MiniMapView, ViewValidationError};
pub use window::{WindowSnapshot, WindowValidationError};

pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct CoreState {
    settings: OverlaySettings,
    settings_version: u64,
    window_snapshot: Option<WindowSnapshot>,
    position_sample: Option<PositionSample>,
    freshness: Freshness,
    heading_available: bool,
    connected: bool,
    visible: bool,
    interpolate_position: bool,
    mini_map_view: MiniMapView,
    expanded_map_view: ExpandedMapView,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoreStateParts {
    pub settings: OverlaySettings,
    pub settings_version: u64,
    pub window_snapshot: Option<WindowSnapshot>,
    pub position_sample: Option<PositionSample>,
    pub freshness: Freshness,
    pub heading_available: bool,
    pub connected: bool,
    pub visible: bool,
    pub interpolate_position: bool,
    pub mini_map_view: MiniMapView,
    pub expanded_map_view: ExpandedMapView,
}

impl CoreState {
    pub fn new(
        settings: OverlaySettings,
        window_snapshot: Option<WindowSnapshot>,
        position_sample: Option<PositionSample>,
        freshness: Freshness,
        heading_available: bool,
    ) -> Result<Self, CoreStateValidationError> {
        settings
            .validate()
            .map_err(CoreStateValidationError::Settings)?;
        let connected = freshness != Freshness::Offline;
        let interpolate_position =
            position_sample.is_some() && matches!(freshness, Freshness::Live | Freshness::Delayed);
        let visible = settings.enabled;
        let mini_map_view = MiniMapView::new(0.0, 0.0, settings.zoom)
            .expect("validated settings zoom produces a valid minimap view");
        let expanded_map_view = ExpandedMapView::new(0.0, 0.0, settings.zoom)
            .expect("validated settings zoom produces a valid expanded-map view");
        Self::from_parts(CoreStateParts {
            settings,
            settings_version: 1,
            window_snapshot,
            position_sample,
            freshness,
            heading_available,
            connected,
            visible,
            interpolate_position,
            mini_map_view,
            expanded_map_view,
        })
    }

    pub fn from_parts(parts: CoreStateParts) -> Result<Self, CoreStateValidationError> {
        parts
            .settings
            .validate()
            .map_err(CoreStateValidationError::Settings)?;
        if parts.settings_version == 0 {
            return Err(CoreStateValidationError::SettingsVersionZero);
        }
        if parts.connected == (parts.freshness == Freshness::Offline) {
            return Err(CoreStateValidationError::ConnectionFreshnessMismatch);
        }
        let fresh_or_delayed = matches!(parts.freshness, Freshness::Live | Freshness::Delayed);
        if fresh_or_delayed && parts.position_sample.is_none() {
            return Err(CoreStateValidationError::FreshnessPositionMissing);
        }
        let interpolation_eligible =
            parts.connected && parts.position_sample.is_some() && fresh_or_delayed;
        if parts.interpolate_position != interpolation_eligible {
            return Err(CoreStateValidationError::InterpolationStateInvalid);
        }
        if parts.heading_available
            && parts
                .position_sample
                .as_ref()
                .is_none_or(|position| position.heading_degrees().is_none())
        {
            return Err(CoreStateValidationError::HeadingAvailabilityInvalid);
        }

        Ok(Self {
            settings: parts.settings,
            settings_version: parts.settings_version,
            window_snapshot: parts.window_snapshot,
            position_sample: parts.position_sample,
            freshness: parts.freshness,
            heading_available: parts.heading_available,
            connected: parts.connected,
            visible: parts.visible,
            interpolate_position: parts.interpolate_position,
            mini_map_view: parts.mini_map_view,
            expanded_map_view: parts.expanded_map_view,
        })
    }

    pub const fn settings(&self) -> &OverlaySettings {
        &self.settings
    }

    pub const fn settings_version(&self) -> u64 {
        self.settings_version
    }

    pub const fn window_snapshot(&self) -> Option<&WindowSnapshot> {
        self.window_snapshot.as_ref()
    }

    pub const fn position_sample(&self) -> Option<&PositionSample> {
        self.position_sample.as_ref()
    }

    pub const fn freshness(&self) -> Freshness {
        self.freshness
    }

    pub const fn heading_available(&self) -> bool {
        self.heading_available
    }

    pub const fn connected(&self) -> bool {
        self.connected
    }

    pub const fn visible(&self) -> bool {
        self.visible
    }

    pub const fn interpolate_position(&self) -> bool {
        self.interpolate_position
    }

    pub const fn mini_map_view(&self) -> &MiniMapView {
        &self.mini_map_view
    }

    pub const fn expanded_map_view(&self) -> &ExpandedMapView {
        &self.expanded_map_view
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreStateValidationError {
    Settings(SettingsValidationError),
    SettingsVersionZero,
    ConnectionFreshnessMismatch,
    FreshnessPositionMissing,
    InterpolationStateInvalid,
    HeadingAvailabilityInvalid,
}

impl CoreStateValidationError {
    pub const fn field(self) -> &'static str {
        match self {
            Self::Settings(error) => error.field(),
            Self::SettingsVersionZero => "settings_version",
            Self::ConnectionFreshnessMismatch => "freshness",
            Self::FreshnessPositionMissing => "position_sample",
            Self::InterpolationStateInvalid => "interpolate_position",
            Self::HeadingAvailabilityInvalid => "heading_available",
        }
    }
}

impl fmt::Display for CoreStateValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Settings(error) => error.fmt(formatter),
            Self::SettingsVersionZero => write!(formatter, "settings_version must be nonzero"),
            Self::ConnectionFreshnessMismatch => {
                write!(formatter, "freshness must agree with connected")
            }
            Self::FreshnessPositionMissing => {
                write!(
                    formatter,
                    "live or delayed freshness requires a position sample"
                )
            }
            Self::InterpolationStateInvalid => write!(
                formatter,
                "interpolate_position must exactly match runtime interpolation eligibility"
            ),
            Self::HeadingAvailabilityInvalid => write!(
                formatter,
                "heading_available requires a position sample with heading"
            ),
        }
    }
}

impl Error for CoreStateValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Settings(error) => Some(error),
            Self::SettingsVersionZero => None,
            Self::ConnectionFreshnessMismatch
            | Self::FreshnessPositionMissing
            | Self::InterpolationStateInvalid
            | Self::HeadingAvailabilityInvalid => None,
        }
    }
}
