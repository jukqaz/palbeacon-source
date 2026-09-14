use std::fmt;

use thiserror::Error;

/// Errors returned while parsing a shared domain identifier.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdError {
    #[error("identifier must not be empty")]
    Empty,
    #[error("identifier exceeds the 128-byte limit")]
    TooLong,
    #[error("identifier is not canonical")]
    NonCanonical,
    #[error("identifier has invalid length: expected {expected}, got {actual}")]
    InvalidLength { expected: usize, actual: usize },
}

fn parse_text_id(value: String) -> Result<String, IdError> {
    if value.is_empty() {
        return Err(IdError::Empty);
    }
    if value.len() > 128 {
        return Err(IdError::TooLong);
    }
    if value.trim() != value || !value.is_ascii() {
        return Err(IdError::NonCanonical);
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(IdError::NonCanonical);
    }
    Ok(value)
}

macro_rules! text_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, IdError> {
                parse_text_id(value.into()).map(Self)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

text_id!(GameBuildId);
text_id!(WorldId);
text_id!(SpeciesId);
text_id!(PassiveId);
text_id!(SkillId);
text_id!(ItemId);
text_id!(RecipeId);
text_id!(LocationId);
text_id!(TechnologyId);
text_id!(BuildingId);
text_id!(WorkTypeId);
text_id!(MapLayerId);
text_id!(PoiId);
text_id!(FarmMethodId);
text_id!(DataSourceId);
text_id!(MechanicsModelId);
text_id!(SolverVersion);

macro_rules! binary_id {
    ($name:ident, $length:expr) => {
        #[derive(PartialEq, Eq)]
        pub struct $name([u8; $length]);

        impl $name {
            pub fn from_bytes(value: [u8; $length]) -> Self {
                Self(value)
            }

            pub fn from_slice(value: &[u8]) -> Result<Self, IdError> {
                let actual = value.len();
                let bytes = value.try_into().map_err(|_| IdError::InvalidLength {
                    expected: $length,
                    actual,
                })?;
                Ok(Self(bytes))
            }

            pub fn as_bytes(&self) -> &[u8; $length] {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                for byte in self.0 {
                    write!(formatter, "{byte:02x}")?;
                }
                Ok(())
            }
        }
    };
}

binary_id!(PalInstanceId, 16);
binary_id!(BaseId, 16);
binary_id!(SourceInstallId, 16);
binary_id!(ClientInstanceId, 16);
binary_id!(GameSessionId, 16);
binary_id!(RequestId, 16);
binary_id!(ApprovalId, 16);
binary_id!(PrincipalId, 16);

binary_id!(OwnerSubjectId, 32);
binary_id!(DatasetManifestId, 32);
binary_id!(PersonalSnapshotId, 32);
binary_id!(ProjectionId, 32);
binary_id!(AnalysisId, 32);
binary_id!(ArtifactId, 32);
binary_id!(ReleaseManifestId, 32);
binary_id!(EvidenceRef, 32);
