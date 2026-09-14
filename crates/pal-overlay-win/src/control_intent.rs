use pal_domain::{HotkeyAction, InputMode, OverlayAction, OverlaySettings, PoiFilters};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlIntent {
    Action {
        expected_settings_version: u64,
        action: OverlayAction,
    },
    ReplacePoiFilters {
        expected_settings_version: u64,
        filters: PoiFilters,
    },
}

impl ControlIntent {
    pub const fn action(&self) -> Option<OverlayAction> {
        match self {
            Self::Action { action, .. } => Some(*action),
            Self::ReplacePoiFilters { .. } => None,
        }
    }

    pub const fn expected_settings_version(&self) -> u64 {
        match self {
            Self::Action {
                expected_settings_version,
                ..
            } => *expected_settings_version,
            Self::ReplacePoiFilters {
                expected_settings_version,
                ..
            } => *expected_settings_version,
        }
    }
}

pub const fn control_intent_for_hotkey(
    action: HotkeyAction,
    settings: &OverlaySettings,
    expected_settings_version: u64,
) -> ControlIntent {
    let action = match action {
        HotkeyAction::OverlayVisibility => OverlayAction::ToggleVisibility,
        HotkeyAction::RotationToggle => OverlayAction::ToggleRotation,
        HotkeyAction::TemporaryInteraction => match settings.input_mode {
            InputMode::Locked => OverlayAction::EnterInteractive,
            InputMode::TemporaryInteractive
            | InputMode::PinnedInteractive
            | InputMode::LayoutEdit => OverlayAction::Lock,
        },
        HotkeyAction::InteractionLock => OverlayAction::Lock,
    };
    ControlIntent::Action {
        expected_settings_version,
        action,
    }
}

pub trait ControlIntentSink {
    type Error;

    fn submit(&self, intent: ControlIntent) -> Result<(), Self::Error>;
}
