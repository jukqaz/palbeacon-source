#![forbid(unsafe_code)]

mod build;
mod ids;

pub use build::GameBuildId;
pub use ids::{
    AnalysisId, ApprovalId, ArtifactId, BaseId, BuildingId, ClientInstanceId, DataSourceId,
    DatasetManifestId, EvidenceRef, FarmMethodId, GameSessionId, IdError, ItemId, LocationId,
    MapLayerId, MechanicsModelId, OwnerSubjectId, PalInstanceId, PassiveId, PersonalSnapshotId,
    PoiId, PrincipalId, ProjectionId, RecipeId, ReleaseManifestId, RequestId, SkillId,
    SolverVersion, SourceInstallId, SpeciesId, TechnologyId, WorkTypeId, WorldId,
};
