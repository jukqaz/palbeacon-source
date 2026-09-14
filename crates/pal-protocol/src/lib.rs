//! Bounded local Pal Companion protocol contract.

mod framing;

use std::error::Error;
use std::fmt;

use pal_domain::{
    DisplayMode as DomainDisplayMode, FpsProfile as DomainFpsProfile, Freshness as DomainFreshness,
    HotkeyBindings as DomainHotkeyBindings, HotkeyChord as DomainHotkeyChord,
    HotkeyModifiers as DomainHotkeyModifiers, InputMode as DomainInputMode,
    OverlayAction as DomainOverlayAction, OverlaySettings as DomainOverlaySettings,
    PoiFilters as DomainPoiFilters, PositionSample, PositionValidationError,
    RotationMode as DomainRotationMode, SampleClock, SettingsValidationError,
    WindowSnapshot as DomainWindowSnapshot, WindowValidationError,
};

pub use framing::{FrameCodec, FrameError, MAX_FRAME_LEN, MAX_PAYLOAD_LEN};
pub use pal_domain::PROTOCOL_VERSION;

pub mod v2 {
    pub use pal_wire::v2::*;
    include!(concat!(env!("OUT_DIR"), "/palcompanion.v2.rs"));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionValue {
    Missing,
    Empty,
    Enum(i32),
    U32(u32),
    F32Bits(u32),
    F64Bits(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainValidationReason {
    Settings(SettingsValidationError),
    Window(WindowValidationError),
    Position(PositionValidationError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainConversionError {
    MissingMessage {
        field: &'static str,
    },
    InvalidEnum {
        field: &'static str,
        value: i32,
    },
    OutOfRangeU32 {
        field: &'static str,
        value: u32,
    },
    InvalidDomainValue {
        field: &'static str,
        value: ConversionValue,
        reason: DomainValidationReason,
    },
}

impl DomainConversionError {
    pub const fn field(self) -> &'static str {
        match self {
            Self::MissingMessage { field }
            | Self::InvalidEnum { field, .. }
            | Self::OutOfRangeU32 { field, .. }
            | Self::InvalidDomainValue { field, .. } => field,
        }
    }

    pub const fn value(self) -> ConversionValue {
        match self {
            Self::MissingMessage { .. } => ConversionValue::Missing,
            Self::InvalidEnum { value, .. } => ConversionValue::Enum(value),
            Self::OutOfRangeU32 { value, .. } => ConversionValue::U32(value),
            Self::InvalidDomainValue { value, .. } => value,
        }
    }
}

impl fmt::Display for DomainConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingMessage { field } => write!(formatter, "{field} is required"),
            Self::InvalidEnum { field, value } => {
                write!(formatter, "{field} has unsupported enum value {value}")
            }
            Self::OutOfRangeU32 { field, value } => {
                write!(formatter, "{field} value {value} is out of range")
            }
            Self::InvalidDomainValue {
                field,
                value,
                reason,
            } => write!(
                formatter,
                "{field} value {value:?} failed domain validation: {reason:?}"
            ),
        }
    }
}

impl Error for DomainConversionError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalValidationError {
    MissingPayload,
    InvalidLength {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
}

impl LocalValidationError {
    pub const fn field(self) -> &'static str {
        match self {
            Self::MissingPayload => "payload",
            Self::InvalidLength { field, .. } => field,
        }
    }
}

impl fmt::Display for LocalValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPayload => write!(formatter, "payload is required"),
            Self::InvalidLength {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "{field} must be {expected} bytes, but was {actual} bytes"
            ),
        }
    }
}

impl Error for LocalValidationError {}

pub fn validate_local_envelope(envelope: &v2::LocalEnvelope) -> Result<(), LocalValidationError> {
    validate_len("connection_id", &envelope.connection_id, 16)?;

    let payload = envelope
        .payload
        .as_ref()
        .ok_or(LocalValidationError::MissingPayload)?;
    let trace = match payload {
        v2::local_envelope::Payload::SettingsPatch(value) => {
            Some(("settings_patch.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::SettingsAck(value) => {
            Some(("settings_ack.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::Position(value) => {
            Some(("position.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::RenderSnapshot(value) => {
            Some(("render_snapshot.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::OverlayCommand(value) => {
            Some(("overlay_command.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::OverlayCommandResult(value) => {
            Some(("overlay_command_result.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::RenderApplied(value) => {
            Some(("render_applied.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::ProductRequest(value) => {
            Some(("product_request.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::ProductResponse(value) => {
            Some(("product_response.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::SettingsQuery(value) => {
            Some(("settings_query.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::SettingsSnapshot(value) => {
            Some(("settings_snapshot.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::MapViewIntent(value) => {
            Some(("map_view_intent.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::MapViewUpdate(value) => {
            Some(("map_view_update.trace_id", value.trace_id.as_slice()))
        }
        v2::local_envelope::Payload::ClientHello(_)
        | v2::local_envelope::Payload::CoreStatus(_)
        | v2::local_envelope::Payload::Error(_) => None,
    };

    if let Some((field, value)) = trace {
        validate_len(field, value, 16)?;
    }
    Ok(())
}

fn validate_len(
    field: &'static str,
    value: &[u8],
    expected: usize,
) -> Result<(), LocalValidationError> {
    if value.len() == expected {
        Ok(())
    } else {
        Err(LocalValidationError::InvalidLength {
            field,
            expected,
            actual: value.len(),
        })
    }
}

macro_rules! domain_enum_conversion {
    ($encode:ident, $decode:ident, $wire:ty, $domain:ty, $field:literal, {
        $($wire_variant:path => $domain_variant:path),+ $(,)?
    }) => {
        pub const fn $encode(value: $domain) -> $wire {
            match value {
                $($domain_variant => $wire_variant,)+
            }
        }

        pub fn $decode(value: $wire) -> Result<$domain, DomainConversionError> {
            match value {
                <$wire>::Unspecified => Err(DomainConversionError::InvalidEnum {
                    field: $field,
                    value: value as i32,
                }),
                $($wire_variant => Ok($domain_variant),)+
            }
        }
    };
}

domain_enum_conversion!(encode_rotation_mode, decode_rotation_mode, v2::RotationMode, DomainRotationMode, "rotation_mode", {
    v2::RotationMode::NorthUp => DomainRotationMode::NorthUp,
    v2::RotationMode::HeadingUp => DomainRotationMode::HeadingUp,
});
domain_enum_conversion!(encode_input_mode, decode_input_mode, v2::InputMode, DomainInputMode, "input_mode", {
    v2::InputMode::Locked => DomainInputMode::Locked,
    v2::InputMode::TemporaryInteractive => DomainInputMode::TemporaryInteractive,
    v2::InputMode::PinnedInteractive => DomainInputMode::PinnedInteractive,
    v2::InputMode::LayoutEdit => DomainInputMode::LayoutEdit,
});
domain_enum_conversion!(encode_display_mode, decode_display_mode, v2::DisplayMode, DomainDisplayMode, "display_mode", {
    v2::DisplayMode::MiniMap => DomainDisplayMode::MiniMap,
    v2::DisplayMode::ExpandedMap => DomainDisplayMode::ExpandedMap,
});
domain_enum_conversion!(encode_fps_profile, decode_fps_profile, v2::FpsProfile, DomainFpsProfile, "fps_profile", {
    v2::FpsProfile::Auto => DomainFpsProfile::Auto,
    v2::FpsProfile::Thirty => DomainFpsProfile::Thirty,
    v2::FpsProfile::Sixty => DomainFpsProfile::Sixty,
});
domain_enum_conversion!(encode_freshness, decode_freshness, v2::Freshness, DomainFreshness, "freshness", {
    v2::Freshness::Live => DomainFreshness::Live,
    v2::Freshness::Delayed => DomainFreshness::Delayed,
    v2::Freshness::Stale => DomainFreshness::Stale,
    v2::Freshness::Offline => DomainFreshness::Offline,
});

pub const fn encode_overlay_action(value: DomainOverlayAction) -> v2::OverlayCommandKind {
    match value {
        DomainOverlayAction::Show => v2::OverlayCommandKind::Show,
        DomainOverlayAction::Hide => v2::OverlayCommandKind::Hide,
        DomainOverlayAction::ToggleVisibility => v2::OverlayCommandKind::ToggleVisibility,
        DomainOverlayAction::EnterInteractive => v2::OverlayCommandKind::EnterInteractive,
        DomainOverlayAction::ExitInteractive => v2::OverlayCommandKind::ExitInteractive,
        DomainOverlayAction::OpenExpanded => v2::OverlayCommandKind::OpenExpanded,
        DomainOverlayAction::CloseExpanded => v2::OverlayCommandKind::CloseExpanded,
        DomainOverlayAction::ZoomIn => v2::OverlayCommandKind::ZoomIn,
        DomainOverlayAction::ZoomOut => v2::OverlayCommandKind::ZoomOut,
        DomainOverlayAction::ToggleRotation => v2::OverlayCommandKind::ToggleRotation,
        DomainOverlayAction::Lock => v2::OverlayCommandKind::Lock,
        DomainOverlayAction::ToggleFastTravel => v2::OverlayCommandKind::ToggleFastTravel,
        DomainOverlayAction::ToggleBoss => v2::OverlayCommandKind::ToggleBoss,
        DomainOverlayAction::ToggleWanted => v2::OverlayCommandKind::ToggleWanted,
        DomainOverlayAction::ToggleDungeon => v2::OverlayCommandKind::ToggleDungeon,
        DomainOverlayAction::ToggleTower => v2::OverlayCommandKind::ToggleTower,
        DomainOverlayAction::ToggleEgg => v2::OverlayCommandKind::ToggleEgg,
        DomainOverlayAction::ToggleResources => v2::OverlayCommandKind::ToggleResources,
        DomainOverlayAction::ToggleSalvage => v2::OverlayCommandKind::ToggleSalvage,
    }
}

pub fn decode_overlay_action(
    value: v2::OverlayCommandKind,
) -> Result<DomainOverlayAction, DomainConversionError> {
    match value {
        v2::OverlayCommandKind::Unspecified
        | v2::OverlayCommandKind::Start
        | v2::OverlayCommandKind::Stop
        | v2::OverlayCommandKind::Restart => Err(DomainConversionError::InvalidEnum {
            field: "overlay_command.command",
            value: value as i32,
        }),
        v2::OverlayCommandKind::Show => Ok(DomainOverlayAction::Show),
        v2::OverlayCommandKind::Hide => Ok(DomainOverlayAction::Hide),
        v2::OverlayCommandKind::EnterInteractive => Ok(DomainOverlayAction::EnterInteractive),
        v2::OverlayCommandKind::ExitInteractive => Ok(DomainOverlayAction::ExitInteractive),
        v2::OverlayCommandKind::OpenExpanded => Ok(DomainOverlayAction::OpenExpanded),
        v2::OverlayCommandKind::CloseExpanded => Ok(DomainOverlayAction::CloseExpanded),
        v2::OverlayCommandKind::ZoomIn => Ok(DomainOverlayAction::ZoomIn),
        v2::OverlayCommandKind::ZoomOut => Ok(DomainOverlayAction::ZoomOut),
        v2::OverlayCommandKind::ToggleRotation => Ok(DomainOverlayAction::ToggleRotation),
        v2::OverlayCommandKind::Lock => Ok(DomainOverlayAction::Lock),
        v2::OverlayCommandKind::ToggleVisibility => Ok(DomainOverlayAction::ToggleVisibility),
        v2::OverlayCommandKind::ToggleFastTravel => Ok(DomainOverlayAction::ToggleFastTravel),
        v2::OverlayCommandKind::ToggleBoss => Ok(DomainOverlayAction::ToggleBoss),
        v2::OverlayCommandKind::ToggleWanted => Ok(DomainOverlayAction::ToggleWanted),
        v2::OverlayCommandKind::ToggleDungeon => Ok(DomainOverlayAction::ToggleDungeon),
        v2::OverlayCommandKind::ToggleTower => Ok(DomainOverlayAction::ToggleTower),
        v2::OverlayCommandKind::ToggleEgg => Ok(DomainOverlayAction::ToggleEgg),
        v2::OverlayCommandKind::ToggleResources => Ok(DomainOverlayAction::ToggleResources),
        v2::OverlayCommandKind::ToggleSalvage => Ok(DomainOverlayAction::ToggleSalvage),
    }
}

pub fn encode_overlay_settings(
    value: &DomainOverlaySettings,
) -> Result<v2::OverlaySettings, DomainConversionError> {
    value.validate().map_err(|reason| {
        let field = reason.field();
        DomainConversionError::InvalidDomainValue {
            field,
            value: settings_value(value, field),
            reason: DomainValidationReason::Settings(reason),
        }
    })?;

    Ok(v2::OverlaySettings {
        enabled: value.enabled,
        auto_show: value.auto_show,
        rotation_mode: encode_rotation_mode(value.rotation_mode) as i32,
        input_mode: encode_input_mode(value.input_mode) as i32,
        display_mode: encode_display_mode(value.display_mode) as i32,
        opacity: value.opacity,
        diameter_px: value.diameter_px,
        zoom: value.zoom,
        normalized_x: value.normalized_x,
        normalized_y: value.normalized_y,
        fps_profile: encode_fps_profile(value.fps_profile) as i32,
        poi_filters: Some(v2::PoiFilters {
            fast_travel: value.poi_filters.fast_travel,
            boss: value.poi_filters.boss,
            dungeon: value.poi_filters.dungeon,
            wanted: value.poi_filters.wanted,
            enabled_layer_ids: value.poi_filters.enabled_layer_ids.clone(),
            selected_pal_ids: value.poi_filters.selected_pal_ids.clone(),
            night_only: value.poi_filters.night_only,
        }),
        hotkey_bindings: Some(v2::HotkeyBindings {
            overlay_visibility: value.hotkey_bindings.overlay_visibility.map(wire_hotkey),
            rotation_toggle: value.hotkey_bindings.rotation_toggle.map(wire_hotkey),
            temporary_interaction: value.hotkey_bindings.temporary_interaction.map(wire_hotkey),
            interaction_lock: value.hotkey_bindings.interaction_lock.map(wire_hotkey),
        }),
    })
}

fn wire_hotkey(value: DomainHotkeyChord) -> v2::HotkeyChord {
    v2::HotkeyChord {
        modifiers: Some(v2::HotkeyModifiers {
            control: value.modifiers.control,
            alt: value.modifiers.alt,
            shift: value.modifiers.shift,
            windows: value.modifiers.windows,
        }),
        virtual_key: u32::from(value.virtual_key),
    }
}

pub fn decode_overlay_settings(
    value: v2::OverlaySettings,
) -> Result<DomainOverlaySettings, DomainConversionError> {
    let poi_filters = value
        .poi_filters
        .ok_or(DomainConversionError::MissingMessage {
            field: "poi_filters",
        })?;
    let hotkey_bindings = value
        .hotkey_bindings
        .ok_or(DomainConversionError::MissingMessage {
            field: "hotkey_bindings",
        })?;

    let settings = DomainOverlaySettings {
        enabled: value.enabled,
        auto_show: value.auto_show,
        rotation_mode: checked_enum("rotation_mode", value.rotation_mode, decode_rotation_mode)?,
        input_mode: checked_enum("input_mode", value.input_mode, decode_input_mode)?,
        display_mode: checked_enum("display_mode", value.display_mode, decode_display_mode)?,
        opacity: value.opacity,
        diameter_px: value.diameter_px,
        zoom: value.zoom,
        normalized_x: value.normalized_x,
        normalized_y: value.normalized_y,
        fps_profile: checked_enum("fps_profile", value.fps_profile, decode_fps_profile)?,
        poi_filters: DomainPoiFilters {
            fast_travel: poi_filters.fast_travel,
            boss: poi_filters.boss,
            wanted: poi_filters.wanted,
            dungeon: poi_filters.dungeon,
            enabled_layer_ids: poi_filters.enabled_layer_ids,
            selected_pal_ids: poi_filters.selected_pal_ids,
            night_only: poi_filters.night_only,
        },
        hotkey_bindings: DomainHotkeyBindings {
            overlay_visibility: checked_hotkey(
                "hotkey_bindings.overlay_visibility",
                hotkey_bindings.overlay_visibility,
            )?,
            rotation_toggle: checked_hotkey(
                "hotkey_bindings.rotation_toggle",
                hotkey_bindings.rotation_toggle,
            )?,
            temporary_interaction: checked_hotkey(
                "hotkey_bindings.temporary_interaction",
                hotkey_bindings.temporary_interaction,
            )?,
            interaction_lock: checked_hotkey(
                "hotkey_bindings.interaction_lock",
                hotkey_bindings.interaction_lock,
            )?,
        },
    };

    settings.validate().map_err(|reason| {
        let field = reason.field();
        DomainConversionError::InvalidDomainValue {
            field,
            value: settings_value(&settings, field),
            reason: DomainValidationReason::Settings(reason),
        }
    })?;
    Ok(settings)
}

fn checked_enum<Wire, Domain>(
    field: &'static str,
    raw: i32,
    decode: fn(Wire) -> Result<Domain, DomainConversionError>,
) -> Result<Domain, DomainConversionError>
where
    Wire: TryFrom<i32> + Copy,
{
    let wire = Wire::try_from(raw)
        .map_err(|_| DomainConversionError::InvalidEnum { field, value: raw })?;
    decode(wire)
}

fn checked_hotkey(
    field: &'static str,
    value: Option<v2::HotkeyChord>,
) -> Result<Option<DomainHotkeyChord>, DomainConversionError> {
    value
        .map(|value| {
            let modifiers = value
                .modifiers
                .ok_or(DomainConversionError::MissingMessage {
                    field: hotkey_modifiers_field(field),
                })?;
            let virtual_key = u16::try_from(value.virtual_key).map_err(|_| {
                DomainConversionError::OutOfRangeU32 {
                    field: hotkey_key_field(field),
                    value: value.virtual_key,
                }
            })?;
            Ok(DomainHotkeyChord {
                modifiers: DomainHotkeyModifiers {
                    control: modifiers.control,
                    alt: modifiers.alt,
                    shift: modifiers.shift,
                    windows: modifiers.windows,
                },
                virtual_key,
            })
        })
        .transpose()
}

fn hotkey_modifiers_field(field: &'static str) -> &'static str {
    match field {
        "hotkey_bindings.overlay_visibility" => "hotkey_bindings.overlay_visibility.modifiers",
        "hotkey_bindings.rotation_toggle" => "hotkey_bindings.rotation_toggle.modifiers",
        "hotkey_bindings.temporary_interaction" => {
            "hotkey_bindings.temporary_interaction.modifiers"
        }
        "hotkey_bindings.interaction_lock" => "hotkey_bindings.interaction_lock.modifiers",
        _ => "hotkey_bindings.modifiers",
    }
}

fn hotkey_key_field(field: &'static str) -> &'static str {
    match field {
        "hotkey_bindings.overlay_visibility" => "hotkey_bindings.overlay_visibility.virtual_key",
        "hotkey_bindings.rotation_toggle" => "hotkey_bindings.rotation_toggle.virtual_key",
        "hotkey_bindings.temporary_interaction" => {
            "hotkey_bindings.temporary_interaction.virtual_key"
        }
        "hotkey_bindings.interaction_lock" => "hotkey_bindings.interaction_lock.virtual_key",
        _ => "hotkey_bindings.virtual_key",
    }
}

fn settings_value(settings: &DomainOverlaySettings, field: &str) -> ConversionValue {
    match field {
        "opacity" => ConversionValue::F32Bits(settings.opacity.to_bits()),
        "diameter_px" => ConversionValue::U32(settings.diameter_px),
        "zoom" => ConversionValue::F32Bits(settings.zoom.to_bits()),
        "normalized_x" => ConversionValue::F32Bits(settings.normalized_x.to_bits()),
        "normalized_y" => ConversionValue::F32Bits(settings.normalized_y.to_bits()),
        "hotkey_bindings.overlay_visibility" => {
            hotkey_value(settings.hotkey_bindings.overlay_visibility)
        }
        "hotkey_bindings.rotation_toggle" => hotkey_value(settings.hotkey_bindings.rotation_toggle),
        "hotkey_bindings.temporary_interaction" => {
            hotkey_value(settings.hotkey_bindings.temporary_interaction)
        }
        "hotkey_bindings.interaction_lock" => {
            hotkey_value(settings.hotkey_bindings.interaction_lock)
        }
        _ => ConversionValue::Missing,
    }
}

fn hotkey_value(value: Option<DomainHotkeyChord>) -> ConversionValue {
    value.map_or(ConversionValue::Missing, |chord| {
        ConversionValue::U32(u32::from(chord.virtual_key))
    })
}

pub fn encode_window_snapshot(value: &DomainWindowSnapshot) -> v2::WindowSnapshot {
    v2::WindowSnapshot {
        process_id: value.process_id(),
        client_left: value.client_left(),
        client_top: value.client_top(),
        client_width: value.client_width(),
        client_height: value.client_height(),
        dpi: value.dpi(),
        visible: value.visible(),
        active: value.active(),
        minimized: value.minimized(),
    }
}

pub fn decode_window_snapshot(
    value: v2::WindowSnapshot,
) -> Result<DomainWindowSnapshot, DomainConversionError> {
    DomainWindowSnapshot::new(
        value.process_id,
        value.client_left,
        value.client_top,
        value.client_width,
        value.client_height,
        value.dpi,
        value.visible,
        value.active,
        value.minimized,
    )
    .map_err(|reason| DomainConversionError::InvalidDomainValue {
        field: reason.field(),
        value: match reason {
            WindowValidationError::VisibleWidthZero => ConversionValue::U32(value.client_width),
            WindowValidationError::VisibleHeightZero => ConversionValue::U32(value.client_height),
            WindowValidationError::VisibleDpiZero => ConversionValue::U32(value.dpi),
        },
        reason: DomainValidationReason::Window(reason),
    })
}

pub fn encode_position_sample(value: &PositionSample) -> v2::PositionData {
    v2::PositionData {
        world_alias: value.world_alias().to_owned(),
        subject_id: value.subject_id().to_vec(),
        source_boot_id: value.agent_boot_id().to_vec(),
        source_connection_generation: value.source_connection_generation(),
        sequence: value.sequence(),
        x: value.x(),
        y: value.y(),
        z: value.z(),
        heading_degrees: value.heading_degrees(),
        age_at_receive_upper_bound_ms: value.clock().age_at_receive_upper_bound_ms,
        received_at_monotonic_ms: value.clock().received_at_monotonic_ms,
    }
}

pub fn decode_position_sample(
    value: v2::PositionData,
) -> Result<PositionSample, DomainConversionError> {
    PositionSample::new(
        value.world_alias.clone(),
        &value.subject_id,
        &value.source_boot_id,
        value.source_connection_generation,
        value.sequence,
        value.x,
        value.y,
        value.z,
        value.heading_degrees,
        SampleClock::received_with_age(
            value.age_at_receive_upper_bound_ms,
            value.received_at_monotonic_ms,
        ),
    )
    .map_err(|reason| DomainConversionError::InvalidDomainValue {
        field: position_wire_field(reason),
        value: position_value(&value, reason),
        reason: DomainValidationReason::Position(reason),
    })
}

const fn position_wire_field(reason: PositionValidationError) -> &'static str {
    match reason {
        PositionValidationError::EmptyAgentBootId => "source_boot_id",
        _ => reason.field(),
    }
}

fn position_value(value: &v2::PositionData, reason: PositionValidationError) -> ConversionValue {
    match reason {
        PositionValidationError::EmptyWorldAlias
        | PositionValidationError::EmptySubjectId
        | PositionValidationError::EmptyAgentBootId => ConversionValue::Empty,
        PositionValidationError::NonFiniteX => ConversionValue::F64Bits(value.x.to_bits()),
        PositionValidationError::NonFiniteY => ConversionValue::F64Bits(value.y.to_bits()),
        PositionValidationError::NonFiniteZ => ConversionValue::F64Bits(value.z.to_bits()),
        PositionValidationError::NonFiniteHeading => ConversionValue::F32Bits(
            value
                .heading_degrees
                .expect("NonFiniteHeading requires a present heading")
                .to_bits(),
        ),
    }
}
