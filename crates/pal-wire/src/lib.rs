//! Transport-free Protobuf messages and deterministic content identities.

#![forbid(unsafe_code)]

mod canonical;

pub mod v2 {
    include!(concat!(env!("OUT_DIR"), "/palcompanion.v2.rs"));
}

pub use canonical::{
    CanonicalError, MAX_CANONICAL_CONTENT_LEN, MAX_PROJECTION_PAGE_COUNT,
    MAX_PROJECTION_PAGE_ENCODED_LEN, MAX_PROJECTION_PAGE_ENTRIES, PaginatedCloudProfile,
    canonical_cloud_profile_projection_content, canonical_cloud_profile_projection_identity,
    canonical_snapshot_content, cloud_profile_projection_id, paginate_cloud_profile,
    personal_snapshot_id,
};
