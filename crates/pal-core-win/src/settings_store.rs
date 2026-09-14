use pal_domain::{
    InputMode, OverlayAction, OverlaySettings, SettingsValidationError, reduce_overlay_action,
};
use pal_protocol::{DomainConversionError, decode_overlay_settings, v2};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq)]
pub struct StoreSnapshot {
    settings: OverlaySettings,
    version: u64,
}

impl StoreSnapshot {
    pub const fn settings(&self) -> &OverlaySettings {
        &self.settings
    }

    pub const fn version(&self) -> u64 {
        self.version
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum SettingsStoreError {
    #[error("initial settings are invalid")]
    InvalidInitial(SettingsValidationError),
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PatchRejection {
    #[error("candidate settings are missing")]
    MissingCandidate,
    #[error("candidate settings are invalid")]
    InvalidCandidate(DomainConversionError),
    #[error("changed field mask is empty")]
    EmptyFieldMask,
    #[error("changed field is unspecified")]
    UnspecifiedField,
    #[error("changed field is unknown")]
    UnknownField,
    #[error("changed field is duplicated")]
    DuplicateField,
    #[error("effective settings are invalid")]
    InvalidEffective(SettingsValidationError),
    #[error("settings version overflow")]
    VersionOverflow,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatchResult {
    Applied(StoreSnapshot),
    VersionConflict(StoreSnapshot),
    Rejected {
        current: StoreSnapshot,
        reason: PatchRejection,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ActionResult {
    Applied(StoreSnapshot),
    NoOp(StoreSnapshot),
    VersionConflict(StoreSnapshot),
    Rejected {
        current: StoreSnapshot,
        reason: PatchRejection,
    },
}

pub struct SettingsStore {
    settings: OverlaySettings,
    version: u64,
}

impl SettingsStore {
    pub fn new(settings: OverlaySettings) -> Result<Self, SettingsStoreError> {
        settings
            .validate()
            .map_err(SettingsStoreError::InvalidInitial)?;
        Ok(Self {
            settings,
            version: 1,
        })
    }

    pub const fn settings(&self) -> &OverlaySettings {
        &self.settings
    }

    pub const fn version(&self) -> u64 {
        self.version
    }

    pub fn snapshot(&self) -> StoreSnapshot {
        StoreSnapshot {
            settings: self.settings.clone(),
            version: self.version,
        }
    }

    pub fn apply_patch(
        &mut self,
        expected_version: u64,
        candidate: v2::OverlaySettings,
        changed_fields: &[i32],
    ) -> PatchResult {
        if expected_version != self.version {
            return PatchResult::VersionConflict(self.snapshot());
        }

        let candidate = match decode_overlay_settings(candidate) {
            Ok(candidate) => candidate,
            Err(error) => return self.rejected(PatchRejection::InvalidCandidate(error)),
        };
        if let Err(error) = validate_fields(changed_fields) {
            return self.rejected(error);
        }
        let Some(next_version) = self.version.checked_add(1) else {
            return self.rejected(PatchRejection::VersionOverflow);
        };

        let mut effective = self.settings.clone();
        for &raw in changed_fields {
            let field = v2::OverlaySettingField::try_from(raw)
                .expect("the complete field mask was validated before mutation");
            copy_field(&mut effective, &candidate, field);
        }
        if let Err(error) = effective.validate() {
            return self.rejected(PatchRejection::InvalidEffective(error));
        }

        self.settings = effective;
        self.version = next_version;
        PatchResult::Applied(self.snapshot())
    }

    pub fn apply_action(&mut self, expected_version: u64, action: OverlayAction) -> ActionResult {
        if expected_version != self.version {
            return ActionResult::VersionConflict(self.snapshot());
        }
        let Some(next_version) = self.version.checked_add(1) else {
            return self.rejected_action(PatchRejection::VersionOverflow);
        };
        let effective = reduce_overlay_action(&self.settings, action);
        if effective == self.settings {
            return ActionResult::NoOp(self.snapshot());
        }
        if let Err(error) = effective.validate() {
            return self.rejected_action(PatchRejection::InvalidEffective(error));
        }
        self.settings = effective;
        self.version = next_version;
        ActionResult::Applied(self.snapshot())
    }

    pub fn force_locked(&mut self) -> Result<StoreSnapshot, PatchRejection> {
        if self.settings.input_mode == InputMode::Locked {
            return Ok(self.snapshot());
        }
        let next_version = self
            .version
            .checked_add(1)
            .ok_or(PatchRejection::VersionOverflow)?;
        let mut effective = self.settings.clone();
        effective.input_mode = InputMode::Locked;
        effective
            .validate()
            .map_err(PatchRejection::InvalidEffective)?;
        self.settings = effective;
        self.version = next_version;
        Ok(self.snapshot())
    }

    pub(crate) fn reject_missing_candidate(&self) -> PatchResult {
        self.rejected(PatchRejection::MissingCandidate)
    }

    fn rejected(&self, reason: PatchRejection) -> PatchResult {
        PatchResult::Rejected {
            current: self.snapshot(),
            reason,
        }
    }

    fn rejected_action(&self, reason: PatchRejection) -> ActionResult {
        ActionResult::Rejected {
            current: self.snapshot(),
            reason,
        }
    }
}

fn validate_fields(changed_fields: &[i32]) -> Result<(), PatchRejection> {
    if changed_fields.is_empty() {
        return Err(PatchRejection::EmptyFieldMask);
    }
    for (index, &raw) in changed_fields.iter().enumerate() {
        let field =
            v2::OverlaySettingField::try_from(raw).map_err(|_| PatchRejection::UnknownField)?;
        if field == v2::OverlaySettingField::Unspecified {
            return Err(PatchRejection::UnspecifiedField);
        }
        if changed_fields[..index].contains(&raw) {
            return Err(PatchRejection::DuplicateField);
        }
    }
    Ok(())
}

fn copy_field(
    target: &mut OverlaySettings,
    candidate: &OverlaySettings,
    field: v2::OverlaySettingField,
) {
    match field {
        v2::OverlaySettingField::Unspecified => unreachable!("validated field mask"),
        v2::OverlaySettingField::Enabled => target.enabled = candidate.enabled,
        v2::OverlaySettingField::AutoShow => target.auto_show = candidate.auto_show,
        v2::OverlaySettingField::RotationMode => {
            target.rotation_mode = candidate.rotation_mode;
        }
        v2::OverlaySettingField::InputMode => target.input_mode = candidate.input_mode,
        v2::OverlaySettingField::DisplayMode => target.display_mode = candidate.display_mode,
        v2::OverlaySettingField::Opacity => target.opacity = candidate.opacity,
        v2::OverlaySettingField::DiameterPx => target.diameter_px = candidate.diameter_px,
        v2::OverlaySettingField::Zoom => target.zoom = candidate.zoom,
        v2::OverlaySettingField::NormalizedX => target.normalized_x = candidate.normalized_x,
        v2::OverlaySettingField::NormalizedY => target.normalized_y = candidate.normalized_y,
        v2::OverlaySettingField::FpsProfile => target.fps_profile = candidate.fps_profile,
        v2::OverlaySettingField::PoiFilters => {
            target.poi_filters = candidate.poi_filters.clone();
        }
        v2::OverlaySettingField::HotkeyBindings => {
            target.hotkey_bindings = candidate.hotkey_bindings;
        }
    }
}

#[cfg(test)]
mod tests {
    use pal_domain::{InputMode, OverlaySettings};
    use pal_protocol::v2::OverlaySettingField;

    use super::{PatchRejection, PatchResult, SettingsStore};

    #[test]
    fn version_overflow_rejects_without_mutation() {
        let initial = OverlaySettings {
            input_mode: InputMode::PinnedInteractive,
            ..OverlaySettings::default()
        };
        let mut store = SettingsStore {
            settings: initial.clone(),
            version: u64::MAX,
        };
        let mut candidate = initial;
        candidate.input_mode = InputMode::Locked;
        let wire = pal_protocol::encode_overlay_settings(&candidate).unwrap();
        let before = store.snapshot();

        assert!(matches!(
            store.apply_patch(u64::MAX, wire, &[OverlaySettingField::InputMode as i32]),
            PatchResult::Rejected { .. }
        ));
        assert_eq!(store.snapshot(), before);
    }

    #[test]
    fn disconnect_version_exhaustion_fails_without_same_version_mutation() {
        let initial = OverlaySettings {
            input_mode: InputMode::PinnedInteractive,
            ..OverlaySettings::default()
        };
        let mut store = SettingsStore {
            settings: initial,
            version: u64::MAX,
        };
        let before = store.snapshot();

        assert_eq!(store.force_locked(), Err(PatchRejection::VersionOverflow));
        assert_eq!(store.snapshot(), before);
    }
}
