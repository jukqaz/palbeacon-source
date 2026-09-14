//! Deterministic, transport-free product services shared by the Windows core
//! and the private Cloudflare Worker.
//!
//! Authentication tokens and persistence bindings deliberately live outside
//! this crate. Callers must construct [`VerifiedAccessIdentity`] only after
//! validating the Cloudflare Access assertion.

#![forbid(unsafe_code)]

pub mod access;
pub mod assistant;
pub mod breeding;

pub use access::{
    AccessRole, AuthorizationError, PrincipalContext, ProtectedCapability, VerifiedAccessIdentity,
};
pub use assistant::{
    AssistantEvidence, AssistantEvidenceKind, AssistantFallback, AssistantGroundingError,
    GroundedAssistantRequest, GroundedAssistantResponse, GroundedPrompt, MAX_EVIDENCE_ROWS,
    MAX_EVIDENCE_TEXT_BYTES, MAX_QUESTION_BYTES,
};
pub use breeding::{
    BreedingError, BreedingPlan, BreedingPlanNode, BreedingPlanSource, BreedingRule, OwnedPalSeed,
    PassiveCoverage, solve_owned_breeding,
};
pub use pal_knowledge_graph::{
    GraphDirection, KnowledgeContextPack, KnowledgeEdge, KnowledgeEvidenceRef, KnowledgeGraph,
    KnowledgeGraphError, KnowledgeGraphIdentity, KnowledgeGraphQuery, KnowledgeNode,
    KnowledgeNodeKind, KnowledgeQuality, KnowledgeQueryIntent, KnowledgeRelationKind,
    KnowledgeRetrievalPlan, KnowledgeWikiPage, WikiClaim, WikiPageStatus,
};
