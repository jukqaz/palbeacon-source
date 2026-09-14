#![forbid(unsafe_code)]

use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use pal_domain::{
    DisplayMode, FpsProfile, HotkeyBindings, HotkeyChord, HotkeyModifiers, InputMode,
    OverlaySettings, PoiFilters, RotationMode,
};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;

pub const CONTROL_SCHEMA: &str = "palbeacon.overlay_control.v2";
pub const POI_SCHEMA: &str = "pal_companion.overlay_pois.v1";
const MAX_CONTROL_BYTES: u64 = 16 * 1024;
const MAX_POI_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRotationMode {
    #[default]
    NorthUp,
    HeadingUp,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlInputMode {
    #[default]
    Locked,
    PinnedInteractive,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlDisplayMode {
    #[default]
    MiniMap,
    ExpandedMap,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlFpsProfile {
    Thirty,
    #[default]
    Sixty,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControlHotkeyModifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub windows: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlHotkeyChord {
    pub modifiers: ControlHotkeyModifiers,
    pub virtual_key: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControlHotkeyBindings {
    pub overlay_visibility: Option<ControlHotkeyChord>,
    pub rotation_toggle: Option<ControlHotkeyChord>,
    pub temporary_interaction: Option<ControlHotkeyChord>,
    pub interaction_lock: Option<ControlHotkeyChord>,
}

impl Default for ControlHotkeyBindings {
    fn default() -> Self {
        let modifiers = ControlHotkeyModifiers::default();
        Self {
            overlay_visibility: Some(ControlHotkeyChord {
                modifiers,
                virtual_key: 0x77,
            }),
            rotation_toggle: Some(ControlHotkeyChord {
                modifiers: ControlHotkeyModifiers {
                    control: true,
                    ..modifiers
                },
                virtual_key: 0x78,
            }),
            temporary_interaction: None,
            interaction_lock: None,
        }
    }
}

impl ControlHotkeyBindings {
    fn from_domain(bindings: HotkeyBindings) -> Self {
        Self {
            overlay_visibility: bindings
                .overlay_visibility
                .map(ControlHotkeyChord::from_domain),
            rotation_toggle: bindings
                .rotation_toggle
                .map(ControlHotkeyChord::from_domain),
            temporary_interaction: bindings
                .temporary_interaction
                .map(ControlHotkeyChord::from_domain),
            interaction_lock: bindings
                .interaction_lock
                .map(ControlHotkeyChord::from_domain),
        }
    }

    fn to_domain(self) -> HotkeyBindings {
        HotkeyBindings {
            overlay_visibility: self.overlay_visibility.map(ControlHotkeyChord::to_domain),
            rotation_toggle: self.rotation_toggle.map(ControlHotkeyChord::to_domain),
            temporary_interaction: self
                .temporary_interaction
                .map(ControlHotkeyChord::to_domain),
            interaction_lock: self.interaction_lock.map(ControlHotkeyChord::to_domain),
        }
    }
}

impl ControlHotkeyChord {
    fn from_domain(chord: HotkeyChord) -> Self {
        Self {
            modifiers: ControlHotkeyModifiers {
                control: chord.modifiers.control,
                alt: chord.modifiers.alt,
                shift: chord.modifiers.shift,
                windows: chord.modifiers.windows,
            },
            virtual_key: chord.virtual_key,
        }
    }

    fn to_domain(self) -> HotkeyChord {
        HotkeyChord {
            modifiers: HotkeyModifiers {
                control: self.modifiers.control,
                alt: self.modifiers.alt,
                shift: self.modifiers.shift,
                windows: self.modifiers.windows,
            },
            virtual_key: self.virtual_key,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControlPoiFilters {
    pub fast_travel: bool,
    pub boss: bool,
    pub wanted: bool,
    pub dungeon: bool,
    pub enabled_layer_ids: Vec<String>,
    pub selected_pal_ids: Vec<String>,
    pub night_only: bool,
}

impl Default for ControlPoiFilters {
    fn default() -> Self {
        Self {
            fast_travel: true,
            boss: true,
            wanted: false,
            dungeon: true,
            enabled_layer_ids: vec![
                "map-unlock".to_owned(),
                "poi".to_owned(),
                "tower".to_owned(),
            ],
            selected_pal_ids: Vec::new(),
            night_only: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControlSettings {
    pub enabled: bool,
    pub rotation_mode: ControlRotationMode,
    pub input_mode: ControlInputMode,
    pub display_mode: ControlDisplayMode,
    pub opacity: f32,
    pub diameter_px: u32,
    pub zoom: f32,
    pub fps_profile: ControlFpsProfile,
    pub poi_filters: ControlPoiFilters,
    pub hotkey_bindings: ControlHotkeyBindings,
}

impl Default for ControlSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            rotation_mode: ControlRotationMode::NorthUp,
            input_mode: ControlInputMode::Locked,
            display_mode: ControlDisplayMode::MiniMap,
            opacity: 0.92,
            diameter_px: 320,
            zoom: 1.0,
            fps_profile: ControlFpsProfile::Sixty,
            poi_filters: ControlPoiFilters::default(),
            hotkey_bindings: ControlHotkeyBindings::default(),
        }
    }
}

impl ControlSettings {
    pub fn from_domain(settings: &OverlaySettings) -> Self {
        Self {
            enabled: settings.enabled,
            rotation_mode: match settings.rotation_mode {
                RotationMode::NorthUp => ControlRotationMode::NorthUp,
                RotationMode::HeadingUp => ControlRotationMode::HeadingUp,
            },
            input_mode: match settings.input_mode {
                InputMode::Locked | InputMode::TemporaryInteractive | InputMode::LayoutEdit => {
                    ControlInputMode::Locked
                }
                InputMode::PinnedInteractive => ControlInputMode::PinnedInteractive,
            },
            display_mode: match settings.display_mode {
                DisplayMode::MiniMap => ControlDisplayMode::MiniMap,
                DisplayMode::ExpandedMap => ControlDisplayMode::ExpandedMap,
            },
            opacity: settings.opacity,
            diameter_px: settings.diameter_px,
            zoom: settings.zoom,
            fps_profile: match settings.fps_profile {
                FpsProfile::Auto | FpsProfile::Sixty => ControlFpsProfile::Sixty,
                FpsProfile::Thirty => ControlFpsProfile::Thirty,
            },
            poi_filters: ControlPoiFilters {
                fast_travel: settings.poi_filters.fast_travel,
                boss: settings.poi_filters.boss,
                wanted: settings.poi_filters.wanted,
                dungeon: settings.poi_filters.dungeon,
                enabled_layer_ids: settings.poi_filters.enabled_layer_ids.clone(),
                selected_pal_ids: settings.poi_filters.selected_pal_ids.clone(),
                night_only: settings.poi_filters.night_only,
            },
            hotkey_bindings: ControlHotkeyBindings::from_domain(settings.hotkey_bindings),
        }
    }

    pub fn to_domain(&self) -> Result<OverlaySettings, ControlError> {
        let mut settings = OverlaySettings {
            enabled: self.enabled,
            auto_show: true,
            rotation_mode: match self.rotation_mode {
                ControlRotationMode::NorthUp => RotationMode::NorthUp,
                ControlRotationMode::HeadingUp => RotationMode::HeadingUp,
            },
            input_mode: match self.input_mode {
                ControlInputMode::Locked => InputMode::Locked,
                ControlInputMode::PinnedInteractive => InputMode::PinnedInteractive,
            },
            display_mode: match self.display_mode {
                ControlDisplayMode::MiniMap => DisplayMode::MiniMap,
                ControlDisplayMode::ExpandedMap => DisplayMode::ExpandedMap,
            },
            opacity: self.opacity,
            diameter_px: self.diameter_px,
            zoom: self.zoom,
            normalized_x: 0.0,
            normalized_y: 0.0,
            fps_profile: match self.fps_profile {
                ControlFpsProfile::Thirty => FpsProfile::Thirty,
                ControlFpsProfile::Sixty => FpsProfile::Sixty,
            },
            poi_filters: PoiFilters {
                fast_travel: self.poi_filters.fast_travel,
                boss: self.poi_filters.boss,
                wanted: self.poi_filters.wanted,
                dungeon: self.poi_filters.dungeon,
                enabled_layer_ids: self.poi_filters.enabled_layer_ids.clone(),
                selected_pal_ids: self.poi_filters.selected_pal_ids.clone(),
                night_only: self.poi_filters.night_only,
            },
            hotkey_bindings: self.hotkey_bindings.to_domain(),
        };
        settings.poi_filters.remove_hidden_unverified_layers();
        settings
            .validate()
            .map_err(|error| ControlError::InvalidSettings(error.to_string()))?;
        Ok(settings)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayControlDocument {
    pub schema: String,
    pub version: u64,
    pub updated_at_unix_ms: u64,
    pub settings: ControlSettings,
}

impl Default for OverlayControlDocument {
    fn default() -> Self {
        Self {
            schema: CONTROL_SCHEMA.to_owned(),
            version: 1,
            updated_at_unix_ms: unix_ms(),
            settings: ControlSettings::default(),
        }
    }
}

impl OverlayControlDocument {
    pub fn validate(&self) -> Result<(), ControlError> {
        if self.schema != CONTROL_SCHEMA {
            return Err(ControlError::SchemaMismatch);
        }
        if self.version == 0 {
            return Err(ControlError::InvalidVersion);
        }
        self.settings.to_domain().map(|_| ())
    }

    pub fn next(settings: ControlSettings, current_version: u64) -> Result<Self, ControlError> {
        let version = current_version
            .checked_add(1)
            .ok_or(ControlError::InvalidVersion)?;
        let document = Self {
            schema: CONTROL_SCHEMA.to_owned(),
            version,
            updated_at_unix_ms: unix_ms(),
            settings,
        };
        document.validate()?;
        Ok(document)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayPoiKind {
    FastTravel,
    Boss,
    Dungeon,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayPoi {
    pub id: String,
    pub name: String,
    pub kind: OverlayPoiKind,
    pub world_x: f64,
    pub world_y: f64,
    #[serde(default)]
    pub icon_file: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayPoiCatalog {
    pub schema: String,
    pub game_build_id: u64,
    pub dataset_version: String,
    pub source_url: String,
    pub source_version: String,
    pub pois: Vec<OverlayPoi>,
}

impl OverlayPoiCatalog {
    pub fn validate(&self, expected_build_id: u64) -> Result<(), ControlError> {
        if self.schema != POI_SCHEMA {
            return Err(ControlError::PoiSchemaMismatch);
        }
        if self.game_build_id != expected_build_id {
            return Err(ControlError::PoiBuildMismatch);
        }
        if self.pois.len() > 10_000 {
            return Err(ControlError::PoiCountOutOfRange);
        }
        if self.pois.iter().any(|poi| {
            poi.id.is_empty()
                || poi.name.is_empty()
                || !poi.world_x.is_finite()
                || !poi.world_y.is_finite()
                || poi.icon_file.as_ref().is_some_and(|path| {
                    path.is_empty()
                        || Path::new(path).is_absolute()
                        || Path::new(path).components().any(|component| {
                            matches!(
                                component,
                                std::path::Component::ParentDir
                                    | std::path::Component::RootDir
                                    | std::path::Component::Prefix(_)
                            )
                        })
                })
        }) {
            return Err(ControlError::InvalidPoi);
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("the overlay control file could not be read or written")]
    Io(#[from] std::io::Error),
    #[error("the overlay control JSON is invalid")]
    Json(#[from] serde_json::Error),
    #[error("the overlay control file exceeds its size limit")]
    FileTooLarge,
    #[error("the overlay control schema does not match")]
    SchemaMismatch,
    #[error("the overlay control version is invalid")]
    InvalidVersion,
    #[error("overlay settings are invalid: {0}")]
    InvalidSettings(String),
    #[error("the POI schema does not match")]
    PoiSchemaMismatch,
    #[error("the POI game build does not match")]
    PoiBuildMismatch,
    #[error("the POI count exceeds its bound")]
    PoiCountOutOfRange,
    #[error("a POI row is invalid")]
    InvalidPoi,
}

pub fn default_control_path() -> PathBuf {
    control_path_from_local_data_root(
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir),
    )
}

fn control_path_from_local_data_root(root: PathBuf) -> PathBuf {
    root.join("PalBeacon")
        .join("config")
        .join("overlay-control-v2.json")
}

pub fn read_control(path: &Path) -> Result<OverlayControlDocument, ControlError> {
    let bytes = read_bounded(path, MAX_CONTROL_BYTES)?;
    let document: OverlayControlDocument = serde_json::from_slice(&bytes)?;
    document.validate()?;
    Ok(document)
}

pub fn read_or_create_control(path: &Path) -> Result<OverlayControlDocument, ControlError> {
    match read_control(path) {
        Ok(document) => {
            let sanitized = document.settings.to_domain()?;
            let sanitized_settings = ControlSettings::from_domain(&sanitized);
            if sanitized_settings == document.settings {
                return Ok(document);
            }
            let migrated = OverlayControlDocument::next(sanitized_settings, document.version)?;
            write_control(path, &migrated)?;
            Ok(migrated)
        }
        Err(ControlError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            let document = OverlayControlDocument::default();
            write_control(path, &document)?;
            Ok(document)
        }
        Err(error) => Err(error),
    }
}

pub fn write_control(path: &Path, document: &OverlayControlDocument) -> Result<(), ControlError> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_CONTROL_BYTES {
        return Err(ControlError::FileTooLarge);
    }
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "control path has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub fn read_poi_catalog(
    path: &Path,
    expected_build_id: u64,
) -> Result<OverlayPoiCatalog, ControlError> {
    let bytes = read_bounded(path, MAX_POI_BYTES)?;
    let catalog: OverlayPoiCatalog = serde_json::from_slice(&bytes)?;
    catalog.validate(expected_build_id)?;
    Ok(catalog)
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, ControlError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > maximum {
        return Err(ControlError::FileTooLarge);
    }
    let mut bytes = Vec::new();
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(ControlError::FileTooLarge);
    }
    Ok(bytes)
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_control_path_uses_the_current_product_and_schema_names() {
        assert_eq!(
            control_path_from_local_data_root(PathBuf::from("local-data")),
            PathBuf::from("local-data")
                .join("PalBeacon")
                .join("config")
                .join("overlay-control-v2.json")
        );
    }

    #[test]
    fn default_document_round_trips_and_validates() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("overlay.json");
        let document = read_or_create_control(&path).expect("create");
        assert_eq!(document.settings.diameter_px, 320);
        assert_eq!(
            document
                .settings
                .hotkey_bindings
                .rotation_toggle
                .expect("recommended rotation binding")
                .virtual_key,
            0x78
        );
        assert_eq!(read_control(&path).expect("read"), document);
    }

    #[test]
    fn write_control_atomically_replaces_an_existing_document() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("overlay.json");
        let initial = OverlayControlDocument::default();
        write_control(&path, &initial).expect("initial write");

        let replacement = OverlayControlDocument::next(initial.settings.clone(), initial.version)
            .expect("replacement");
        write_control(&path, &replacement).expect("replace");

        assert_eq!(read_control(&path).expect("read replacement"), replacement);
        assert_eq!(
            fs::read_dir(directory.path())
                .expect("read directory")
                .count(),
            1
        );
    }

    #[test]
    fn next_document_rejects_invalid_settings() {
        let settings = ControlSettings {
            zoom: 9.0,
            ..ControlSettings::default()
        };
        assert!(matches!(
            OverlayControlDocument::next(settings, 1),
            Err(ControlError::InvalidSettings(_))
        ));
    }

    #[test]
    fn control_settings_preserve_the_persistent_domain_subset() {
        let domain = OverlaySettings {
            enabled: false,
            rotation_mode: RotationMode::HeadingUp,
            input_mode: InputMode::PinnedInteractive,
            display_mode: DisplayMode::ExpandedMap,
            opacity: 0.71,
            diameter_px: 512,
            zoom: 1.75,
            fps_profile: FpsProfile::Thirty,
            poi_filters: PoiFilters {
                fast_travel: false,
                boss: true,
                wanted: false,
                dungeon: false,
                ..PoiFilters::default()
            },
            hotkey_bindings: HotkeyBindings {
                rotation_toggle: Some(HotkeyChord {
                    modifiers: HotkeyModifiers {
                        control: true,
                        shift: true,
                        ..HotkeyModifiers::default()
                    },
                    virtual_key: 0x78,
                }),
                ..HotkeyBindings::default()
            },
            ..OverlaySettings::default()
        };
        let restored = ControlSettings::from_domain(&domain)
            .to_domain()
            .expect("round trip");
        assert_eq!(restored.enabled, domain.enabled);
        assert_eq!(restored.rotation_mode, domain.rotation_mode);
        assert_eq!(restored.input_mode, domain.input_mode);
        assert_eq!(restored.display_mode, domain.display_mode);
        assert_eq!(restored.opacity, domain.opacity);
        assert_eq!(restored.diameter_px, domain.diameter_px);
        assert_eq!(restored.zoom, domain.zoom);
        assert_eq!(restored.fps_profile, domain.fps_profile);
        assert_eq!(restored.poi_filters, domain.poi_filters);
        assert_eq!(restored.hotkey_bindings, domain.hotkey_bindings);
    }

    #[test]
    fn previous_control_schema_is_rejected() {
        let document: OverlayControlDocument = serde_json::from_str(
            r#"{
                "schema":"pal_companion.overlay_control.v1",
                "version":1,
                "updated_at_unix_ms":1,
                "settings":{
                    "enabled":true,
                    "rotation_mode":"north_up",
                    "input_mode":"locked",
                    "display_mode":"mini_map",
                    "opacity":0.92,
                    "diameter_px":320,
                    "zoom":1.0,
                    "fps_profile":"sixty",
                    "poi_filters":{}
                }
            }"#,
        )
        .expect("legacy document");

        assert!(matches!(
            document.validate(),
            Err(ControlError::SchemaMismatch)
        ));
    }

    #[test]
    fn read_or_create_removes_hidden_unverified_layers_from_existing_settings() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("overlay.json");
        let document = OverlayControlDocument {
            version: 7,
            settings: ControlSettings {
                poi_filters: ControlPoiFilters {
                    enabled_layer_ids: vec![
                        "tower".to_owned(),
                        "captured-pal".to_owned(),
                        "anti-air-turret".to_owned(),
                        "medal".to_owned(),
                        "ancient-shrine".to_owned(),
                        "healing-spring".to_owned(),
                    ],
                    ..ControlPoiFilters::default()
                },
                ..ControlSettings::default()
            },
            ..OverlayControlDocument::default()
        };
        write_control(&path, &document).expect("write legacy settings");

        let migrated = read_or_create_control(&path).expect("migrate settings");

        assert_eq!(migrated.version, 8);
        assert_eq!(migrated.settings.poi_filters.enabled_layer_ids, ["tower"]);
        assert_eq!(read_control(&path).expect("persisted migration"), migrated);
    }
}
