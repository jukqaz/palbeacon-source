use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotationMode {
    NorthUp,
    HeadingUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Locked,
    TemporaryInteractive,
    PinnedInteractive,
    LayoutEdit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    MiniMap,
    ExpandedMap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayAction {
    Show,
    Hide,
    ToggleVisibility,
    EnterInteractive,
    ExitInteractive,
    OpenExpanded,
    CloseExpanded,
    ZoomIn,
    ZoomOut,
    ToggleRotation,
    Lock,
    ToggleFastTravel,
    ToggleBoss,
    ToggleWanted,
    ToggleDungeon,
    ToggleTower,
    ToggleEgg,
    ToggleResources,
    ToggleSalvage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpsProfile {
    Auto,
    Thirty,
    Sixty,
}

const DEFAULT_OVERLAY_LAYER_IDS: [&str; 3] = ["map-unlock", "poi", "tower"];
pub const TOWER_LAYER_IDS: [&str; 1] = ["tower"];
pub const EGG_LAYER_IDS: [&str; 1] = ["egg"];
pub const SALVAGE_LAYER_IDS: [&str; 2] = ["salvage-rank-1", "salvage-rank-2"];
pub const HIDDEN_UNVERIFIED_LAYER_IDS: [&str; 6] = [
    "respawn-point",
    "captured-pal",
    "anti-air-turret",
    "medal",
    "ancient-shrine",
    "healing-spring",
];
pub const RESOURCE_LAYER_IDS: [&str; 12] = [
    "oil-field",
    "soralite",
    "chromite",
    "hexolite-quartz",
    "ancient-bark",
    "ancient-lava",
    "ore-copper",
    "ore-coal",
    "ore-quartz",
    "ore-sulfur",
    "ore-crystal",
    "paloxite",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoiFilters {
    pub fast_travel: bool,
    pub boss: bool,
    pub wanted: bool,
    pub dungeon: bool,
    pub enabled_layer_ids: Vec<String>,
    pub selected_pal_ids: Vec<String>,
    pub night_only: bool,
}

impl Default for PoiFilters {
    fn default() -> Self {
        Self {
            fast_travel: true,
            boss: true,
            wanted: false,
            dungeon: true,
            enabled_layer_ids: DEFAULT_OVERLAY_LAYER_IDS
                .into_iter()
                .map(str::to_owned)
                .collect(),
            selected_pal_ids: Vec::new(),
            night_only: false,
        }
    }
}

impl PoiFilters {
    pub fn remove_hidden_unverified_layers(&mut self) -> bool {
        let previous_len = self.enabled_layer_ids.len();
        self.enabled_layer_ids
            .retain(|layer_id| !HIDDEN_UNVERIFIED_LAYER_IDS.contains(&layer_id.as_str()));
        self.enabled_layer_ids.len() != previous_len
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HotkeyModifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub windows: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyChord {
    pub modifiers: HotkeyModifiers,
    pub virtual_key: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HotkeyBindings {
    pub overlay_visibility: Option<HotkeyChord>,
    pub rotation_toggle: Option<HotkeyChord>,
    pub temporary_interaction: Option<HotkeyChord>,
    pub interaction_lock: Option<HotkeyChord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    OverlayVisibility,
    RotationToggle,
    TemporaryInteraction,
    InteractionLock,
}

impl HotkeyAction {
    const fn field(self) -> &'static str {
        match self {
            Self::OverlayVisibility => "hotkey_bindings.overlay_visibility",
            Self::RotationToggle => "hotkey_bindings.rotation_toggle",
            Self::TemporaryInteraction => "hotkey_bindings.temporary_interaction",
            Self::InteractionLock => "hotkey_bindings.interaction_lock",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyValidationReason {
    ZeroVirtualKey,
    UnmodifiedK,
    UnmodifiedGameKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsValidationError {
    OpacityOutOfRange,
    DiameterOutOfRange,
    ZoomOutOfRange,
    NormalizedXOutOfRange,
    NormalizedYOutOfRange,
    InvalidHotkey {
        action: HotkeyAction,
        reason: HotkeyValidationReason,
    },
    TooManyEnabledLayers,
    TooManySelectedPals,
    HiddenUnverifiedMapFilterId,
    InvalidMapFilterId,
    DuplicateMapFilterId,
}

impl SettingsValidationError {
    pub const fn field(self) -> &'static str {
        match self {
            Self::OpacityOutOfRange => "opacity",
            Self::DiameterOutOfRange => "diameter_px",
            Self::ZoomOutOfRange => "zoom",
            Self::NormalizedXOutOfRange => "normalized_x",
            Self::NormalizedYOutOfRange => "normalized_y",
            Self::InvalidHotkey { action, .. } => action.field(),
            Self::TooManyEnabledLayers
            | Self::TooManySelectedPals
            | Self::HiddenUnverifiedMapFilterId
            | Self::InvalidMapFilterId
            | Self::DuplicateMapFilterId => "poi_filters",
        }
    }
}

impl fmt::Display for SettingsValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHotkey { reason, .. } => match reason {
                HotkeyValidationReason::ZeroVirtualKey => {
                    write!(formatter, "{} virtual key must be nonzero", self.field())
                }
                HotkeyValidationReason::UnmodifiedK => {
                    write!(formatter, "{} must not be unmodified K", self.field())
                }
                HotkeyValidationReason::UnmodifiedGameKey => {
                    write!(
                        formatter,
                        "{} alphanumeric keys require a modifier",
                        self.field()
                    )
                }
            },
            _ => write!(formatter, "{} is outside its allowed range", self.field()),
        }
    }
}

impl Error for SettingsValidationError {}

#[derive(Clone, Debug, PartialEq)]
pub struct OverlaySettings {
    pub enabled: bool,
    pub auto_show: bool,
    pub rotation_mode: RotationMode,
    pub input_mode: InputMode,
    pub display_mode: DisplayMode,
    pub opacity: f32,
    pub diameter_px: u32,
    pub zoom: f32,
    pub normalized_x: f32,
    pub normalized_y: f32,
    pub fps_profile: FpsProfile,
    pub poi_filters: PoiFilters,
    pub hotkey_bindings: HotkeyBindings,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_show: true,
            rotation_mode: RotationMode::NorthUp,
            input_mode: InputMode::Locked,
            display_mode: DisplayMode::MiniMap,
            opacity: 0.90,
            diameter_px: 320,
            zoom: 1.0,
            normalized_x: 0.85,
            normalized_y: 0.20,
            fps_profile: FpsProfile::Auto,
            poi_filters: PoiFilters::default(),
            hotkey_bindings: HotkeyBindings::default(),
        }
    }
}

impl OverlaySettings {
    pub fn validate(&self) -> Result<(), SettingsValidationError> {
        if !(0.20..=1.00).contains(&self.opacity) {
            return Err(SettingsValidationError::OpacityOutOfRange);
        }
        if !(180..=640).contains(&self.diameter_px) {
            return Err(SettingsValidationError::DiameterOutOfRange);
        }
        if !(0.50..=4.00).contains(&self.zoom) {
            return Err(SettingsValidationError::ZoomOutOfRange);
        }
        if !(0.0..=1.0).contains(&self.normalized_x) {
            return Err(SettingsValidationError::NormalizedXOutOfRange);
        }
        if !(0.0..=1.0).contains(&self.normalized_y) {
            return Err(SettingsValidationError::NormalizedYOutOfRange);
        }

        for (action, chord) in [
            (
                HotkeyAction::OverlayVisibility,
                self.hotkey_bindings.overlay_visibility,
            ),
            (
                HotkeyAction::RotationToggle,
                self.hotkey_bindings.rotation_toggle,
            ),
            (
                HotkeyAction::TemporaryInteraction,
                self.hotkey_bindings.temporary_interaction,
            ),
            (
                HotkeyAction::InteractionLock,
                self.hotkey_bindings.interaction_lock,
            ),
        ] {
            validate_hotkey(action, chord)?;
        }

        validate_map_filter_ids(
            &self.poi_filters.enabled_layer_ids,
            64,
            SettingsValidationError::TooManyEnabledLayers,
        )?;
        if self
            .poi_filters
            .enabled_layer_ids
            .iter()
            .any(|layer_id| HIDDEN_UNVERIFIED_LAYER_IDS.contains(&layer_id.as_str()))
        {
            return Err(SettingsValidationError::HiddenUnverifiedMapFilterId);
        }
        validate_map_filter_ids(
            &self.poi_filters.selected_pal_ids,
            8,
            SettingsValidationError::TooManySelectedPals,
        )?;

        Ok(())
    }
}

pub fn reduce_overlay_action(current: &OverlaySettings, action: OverlayAction) -> OverlaySettings {
    let mut next = current.clone();
    match action {
        OverlayAction::Show => next.enabled = true,
        OverlayAction::Hide => next.enabled = false,
        OverlayAction::ToggleVisibility => next.enabled = !next.enabled,
        OverlayAction::EnterInteractive => {
            next.input_mode = InputMode::PinnedInteractive;
        }
        OverlayAction::ExitInteractive | OverlayAction::Lock => {
            next.input_mode = InputMode::Locked;
        }
        OverlayAction::OpenExpanded => next.display_mode = DisplayMode::ExpandedMap,
        OverlayAction::CloseExpanded => {
            next.display_mode = DisplayMode::MiniMap;
            next.input_mode = InputMode::Locked;
        }
        OverlayAction::ZoomIn => next.zoom = (next.zoom + 0.25).min(4.0),
        OverlayAction::ZoomOut => next.zoom = (next.zoom - 0.25).max(0.5),
        OverlayAction::ToggleRotation => {
            next.rotation_mode = match next.rotation_mode {
                RotationMode::NorthUp => RotationMode::HeadingUp,
                RotationMode::HeadingUp => RotationMode::NorthUp,
            };
        }
        OverlayAction::ToggleFastTravel => {
            next.poi_filters.fast_travel = !next.poi_filters.fast_travel;
        }
        OverlayAction::ToggleBoss => next.poi_filters.boss = !next.poi_filters.boss,
        OverlayAction::ToggleWanted => next.poi_filters.wanted = !next.poi_filters.wanted,
        OverlayAction::ToggleDungeon => next.poi_filters.dungeon = !next.poi_filters.dungeon,
        OverlayAction::ToggleTower => {
            toggle_layer_group(&mut next.poi_filters.enabled_layer_ids, &TOWER_LAYER_IDS);
        }
        OverlayAction::ToggleEgg => {
            toggle_layer_group(&mut next.poi_filters.enabled_layer_ids, &EGG_LAYER_IDS);
        }
        OverlayAction::ToggleResources => {
            toggle_layer_group(&mut next.poi_filters.enabled_layer_ids, &RESOURCE_LAYER_IDS);
        }
        OverlayAction::ToggleSalvage => {
            toggle_layer_group(&mut next.poi_filters.enabled_layer_ids, &SALVAGE_LAYER_IDS);
        }
    }
    next
}

pub fn layer_group_selected(enabled_layer_ids: &[String], group: &[&str]) -> bool {
    group
        .iter()
        .any(|layer_id| enabled_layer_ids.iter().any(|enabled| enabled == layer_id))
}

fn toggle_layer_group(enabled_layer_ids: &mut Vec<String>, group: &[&str]) {
    if layer_group_selected(enabled_layer_ids, group) {
        enabled_layer_ids.retain(|enabled| !group.contains(&enabled.as_str()));
    } else {
        enabled_layer_ids.extend(group.iter().map(|layer_id| (*layer_id).to_owned()));
        enabled_layer_ids.sort_unstable();
        enabled_layer_ids.dedup();
    }
}

fn validate_map_filter_ids(
    ids: &[String],
    maximum: usize,
    too_many: SettingsValidationError,
) -> Result<(), SettingsValidationError> {
    if ids.len() > maximum {
        return Err(too_many);
    }
    for (index, id) in ids.iter().enumerate() {
        if id.is_empty()
            || id.len() > 96
            || !id
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'_' | b'-' | b':'))
        {
            return Err(SettingsValidationError::InvalidMapFilterId);
        }
        if ids[..index].iter().any(|previous| previous == id) {
            return Err(SettingsValidationError::DuplicateMapFilterId);
        }
    }
    Ok(())
}

fn validate_hotkey(
    action: HotkeyAction,
    chord: Option<HotkeyChord>,
) -> Result<(), SettingsValidationError> {
    let Some(chord) = chord else {
        return Ok(());
    };

    if chord.virtual_key == 0 {
        return Err(SettingsValidationError::InvalidHotkey {
            action,
            reason: HotkeyValidationReason::ZeroVirtualKey,
        });
    }

    let modifiers = chord.modifiers;
    let has_modifier = modifiers.control || modifiers.alt || modifiers.shift || modifiers.windows;
    if chord.virtual_key == 0x4B && !has_modifier {
        return Err(SettingsValidationError::InvalidHotkey {
            action,
            reason: HotkeyValidationReason::UnmodifiedK,
        });
    }
    if !has_modifier && matches!(chord.virtual_key, 0x30..=0x39 | 0x41..=0x5A) {
        return Err(SettingsValidationError::InvalidHotkey {
            action,
            reason: HotkeyValidationReason::UnmodifiedGameKey,
        });
    }

    Ok(())
}
